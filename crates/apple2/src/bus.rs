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

    pub(crate) cycle_count: u64,
    cycles_per_frame: u64,
    frame_ready: bool,
    pub(crate) frame_count: u32,
    /// Debug: last known PC (set by lib.rs before each step, used for soft switch address tracking).
    pub(crate) debug_pc: u16,
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

        if use_aux {
            self.memory.aux_ram[addr as usize]
        } else {
            self.memory.ram[addr as usize]
        }
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
            0x0000..=0xBFFF => self.banked_read(addr),
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
                    self.switches.intc8rom = true;
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
        }
    }

    fn poll_nmi(&mut self) -> bool {
        false // Apple II has no NMI
    }

    fn poll_irq(&mut self) -> bool {
        false // No IRQ sources in basic Apple II
    }
}
