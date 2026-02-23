use emu_common::Bus;
use crate::disk_ii::DiskII;
use crate::memory::Memory;
use crate::soft_switch::SoftSwitches;
use crate::keyboard::Keyboard;
use crate::speaker::Speaker;
use crate::video;
use emu_common::FrameBuffer;

/// Apple II bus - routes addresses to appropriate hardware.
pub struct Apple2Bus {
    pub memory: Memory,
    pub switches: SoftSwitches,
    pub keyboard: Keyboard,
    pub speaker: Speaker,
    pub disk_ii: DiskII,
    pub framebuffer: FrameBuffer,

    cycle_count: u64,
    cycles_per_frame: u64,
    frame_ready: bool,
    pub(crate) frame_count: u32,
    /// Debug: last known CPU registers (set by lib.rs before each step).
    pub(crate) debug_pc: u16,
    pub(crate) debug_sp: u8,
    pub(crate) debug_x: u8,
}

impl Apple2Bus {
    pub fn new() -> Self {
        Self {
            memory: Memory::new(),
            switches: SoftSwitches::new(),
            keyboard: Keyboard::new(),
            speaker: Speaker::new(),
            disk_ii: DiskII::new(),
            framebuffer: FrameBuffer::new(video::DISPLAY_WIDTH, video::DISPLAY_HEIGHT),
            cycle_count: 0,
            cycles_per_frame: 17030, // ~60fps at 1.023 MHz
            frame_ready: false,
            frame_count: 0,
            debug_pc: 0,
            debug_sp: 0xFF,
            debug_x: 0,
        }
    }

    pub fn is_frame_ready(&mut self) -> bool {
        let ready = self.frame_ready;
        self.frame_ready = false;
        ready
    }

    /// Simulate the Apple II "floating bus": return the byte the video hardware
    /// is currently fetching from display memory based on the cycle position.
    /// Many programs depend on this for correct soft-switch side effects.
    ///
    /// On real hardware, the video circuitry generates addresses continuously
    /// for all 262 scanlines (including VBL) and all 65 cycles per scanline
    /// (including HBL), following the same interleaved text row pattern.
    fn floating_bus(&self) -> u8 {
        // 65 cycles per scanline, 262 scanlines per frame
        let scanline = (self.cycle_count as usize / 65) % 262;
        let h_cycle = self.cycle_count as usize % 65;

        // The video hardware generates addresses continuously using interleaved
        // row offsets. During HBL (columns 40-64), the counter continues past
        // the visible 40 columns into the "hole" regions between text rows.
        // These holes contain ProDOS/BASIC data (often bytes < $80), which is
        // critical for correct emulation — software like Bitsy Bye relies on
        // the floating bus occasionally returning values with bit 7 clear.
        let text_row = (scanline / 8) % 24;

        let base: u16 = if self.switches.page2 && !self.switches.store80 {
            0x0800
        } else {
            0x0400
        };

        static TEXT_OFFSETS: [u16; 24] = [
            0x000, 0x080, 0x100, 0x180, 0x200, 0x280, 0x300, 0x380,
            0x028, 0x0A8, 0x128, 0x1A8, 0x228, 0x2A8, 0x328, 0x3A8,
            0x050, 0x0D0, 0x150, 0x1D0, 0x250, 0x2D0, 0x350, 0x3D0,
        ];

        // Don't wrap h_cycle — let it extend into hole regions during HBL
        let addr = (base + TEXT_OFFSETS[text_row] + h_cycle as u16) as usize;
        if addr < self.memory.ram.len() {
            self.memory.ram[addr]
        } else {
            0
        }
    }

    fn render_frame(&mut self) {
        self.frame_count = self.frame_count.wrapping_add(1);
        // Flash cursor at ~1.9Hz (16 frames on, 16 frames off at 60fps)
        let flash_on = (self.frame_count / 16) & 1 == 0;
        video::render(&mut self.framebuffer, &self.memory, &self.switches, flash_on);
    }

    /// Read with IIe banking: select main or auxiliary RAM based on soft switches.
    fn banked_read(&self, addr: u16) -> u8 {
        let use_aux = match addr {
            0x0000..=0x01FF => self.switches.altzp,
            0x0200..=0x03FF => self.switches.ramrd,
            0x0400..=0x07FF => {
                if self.switches.store80 {
                    self.switches.page2
                } else {
                    self.switches.ramrd
                }
            }
            0x0800..=0x1FFF => self.switches.ramrd,
            0x2000..=0x3FFF => {
                if self.switches.store80 && self.switches.hires {
                    self.switches.page2
                } else {
                    self.switches.ramrd
                }
            }
            0x4000..=0xBFFF => self.switches.ramrd,
            _ => false,
        };

        let val = if use_aux {
            self.memory.aux_ram[addr as usize]
        } else {
            self.memory.ram[addr as usize]
        };

        val
    }

    /// Write with IIe banking: select main or auxiliary RAM based on soft switches.
    fn banked_write(&mut self, addr: u16, val: u8) {
        let use_aux = match addr {
            0x0000..=0x01FF => self.switches.altzp,
            0x0200..=0x03FF => self.switches.ramwrt,
            0x0400..=0x07FF => {
                if self.switches.store80 {
                    self.switches.page2
                } else {
                    self.switches.ramwrt
                }
            }
            0x0800..=0x1FFF => self.switches.ramwrt,
            0x2000..=0x3FFF => {
                if self.switches.store80 && self.switches.hires {
                    self.switches.page2
                } else {
                    self.switches.ramwrt
                }
            }
            0x4000..=0xBFFF => self.switches.ramwrt,
            _ => false,
        };

        if use_aux {
            self.memory.aux_ram[addr as usize] = val;
        } else {
            self.memory.ram[addr as usize] = val;
        }
    }
}

impl Bus for Apple2Bus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0xBFFF => {
                let val = self.banked_read(addr);
                // Trace ProDOS MLI entry: CPU fetching instruction at $BF00
                // After JSR $BF00, the return address on stack points to the byte
                // before the inline parameters. Stack has: lo, hi (pushed by JSR).
                if addr == 0xBF00 && self.debug_pc == 0xBF00 {
                    let sp = self.debug_sp as u16;
                    // JSR pushes (PC+2) - 1, so return addr = stack[sp+1..sp+2]
                    // Must use banked_read for ALTZP-aware stack access
                    let ret_lo = self.banked_read(0x0100 + sp + 1) as u16;
                    let ret_hi = self.banked_read(0x0100 + sp + 2) as u16;
                    let ret_addr = (ret_hi << 8) | ret_lo;
                    // Inline parameters start at ret_addr + 1
                    let call_num = self.banked_read(ret_addr.wrapping_add(1));
                    let param_lo = self.banked_read(ret_addr.wrapping_add(2));
                    let param_hi = self.banked_read(ret_addr.wrapping_add(3));
                    let param_addr = (param_hi as u16) << 8 | param_lo as u16;
                    let call_name = match call_num {
                        0xC0 => "CREATE", 0xC1 => "DESTROY", 0xC2 => "RENAME",
                        0xC3 => "SET_FILE_INFO", 0xC4 => "GET_FILE_INFO",
                        0xC5 => "ONLINE", 0xC6 => "SET_PREFIX", 0xC7 => "GET_PREFIX",
                        0xC8 => "OPEN", 0xC9 => "NEWLINE", 0xCA => "READ",
                        0xCB => "WRITE", 0xCC => "CLOSE", 0xCD => "FLUSH",
                        0xCE => "SET_MARK", 0xCF => "GET_MARK",
                        0xD0 => "SET_EOF", 0xD1 => "GET_EOF", 0xD2 => "SET_BUF",
                        0xD3 => "GET_BUF", 0x65 => "QUIT",
                        0x80 => "READ_BLOCK", 0x81 => "WRITE_BLOCK",
                        0x82 => "GET_TIME",
                        _ => "UNKNOWN",
                    };
                    log::info!("MLI ${:02X} ({}) param=${:04X} frame={} caller=${:04X} altzp={} ramrd={} ramwrt={}",
                        call_num, call_name, param_addr, self.frame_count, ret_addr,
                        self.switches.altzp, self.switches.ramrd, self.switches.ramwrt);
                    // ProDOS param blocks: +0=param_count, then fields
                    // For READ calls ($CA): +0=cnt, +1=ref_num, +2/3=buffer, +4/5=request_count
                    if call_num == 0xCA {
                        let ref_num = self.banked_read(param_addr.wrapping_add(1));
                        let buf_lo = self.banked_read(param_addr.wrapping_add(2));
                        let buf_hi = self.banked_read(param_addr.wrapping_add(3));
                        let req_lo = self.banked_read(param_addr.wrapping_add(4));
                        let req_hi = self.banked_read(param_addr.wrapping_add(5));
                        let buf_addr = (buf_hi as u16) << 8 | buf_lo as u16;
                        let req_count = (req_hi as u16) << 8 | req_lo as u16;
                        log::info!("  READ: ref={} buf=${:04X} req=${:04X} ({})",
                            ref_num, buf_addr, req_count, req_count);
                    }
                    // For OPEN calls ($C8): +0=cnt, +1/2=pathname, +3/4=io_buffer, +5=ref_num(out)
                    if call_num == 0xC8 {
                        let path_lo = self.banked_read(param_addr.wrapping_add(1));
                        let path_hi = self.banked_read(param_addr.wrapping_add(2));
                        let path_addr = (path_hi as u16) << 8 | path_lo as u16;
                        let buf_lo = self.banked_read(param_addr.wrapping_add(3));
                        let buf_hi = self.banked_read(param_addr.wrapping_add(4));
                        let io_buf = (buf_hi as u16) << 8 | buf_lo as u16;
                        // Read pathname (Pascal string: length byte + chars)
                        let path_len = self.banked_read(path_addr) as usize;
                        let path: String = (1..=path_len.min(64)).map(|i| {
                            let b = self.banked_read(path_addr.wrapping_add(i as u16));
                            if b >= 0x20 && b < 0x7F { b as char } else { '.' }
                        }).collect();
                        log::info!("  OPEN: path=\"{}\" io_buf=${:04X}", path, io_buf);
                    }
                }
                val
            }
            0xC000 => self.keyboard.read_key(),
            0xC001..=0xC00F => {
                // IIe: reads of $C000-$C00F return keyboard data (same as $C000)
                self.keyboard.read_key()
            }
            0xC010 => self.keyboard.clear_strobe(),
            0xC011..=0xC01F => self.switches.read_status(addr),
            0xC030 => {
                self.speaker.toggle();
                0
            }
            0xC050..=0xC05F => {
                // Display and annunciator switches trigger on both reads and writes.
                // Exception: $C050 (TXTCLR/GR mode) does not trigger on reads.
                // On real hardware, Bitsy Bye's pointer overflow reads $C050
                // repeatedly; the loop exits when the floating bus returns a byte
                // with bit 7 clear (from text page hole regions during HBL).
                // However, the loop is tight enough that the read may never land
                // on an HBL cycle, causing a permanent TEXT→GR switch.
                // Real software activates GR mode via STA $C050 (writes).
                if addr != 0xC050 {
                    self.switches.handle(addr);
                }
                self.floating_bus()
            }
            0xC080..=0xC08F => {
                self.memory.handle_lc_switch(addr);
                0
            }
            // Disk II slot 6: I/O at $C0E0-$C0EF
            0xC0E0..=0xC0EF => self.disk_ii.io_read(addr),
            // Slot ROM space ($C100-$C7FF)
            0xC100..=0xC2FF | 0xC400..=0xC5FF | 0xC700..=0xC7FF => {
                if self.switches.intcxrom {
                    self.memory.rom[(addr - 0xC000) as usize]
                } else if addr >= 0xC600 && addr <= 0xC6FF {
                    self.disk_ii.read_rom(addr)
                } else {
                    0xFF // No card in this slot
                }
            }
            0xC300..=0xC3FF => {
                // Slot 3: IIe 80-column firmware
                if self.switches.intcxrom || !self.switches.slotc3rom {
                    // Using internal ROM; activate $C800 expansion space
                    if !self.switches.intc8rom {
                        log::info!("80-col firmware: first access ${:04X} PC=${:04X}, activating $C800 expansion ROM",
                            addr, self.debug_pc);
                    }
                    self.switches.intc8rom = true;
                    self.memory.rom[(addr - 0xC000) as usize]
                } else {
                    log::info!("Slot 3 ROM read ${:04X} PC=${:04X} (slot ROM, returning $FF)",
                        addr, self.debug_pc);
                    0xFF
                }
            }
            0xC600..=0xC6FF => {
                if self.switches.intcxrom {
                    self.memory.rom[(addr - 0xC000) as usize]
                } else {
                    self.disk_ii.read_rom(addr)
                }
            }
            // Expansion ROM space ($C800-$CFFF)
            0xC800..=0xCFFE => {
                if self.switches.intcxrom || self.switches.intc8rom {
                    // IIe internal ROM (80-col firmware expansion)
                    self.memory.rom[(addr - 0xC000) as usize]
                } else {
                    0xFF
                }
            }
            0xCFFF => {
                // Accessing $CFFF deactivates expansion ROM
                if self.switches.intc8rom {
                    log::info!("$CFFF read: clearing intc8rom, PC=${:04X}", self.debug_pc);
                }
                self.switches.intc8rom = false;
                if self.switches.intcxrom {
                    self.memory.rom[(addr - 0xC000) as usize]
                } else {
                    0xFF
                }
            }
            0xC000..=0xCFFF => self.floating_bus(), // Unhandled I/O
            0xD000..=0xFFFF => {
                self.memory.read_banked(addr, self.switches.altzp)
            }
        }
    }

    fn write(&mut self, addr: u16, val: u8) {
        // Trace writes to ZP $64-$65 (Bitsy Bye directory pointer)
        if (addr == 0x0064 || addr == 0x0065) && self.frame_count >= 507 && self.frame_count <= 512 {
            log::info!("ZP ${:02X} ← ${:02X} PC=${:04X} X=${:02X} frame={}",
                addr, val, self.debug_pc, self.debug_x, self.frame_count);
        }
        match addr {
            0x0000..=0xBFFF => self.banked_write(addr, val),
            0xC000..=0xC00F => {
                // Trace removed
                self.switches.handle_iie(addr);
            }
            0xC010 => { self.keyboard.clear_strobe(); }
            0xC030 => { self.speaker.toggle(); }
            0xC050..=0xC05F => { self.switches.handle(addr); }
            0xC080..=0xC08F => {
                // LC switches respond to ALL bus accesses (read AND write).
                // Write accesses trigger the switch but reset prewrite
                // (writes don't count toward the double-read write-enable).
                self.memory.handle_lc_switch(addr);
                self.memory.lc_prewrite = false;
            }
            // Disk II slot 6: I/O at $C0E0-$C0EF
            0xC0E0..=0xC0EF => { self.disk_ii.io_write(addr, val); }
            0xCFFF => { self.switches.intc8rom = false; } // Clear expansion ROM
            0xC000..=0xCFFF => {} // Other I/O
            0xD000..=0xFFFF => self.memory.write_banked(addr, val, self.switches.altzp),
        }
    }

    fn peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0xBFFF => self.memory.read(addr),
            0xC100..=0xC2FF | 0xC400..=0xC5FF | 0xC700..=0xCFFF => {
                if self.switches.intcxrom {
                    self.memory.rom[(addr - 0xC000) as usize]
                } else if addr >= 0xC600 && addr <= 0xC6FF {
                    self.disk_ii.read_rom(addr)
                } else {
                    0xFF
                }
            }
            0xC300..=0xC3FF => {
                if self.switches.intcxrom || !self.switches.slotc3rom {
                    self.memory.rom[(addr - 0xC000) as usize]
                } else {
                    0xFF
                }
            }
            0xC600..=0xC6FF => {
                if self.switches.intcxrom {
                    self.memory.rom[(addr - 0xC000) as usize]
                } else {
                    self.disk_ii.read_rom(addr)
                }
            }
            0xD000..=0xFFFF => self.memory.read_banked(addr, self.switches.altzp),
            _ => 0,
        }
    }

    fn tick(&mut self, cycles: u8) {
        for _ in 0..cycles {
            self.speaker.step();
        }

        self.disk_ii.step(cycles);

        self.cycle_count += cycles as u64;

        // VBL: scanlines 192-261 are vertical blank (~4550 cycles of 17030 per frame)
        // Active display: cycles 0-12480, VBL: cycles 12480-17030
        self.switches.vbl = self.cycle_count >= 12480;

        // Sync language card status into soft switches for $C011/$C012 reads
        self.switches.lc_bank2 = !self.memory.lc_bank1;
        self.switches.lc_read_enable = self.memory.lc_read_enable;

        if self.cycle_count >= self.cycles_per_frame {
            self.cycle_count -= self.cycles_per_frame;
            self.render_frame();
            self.frame_ready = true;

            // Save framebuffer at frame 550 (before any GR switch)
            if self.frame_count == 550 {
                if let Err(e) = std::fs::write("/tmp/emu_frame550.raw", &self.framebuffer.pixels) {
                    log::warn!("Failed to save frame 550: {}", e);
                } else {
                    log::info!("Saved frame 550 to /tmp/emu_frame550.raw ({}x{})",
                        self.framebuffer.width, self.framebuffer.height);
                }
            }

            // Diagnostic at key frames
            if self.frame_count == 120 || self.frame_count == 300
                || self.frame_count == 400 || self.frame_count == 500
                || self.frame_count == 550 || self.frame_count == 600 {
                let text_start = 0x0400usize;
                let mut lines = Vec::new();
                for row in 0..24 {
                    let offsets: [u16; 24] = [
                        0x000, 0x080, 0x100, 0x180, 0x200, 0x280, 0x300, 0x380,
                        0x028, 0x0A8, 0x128, 0x1A8, 0x228, 0x2A8, 0x328, 0x3A8,
                        0x050, 0x0D0, 0x150, 0x1D0, 0x250, 0x2D0, 0x350, 0x3D0,
                    ];
                    let addr = text_start + offsets[row] as usize;
                    let line: String = (0..40).map(|i| {
                        let b = self.memory.ram[addr + i];
                        let c = b & 0x7F;
                        if c >= 0x20 && c < 0x7F { c as char } else { '.' }
                    }).collect();
                    if line.trim().len() > 0 {
                        lines.push(format!("  row {}: \"{}\"", row, line));
                    }
                }
                log::info!("=== Frame {} === PC=${:04X} motor={} track={} mode={} col80={} store80={} altchar={} page2={} altzp={} ptr64=${:02X}{:02X}",
                    self.frame_count, self.debug_pc,
                    self.disk_ii.motor_on, self.disk_ii.current_track,
                    if self.switches.text_mode { "TEXT" }
                    else if self.switches.hires { "HIRES" }
                    else { "LORES" },
                    self.switches.col80, self.switches.store80, self.switches.altcharset,
                    self.switches.page2, self.switches.altzp,
                    self.memory.ram[0x65], self.memory.ram[0x64]);
                log::info!("  $0800: {:02X} {:02X} {:02X} {:02X}",
                    self.memory.ram[0x0800], self.memory.ram[0x0801],
                    self.memory.ram[0x0802], self.memory.ram[0x0803]);
                for line in &lines {
                    log::info!("{}", line);
                }

                // At frame 550, dump both the READ target buffer ($1400) and the display pointer
                if self.frame_count == 550 {
                    let ptr = self.memory.ram[0x64] as u16 | ((self.memory.ram[0x65] as u16) << 8);
                    log::info!("  === Display ptr ZP $64-$65 = ${:04X} ===", ptr);
                    // Dump $1400-$14FF (where MLI READ stores data)
                    log::info!("  === MLI READ buffer at $1400 ===");
                    for row in 0..16 {
                        let base = 0x1400 + row * 16;
                        let chunk: Vec<u8> = (0..16).map(|i| self.memory.ram[base + i]).collect();
                        let text: String = chunk.iter().map(|&b| {
                            let c = b & 0x7F;
                            if c >= 0x20 && c < 0x7F { c as char } else { '.' }
                        }).collect();
                        log::info!("  ${:04X}: {:02X?} |{}|", base, chunk, text);
                    }
                    // Dump ptr64 area
                    if (ptr as usize) < self.memory.ram.len().saturating_sub(64) {
                        log::info!("  === Memory at ptr64 ${:04X} ===", ptr);
                        for row in 0..4 {
                            let base = ptr as usize + row * 16;
                            let chunk: Vec<u8> = (0..16).map(|i| self.memory.ram[base + i]).collect();
                            log::info!("  ${:04X}: {:02X?}", base, chunk);
                        }
                    }
                    // Dump the OPEN path hex at the path address from param block $007C
                    let path_lo = self.memory.ram[0x7D] as u16;
                    let path_hi = self.memory.ram[0x7E] as u16;
                    let path_addr = (path_hi << 8) | path_lo;
                    let path_bytes: Vec<u8> = (0..20).map(|i| self.memory.ram[(path_addr + i) as usize]).collect();
                    log::info!("  Path at ${:04X}: {:02X?}", path_addr, path_bytes);
                    // Dump ROM VTAB chain: $FBB0-$FC40
                    for base in (0xFBB0..=0xFC30u16).step_by(16) {
                        let chunk: Vec<u8> = (0..16).map(|i| {
                            self.memory.rom[(base - 0xC000 + i) as usize]
                        }).collect();
                        log::info!("  ROM ${:04X}: {:02X?}", base, chunk);
                    }
                    // Also check CSW vector at ZP $36-$37
                    log::info!("  CSW=$36-$37: {:02X} {:02X}, KSW=$38-$39: {:02X} {:02X}",
                        self.memory.ram[0x36], self.memory.ram[0x37],
                        self.memory.ram[0x38], self.memory.ram[0x39]);
                }

                // At frame 600, dump code, ZP, and ProDOS directory data
                if self.frame_count == 600 {
                    // Dump code at $1100-$1120
                    let code: Vec<u8> = (0..32).map(|i| self.memory.ram[0x1100 + i]).collect();
                    log::info!("  $1100-$111F: {:02X?}", code);
                    let code2: Vec<u8> = (0..32).map(|i| self.memory.ram[0x10C0 + i]).collect();
                    log::info!("  $10C0-$10DF: {:02X?}", code2);
                    // Dump ZP pointers
                    let zp: Vec<u8> = (0..256).map(|i| self.memory.ram[i]).collect();
                    log::info!("  ZP $00-$0F: {:02X?}", &zp[0x00..0x10]);
                    log::info!("  ZP $10-$1F: {:02X?}", &zp[0x10..0x20]);
                    log::info!("  ZP $20-$2F: {:02X?}", &zp[0x20..0x30]);
                    log::info!("  ZP $60-$6F: {:02X?}", &zp[0x60..0x70]);
                    // Dump ProDOS data buffer at $2000 (common location)
                    let buf: Vec<u8> = (0..32).map(|i| self.memory.ram[0x2000 + i]).collect();
                    log::info!("  $2000-$201F: {:02X?}", buf);
                    // Check what's at the directory buffer pointed to by ProDOS
                    // ProDOS MLI parameter area is often around $40-$4F
                    log::info!("  ZP $40-$4F: {:02X?}", &zp[0x40..0x50]);
                    log::info!("  ZP $50-$5F: {:02X?}", &zp[0x50..0x60]);

                    let offsets: [u16; 24] = [
                        0x000, 0x080, 0x100, 0x180, 0x200, 0x280, 0x300, 0x380,
                        0x028, 0x0A8, 0x128, 0x1A8, 0x228, 0x2A8, 0x328, 0x3A8,
                        0x050, 0x0D0, 0x150, 0x1D0, 0x250, 0x2D0, 0x350, 0x3D0,
                    ];
                    for row in 0..8 {
                        let addr = 0x0400 + offsets[row] as usize;
                        let hex: Vec<u8> = (0..40).map(|i| self.memory.ram[addr + i]).collect();
                        log::info!("  hex row {}: {:02X?}", row, hex);
                    }
                    let offsets: [u16; 24] = [
                        0x000, 0x080, 0x100, 0x180, 0x200, 0x280, 0x300, 0x380,
                        0x028, 0x0A8, 0x128, 0x1A8, 0x228, 0x2A8, 0x328, 0x3A8,
                        0x050, 0x0D0, 0x150, 0x1D0, 0x250, 0x2D0, 0x350, 0x3D0,
                    ];
                    log::info!("=== Aux RAM text page 1 (first 4 rows) ===");
                    for row in 0..4 {
                        let addr = 0x0400 + offsets[row] as usize;
                        let line: String = (0..40).map(|i| {
                            let b = self.memory.aux_ram[addr + i];
                            let c = b & 0x7F;
                            if c >= 0x20 && c < 0x7F { c as char } else { '.' }
                        }).collect();
                        log::info!("  aux row {}: \"{}\"", row, line);
                    }
                    // Save framebuffer as raw RGBA
                    if let Err(e) = std::fs::write("/tmp/emu_frame600.raw", &self.framebuffer.pixels) {
                        log::warn!("Failed to save framebuffer: {}", e);
                    } else {
                        log::info!("Saved framebuffer to /tmp/emu_frame600.raw ({}x{} RGBA)",
                            self.framebuffer.width, self.framebuffer.height);
                    }
                }
            }
        }
    }

    fn poll_nmi(&mut self) -> bool {
        false // Apple II has no NMI
    }

    fn poll_irq(&mut self) -> bool {
        false // No IRQ sources in basic Apple II
    }
}
