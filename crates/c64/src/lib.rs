pub mod bus;
pub mod cia;
mod snapshot;
pub mod d64_image;
pub mod drive1541;
pub mod iec_bus;
pub mod kernal_traps;
pub mod memory;
pub mod rom_loader;
pub mod sid;
pub mod t64_loader;
pub mod via;
pub mod vic_ii;

use emu_common::{AudioSample, Bus, Button, CpuDebugState, DebugSection, FrameBuffer, InputEvent, SystemEmulator};
use emu_cpu::Cpu6502;
use bus::C64Bus;
use drive1541::bus::Drive1541Bus;
use iec_bus::IecBus;

/// A key release scheduled for a future frame.
struct PendingRelease {
    row: u8,
    col: u8,
    frames_left: u8,
}

/// Commodore 64 system emulator.
pub struct C64 {
    cpu: Cpu6502<C64Bus>,
    /// Optional 1541 drive CPU (present when 1541 ROM is available and D64 loaded).
    drive_cpu: Option<Cpu6502<Drive1541Bus>>,
    /// Shared IEC bus state (used when drive_cpu is present).
    iec_bus: IecBus,
    /// PRG data to inject after KERNAL boot completes.
    pending_prg: Option<Vec<u8>>,
    /// Frames to wait before injecting PRG (lets KERNAL boot finish).
    boot_frames: u32,
    /// Whether to auto-type RUN after PRG injection (for D64 auto-load).
    auto_run: bool,
    /// Keys to release after a delay (for shifted chars from Text events).
    pending_releases: Vec<PendingRelease>,
    /// Virtual disk drive for D64 images (KERNAL trap fallback).
    kernal_drive: kernal_traps::KernalDrive,
    /// IEC trace countdown (frames remaining). 0 = tracing off.
    iec_trace_frames: u32,
    /// Previous IEC bus state for edge detection during tracing.
    trace_last_atn: bool,
    trace_last_drive_data: bool,
    trace_last_drive_clk: bool,
    trace_last_drive_pc: u16,
    trace_last_c64_clk: bool,
    trace_last_c64_data: bool,
    trace_last_c64_pc: u16,
}

impl C64 {
    /// Create a C64 from PRG data.
    pub fn from_rom(rom_data: &[u8]) -> Result<Self, String> {
        if rom_data.is_empty() {
            return Err("ROM data is empty".into());
        }

        let bus = C64Bus::new();

        if rom_data.len() < 3 {
            return Err("PRG file too small (need at least 3 bytes)".into());
        }

        let mut cpu = Cpu6502::new(bus);
        cpu.bcd_enabled = true;
        cpu.reset();

        Ok(Self {
            cpu,
            drive_cpu: None,
            iec_bus: IecBus::new(),
            pending_prg: Some(rom_data.to_vec()),
            boot_frames: 0,
            auto_run: false,
            pending_releases: Vec::new(),
            kernal_drive: kernal_traps::KernalDrive::new(None),
            iec_trace_frames: 0,
            trace_last_atn: false,
            trace_last_drive_data: false,
            trace_last_drive_clk: false,
            trace_last_drive_pc: 0,
            trace_last_c64_clk: false,
            trace_last_c64_data: false,
            trace_last_c64_pc: 0,
        })
    }

    /// Create a C64 with system ROMs loaded (no PRG).
    pub fn with_roms(basic: &[u8], kernal: &[u8], char_rom: &[u8]) -> Self {
        let mut bus = C64Bus::new();
        bus.memory.load_roms(basic, kernal, char_rom);

        let mut cpu = Cpu6502::new(bus);
        cpu.bcd_enabled = true;
        cpu.reset();

        Self {
            cpu,
            drive_cpu: None,
            iec_bus: IecBus::new(),
            pending_prg: None,
            boot_frames: 0,
            auto_run: false,
            pending_releases: Vec::new(),
            kernal_drive: kernal_traps::KernalDrive::new(None),
            iec_trace_frames: 0,
            trace_last_atn: false,
            trace_last_drive_data: false,
            trace_last_drive_clk: false,
            trace_last_drive_pc: 0,
            trace_last_c64_clk: false,
            trace_last_c64_data: false,
            trace_last_c64_pc: 0,
        }
    }

    /// Create a C64 with system ROMs and a D64 disk image mounted.
    /// If a 1541 ROM is provided, uses full drive emulation.
    /// Otherwise, falls back to KERNAL traps.
    pub fn from_d64(
        basic: &[u8],
        kernal: &[u8],
        char_rom: &[u8],
        d64_data: &[u8],
    ) -> Result<Self, String> {
        Self::from_d64_with_drive_rom(basic, kernal, char_rom, d64_data, None)
    }

    /// Create a C64 with system ROMs, D64 image, and optional 1541 ROM.
    pub fn from_d64_with_drive_rom(
        basic: &[u8],
        kernal: &[u8],
        char_rom: &[u8],
        d64_data: &[u8],
        drive_rom: Option<&[u8]>,
    ) -> Result<Self, String> {
        let d64 = d64_image::D64Image::parse(d64_data)?;

        let mut bus = C64Bus::new();
        bus.memory.load_roms(basic, kernal, char_rom);

        let mut cpu = Cpu6502::new(bus);
        cpu.bcd_enabled = true;
        cpu.reset();

        // Try to set up full 1541 drive emulation
        let (drive_cpu, kernal_drive, pending_prg, auto_run) = if let Some(rom) = drive_rom {
            if rom.len() >= 16384 {
                // Full 1541 emulation
                let mut drive_bus = Drive1541Bus::new(rom.to_vec());
                drive_bus.disk.load_d64(d64_data);

                let mut dcpu = Cpu6502::new(drive_bus);
                dcpu.bcd_enabled = true;
                dcpu.reset();

                log::info!("1541 drive emulation active (16KB ROM loaded)");
                (Some(dcpu), kernal_traps::KernalDrive::new(None), None, false)
            } else {
                log::warn!("1541 ROM too small ({} bytes), falling back to KERNAL traps", rom.len());
                let prg_data = d64.load_first_prg()?;
                (None, kernal_traps::KernalDrive::new(Some(d64)), Some(prg_data), true)
            }
        } else {
            // No 1541 ROM — use KERNAL trap fallback
            let prg_data = d64.load_first_prg()?;
            (None, kernal_traps::KernalDrive::new(Some(d64)), Some(prg_data), true)
        };

        Ok(Self {
            cpu,
            drive_cpu,
            iec_bus: IecBus::new(),
            pending_prg,
            boot_frames: 0,
            auto_run,
            pending_releases: Vec::new(),
            kernal_drive,
            iec_trace_frames: 0,
            trace_last_atn: false,
            trace_last_drive_data: false,
            trace_last_drive_clk: false,
            trace_last_drive_pc: 0,
            trace_last_c64_clk: false,
            trace_last_c64_data: false,
            trace_last_c64_pc: 0,
        })
    }

    /// Enable IEC trace logging for the next ~72 seconds (3600 frames at 50fps).
    pub fn enable_iec_trace(&mut self) {
        self.iec_trace_frames = 3600;
        log::info!("[IEC] Trace enabled for 3600 frames");
    }

    /// Load system ROMs. Resets the CPU to boot with the new ROMs.
    pub fn load_system_roms(&mut self, basic: &[u8], kernal: &[u8], char_rom: &[u8]) {
        self.cpu.bus.memory.load_roms(basic, kernal, char_rom);
        self.cpu.reset();
        self.boot_frames = 0;
    }

    /// Inject a pending PRG into RAM and set up BASIC pointers.
    fn inject_pending_prg(&mut self) {
        if let Some(prg_data) = self.pending_prg.take() {
            match rom_loader::load_prg(&prg_data, &mut self.cpu.bus.memory.ram) {
                Ok(load_addr) => {
                    log::info!("Injected PRG at ${:04X} after boot", load_addr);
                    if self.auto_run {
                        self.cpu.bus.memory.ram[0x0277] = b'R';
                        self.cpu.bus.memory.ram[0x0278] = b'U';
                        self.cpu.bus.memory.ram[0x0279] = b'N';
                        self.cpu.bus.memory.ram[0x027A] = 0x0D;
                        self.cpu.bus.memory.ram[0x00C6] = 4;
                    }
                }
                Err(e) => {
                    log::error!("Failed to inject PRG: {}", e);
                }
            }
        }
    }

    /// Sync IEC bus between C64 and 1541 drive.
    fn sync_iec(&mut self) {
        let tracing = self.iec_trace_frames > 0;

        // C64 → IEC bus: push CIA2 PA output to IEC bus lines
        let cia2_pa = self.cpu.bus.cia2.pra & self.cpu.bus.cia2.ddra;
        self.iec_bus.update_from_cia2(cia2_pa);

        if tracing {
            let atn_now = self.iec_bus.c64_atn;
            let c64_clk_now = self.iec_bus.c64_clk;
            let c64_data_now = self.iec_bus.c64_data;
            let c64_pc = self.cpu.pc;

            if atn_now != self.trace_last_atn {
                self.trace_last_atn = atn_now;
                log::info!(
                    "[IEC] C64 ATN={} C64_PC={:04X} | bus clk={} data={} | CIA2 PA={:02X} DDRA={:02X}",
                    atn_now as u8, c64_pc,
                    self.iec_bus.clk() as u8, self.iec_bus.data() as u8,
                    self.cpu.bus.cia2.pra, self.cpu.bus.cia2.ddra,
                );
            }
            if c64_clk_now != self.trace_last_c64_clk {
                self.trace_last_c64_clk = c64_clk_now;
                log::info!(
                    "[IEC] C64 CLK={} C64_PC={:04X} bus_clk={} bus_data={}",
                    c64_clk_now as u8, c64_pc,
                    self.iec_bus.clk() as u8, self.iec_bus.data() as u8,
                );
            }
            if c64_data_now != self.trace_last_c64_data {
                self.trace_last_c64_data = c64_data_now;
                log::info!(
                    "[IEC] C64 DATA={} C64_PC={:04X} bus_clk={} bus_data={}",
                    c64_data_now as u8, c64_pc,
                    self.iec_bus.clk() as u8, self.iec_bus.data() as u8,
                );
            }
            // Log C64 PC jumps (rough call tracking) when any IEC line is active
            let iec_active = atn_now || c64_clk_now || c64_data_now
                || self.iec_bus.drive_clk || self.iec_bus.drive_data;
            if iec_active && c64_pc != self.trace_last_c64_pc {
                self.trace_last_c64_pc = c64_pc;
            }
        }

        // IEC bus → 1541 drive
        if let Some(ref mut dcpu) = self.drive_cpu {
            dcpu.bus.sync_iec_input(&self.iec_bus);

            // 1541 → IEC bus
            let drive_data_before = self.iec_bus.drive_data;
            let drive_clk_before = self.iec_bus.drive_clk;
            dcpu.bus.sync_iec_output(&mut self.iec_bus);

            if tracing {
                let drv_pc = dcpu.pc;
                if self.iec_bus.drive_data != drive_data_before || drive_data_before != self.trace_last_drive_data {
                    self.trace_last_drive_data = self.iec_bus.drive_data;
                    log::info!(
                        "[IEC] DRV DATA={} DRV_PC={:04X} C64_PC={:04X} bus_clk={} bus_data={}",
                        self.iec_bus.drive_data as u8, drv_pc, self.cpu.pc,
                        self.iec_bus.clk() as u8, self.iec_bus.data() as u8,
                    );
                }
                if self.iec_bus.drive_clk != drive_clk_before || drive_clk_before != self.trace_last_drive_clk {
                    self.trace_last_drive_clk = self.iec_bus.drive_clk;
                    log::info!(
                        "[IEC] DRV CLK={} DRV_PC={:04X} C64_PC={:04X} bus_clk={} bus_data={}",
                        self.iec_bus.drive_clk as u8, drv_pc, self.cpu.pc,
                        self.iec_bus.clk() as u8, self.iec_bus.data() as u8,
                    );
                }
                if drv_pc != self.trace_last_drive_pc {
                    self.trace_last_drive_pc = drv_pc;
                }
            }
        }

        // IEC bus → C64: update CIA2 PA input bits (bit 6=CLK, bit 7=DATA)
        self.cpu.bus.iec_input = self.iec_bus.cia2_input_bits();
    }
}

/// Map ASCII key to C64 keyboard matrix position (row, col).
fn ascii_to_matrix(key: u8) -> Option<(u8, u8)> {
    match key {
        b'A' => Some((1, 2)),
        b'B' => Some((3, 4)),
        b'C' => Some((2, 4)),
        b'D' => Some((2, 2)),
        b'E' => Some((1, 6)),
        b'F' => Some((2, 5)),
        b'G' => Some((3, 2)),
        b'H' => Some((3, 5)),
        b'I' => Some((4, 1)),
        b'J' => Some((4, 2)),
        b'K' => Some((4, 5)),
        b'L' => Some((5, 2)),
        b'M' => Some((4, 4)),
        b'N' => Some((4, 7)),
        b'O' => Some((4, 6)),
        b'P' => Some((5, 1)),
        b'Q' => Some((7, 6)),
        b'R' => Some((2, 1)),
        b'S' => Some((1, 5)),
        b'T' => Some((2, 6)),
        b'U' => Some((3, 6)),
        b'V' => Some((3, 7)),
        b'W' => Some((1, 1)),
        b'X' => Some((2, 7)),
        b'Y' => Some((3, 1)),
        b'Z' => Some((1, 4)),
        b'0' => Some((4, 3)),
        b'1' => Some((7, 0)),
        b'2' => Some((7, 3)),
        b'3' => Some((1, 0)),
        b'4' => Some((1, 3)),
        b'5' => Some((2, 0)),
        b'6' => Some((2, 3)),
        b'7' => Some((3, 0)),
        b'8' => Some((3, 3)),
        b'9' => Some((4, 0)),
        b' ' => Some((7, 4)),
        b'+' => Some((5, 0)),
        b'-' => Some((5, 3)),
        b'*' => Some((6, 1)),
        b'/' => Some((6, 7)),
        b'=' => Some((6, 5)),
        b':' => Some((5, 5)),
        b';' => Some((6, 2)),
        b'@' => Some((5, 6)),
        b',' => Some((5, 7)),
        b'.' => Some((5, 4)),
        0x0D => Some((0, 1)), // Return
        0x08 => Some((0, 0)), // Backspace → DEL
        0x14 => Some((0, 0)), // DEL (PETSCII)
        0x03 => Some((7, 7)), // Ctrl+C → RUN/STOP
        // Function keys (PETSCII codes, unshifted)
        0x85 => Some((0, 4)), // F1
        0x86 => Some((0, 6)), // F3
        0x87 => Some((0, 5)), // F5
        0x88 => Some((0, 3)), // F7
        _ => None,
    }
}

/// Map shifted ASCII characters to their base key + left shift.
fn shifted_ascii_to_matrix(key: u8) -> Option<(u8, u8)> {
    match key {
        b'"' => Some((7, 3)),
        b'!' => Some((7, 0)),
        b'#' => Some((1, 0)),
        b'$' => Some((1, 3)),
        b'%' => Some((2, 0)),
        b'&' => Some((2, 3)),
        b'\'' => Some((3, 0)),
        b'(' => Some((3, 3)),
        b')' => Some((4, 0)),
        b'?' => Some((6, 7)),
        b'<' => Some((5, 7)),
        b'>' => Some((5, 4)),
        b'[' => Some((5, 5)),
        b']' => Some((6, 2)),
        // Shifted function keys (PETSCII codes) — shift auto-pressed by caller
        0x89 => Some((0, 4)), // F2 = shift+F1
        0x8A => Some((0, 6)), // F4 = shift+F3
        0x8B => Some((0, 5)), // F6 = shift+F5
        0x8C => Some((0, 3)), // F8 = shift+F7
        _ => None,
    }
}

const LEFT_SHIFT: (u8, u8) = (1, 7);

impl SystemEmulator for C64 {
    fn supports_save_states(&self) -> bool { true }

    fn save_state(&self) -> Result<Vec<u8>, String> {
        let bus = &self.cpu.bus;
        let snap = crate::snapshot::C64Snapshot {
            cpu:          self.cpu.snapshot(),
            ram:          bus.memory.ram.to_vec(),
            cpu_port:     bus.memory.cpu_port,
            cpu_port_dir: bus.memory.cpu_port_dir,
            vic:          bus.vic.snapshot(),
            sid:          bus.sid.snapshot(),
            cia1:         bus.cia1.snapshot(),
            cia2:         bus.cia2.snapshot(),
        };
        let bytes = bincode::serde::encode_to_vec(&snap, bincode::config::standard())
            .map_err(|e| e.to_string())?;
        Ok(emu_common::save_encode("C64", &bytes))
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        let payload = emu_common::save_decode("C64", data)?;
        let (snap, _): (crate::snapshot::C64Snapshot, _) =
            bincode::serde::decode_from_slice(payload, bincode::config::standard())
                .map_err(|e| e.to_string())?;
        self.cpu.restore(&snap.cpu);
        if snap.ram.len() != 65536 {
            return Err(format!("C64 save state: expected 65536 bytes for RAM, got {}", snap.ram.len()));
        }
        self.cpu.bus.memory.ram.copy_from_slice(&snap.ram);
        self.cpu.bus.memory.cpu_port = snap.cpu_port;
        self.cpu.bus.memory.cpu_port_dir = snap.cpu_port_dir;
        self.cpu.bus.vic.restore(&snap.vic);
        self.cpu.bus.sid.restore(&snap.sid);
        self.cpu.bus.cia1.restore(&snap.cia1);
        self.cpu.bus.cia2.restore(&snap.cia2);
        self.pending_prg = None;
        self.boot_frames = 0;
        self.pending_releases.clear();
        Ok(())
    }

    fn step_frame(&mut self) -> usize {
        // Count down IEC trace timer
        if self.iec_trace_frames > 0 {
            self.iec_trace_frames -= 1;
        }

        // Inject pending PRG after KERNAL boot completes (~120 frames at 50Hz)
        if self.pending_prg.is_some() {
            self.boot_frames += 1;
            if self.boot_frames >= 120 {
                self.inject_pending_prg();
            }
        }

        // Process pending key releases
        self.pending_releases.retain_mut(|pr| {
            pr.frames_left -= 1;
            if pr.frames_left == 0 {
                self.cpu.bus.cia1.key_up(pr.row, pr.col);
                false
            } else {
                true
            }
        });

        // Heartbeat counter: log CPU state every ~50K steps when tracing
        let mut heartbeat = 0u32;
        const HEARTBEAT_INTERVAL: u32 = 50_000;

        loop {
            // Check KERNAL traps (only active when no 1541 drive CPU)
            if self.drive_cpu.is_none() && !self.kernal_drive.check_trap(&mut self.cpu) {
                self.cpu.step();
            } else if self.drive_cpu.is_some() {
                self.cpu.step();
                // Sync IEC after C64 step so drive sees current C64 output
                self.sync_iec();
            }

            // Run drive CPU in lockstep (both at ~1 MHz)
            if let Some(ref mut dcpu) = self.drive_cpu {
                dcpu.step();
            }
            // Sync IEC after drive step so C64 sees current drive output
            if self.drive_cpu.is_some() {
                self.sync_iec();
            }
            // Log key drive events — check PC first to avoid per-instruction overhead
            if let Some(ref mut dcpu) = self.drive_cpu {
                if self.iec_trace_frames > 0 {
                    let pc = dcpu.pc;
                    match pc {
                        0xE931 => {
                            let ch = dcpu.bus.ram[0x82] as usize;
                            let f2x = dcpu.bus.ram[0xF2 + ch.min(13)];
                            if f2x & 0x08 == 0 {
                                log::info!("[EOI] E931 USE_EOI ch={} F2,X={:02X}", ch, f2x);
                            }
                        }
                        0xD3FA => {
                            let ch = dcpu.bus.ram[0x82] as usize;
                            log::info!("[EOI] D3FA: REL last-byte flag SET ch={}", ch);
                        }
                        0xD162 => {
                            let ch = dcpu.bus.ram[0x82] as usize;
                            let f2x = dcpu.bus.ram[0xF2 + ch.min(13)];
                            log::info!("[EOI] D162: PRG EOI flag SET ch={} F2={:02X}", ch, f2x);
                        }
                        0xD16A => {
                            let ch = dcpu.bus.ram[0x82] as usize;
                            let trk = dcpu.bus.ram[0x18];
                            let sec = dcpu.bus.ram[0x19];
                            let zp80 = dcpu.bus.ram[0x80];
                            let zp81 = dcpu.bus.ram[0x81];
                            let b0_0 = dcpu.bus.ram[0x300];
                            let b0_1 = dcpu.bus.ram[0x301];
                            let b1_0 = dcpu.bus.ram[0x400];
                            let b1_1 = dcpu.bus.ram[0x401];
                            let a7x = dcpu.bus.ram[0xA7 + ch.min(13)];
                            let aex = dcpu.bus.ram[0xAE + ch.min(13)];
                            let bst: [u8; 4] = core::array::from_fn(|i| dcpu.bus.ram[i]);
                            let zp9a = dcpu.bus.ram[0x9A];
                            let zp99 = dcpu.bus.ram[0x99];
                            let zp9c = dcpu.bus.ram[0x9C];
                            let zp9b = dcpu.bus.ram[0x9B];
                            log::info!("[D16A] entry disk=({},{}) $80/$81=({},{}) B0[0,1]=({},{}) B1[0,1]=({},{}) A7={:02X} AE={:02X} bst=[{:02X},{:02X},{:02X},{:02X}] ptr=$9A/$99={:02X}/{:02X} $9C/$9B={:02X}/{:02X}",
                                trk, sec, zp80, zp81, b0_0, b0_1, b1_0, b1_1,
                                a7x, aex, bst[0], bst[1], bst[2], bst[3],
                                zp9a, zp99, zp9c, zp9b);
                        }
                        0xD180 => {
                            let ch = dcpu.bus.ram[0x82] as usize;
                            let zp80 = dcpu.bus.ram[0x80];
                            let zp81 = dcpu.bus.ram[0x81];
                            let a7x = dcpu.bus.ram[0xA7 + ch.min(13)];
                            let aex = dcpu.bus.ram[0xAE + ch.min(13)];
                            let bst: [u8; 4] = core::array::from_fn(|i| dcpu.bus.ram[i]);
                            log::info!("[D16A] D180 $80/$81=({},{}) A7={:02X} AE={:02X} bst=[{:02X},{:02X},{:02X},{:02X}]",
                                zp80, zp81, a7x, aex, bst[0], bst[1], bst[2], bst[3]);
                        }
                        0xD18C => {
                            let ch = dcpu.bus.ram[0x82] as usize;
                            let zp80 = dcpu.bus.ram[0x80];
                            let zp81 = dcpu.bus.ram[0x81];
                            let a7x = dcpu.bus.ram[0xA7 + ch.min(13)];
                            let aex = dcpu.bus.ram[0xAE + ch.min(13)];
                            let bst: [u8; 4] = core::array::from_fn(|i| dcpu.bus.ram[i]);
                            log::info!("[D16A] D18C $80/$81=({},{}) A7={:02X} AE={:02X} bst=[{:02X},{:02X},{:02X},{:02X}]",
                                zp80, zp81, a7x, aex, bst[0], bst[1], bst[2], bst[3]);
                        }
                        0xDCB3 => {
                            let ch = dcpu.bus.ram[0x82] as usize;
                            log::info!("[EOI] DCB3: TALK sets F2,X=$88 ch={}", ch);
                        }
                        0xF533 => {
                            let trk = dcpu.bus.ram[0x18];
                            let sec = dcpu.bus.ram[0x19];
                            let half_trk = dcpu.bus.disk.half_track;
                            log::info!("[SCAN] F533 seeking ({},{}) half_trk={}", trk, sec, half_trk);
                        }
                        0xF54E => {
                            // Header comparison mismatch — wrong sector found, retry
                            let trk = dcpu.bus.ram[0x18];
                            let sec = dcpu.bus.ram[0x19];
                            let x_reg = dcpu.x;
                            let y_reg = dcpu.y;
                            let a_reg = dcpu.a;
                            let hdr_exp: [u8; 8] = core::array::from_fn(|i| dcpu.bus.ram[0x24 + i]);
                            let via2_ira = dcpu.bus.via2.ira;
                            let exp_byte = hdr_exp.get(y_reg as usize).copied().unwrap_or(0xFF);
                            log::info!("[SCAN] F54E mismatch retry={} target=({},{}) Y={} actual=0x{:02X} expected=0x{:02X} via2ira=0x{:02X} hdr={:02X?}",
                                x_reg, trk, sec, y_reg, a_reg, exp_byte, via2_ira, hdr_exp);
                        }
                        0xF553 => {
                            // T1 timeout: sync not found in window, or all retries exhausted
                            let trk = dcpu.bus.ram[0x18];
                            let sec = dcpu.bus.ram[0x19];
                            let half_trk = dcpu.bus.disk.half_track;
                            log::info!("[SCAN] F553 timeout target=({},{}) half_trk={}", trk, sec, half_trk);
                        }
                        _ => {}
                    }
                }
            }

            // Periodic heartbeat during IEC tracing
            if self.iec_trace_frames > 0 {
                heartbeat += 1;
                if heartbeat % HEARTBEAT_INTERVAL == 0 {
                    let drv_info = if let Some(ref dcpu) = self.drive_cpu {
                        format!("DRV_PC={:04X} DRV_CLK={} DRV_DATA={}",
                            dcpu.pc, self.iec_bus.drive_clk as u8, self.iec_bus.drive_data as u8)
                    } else {
                        "no drive".into()
                    };
                    log::info!(
                        "[IEC] heartbeat C64_PC={:04X} C64_CLK={} C64_DATA={} bus_clk={} bus_data={} | {}",
                        self.cpu.pc,
                        self.iec_bus.c64_clk as u8, self.iec_bus.c64_data as u8,
                        self.iec_bus.clk() as u8, self.iec_bus.data() as u8,
                        drv_info,
                    );
                }
            }

            if self.cpu.bus.vic.is_frame_ready() {
                break;
            }
        }
        0
    }

    fn framebuffer(&self) -> &FrameBuffer {
        &self.cpu.bus.vic.framebuffer
    }

    fn audio_samples(&mut self, out: &mut [AudioSample]) -> usize {
        self.cpu.bus.sid.drain_samples(out)
    }

    fn handle_input(&mut self, event: InputEvent) {
        // Joystick port 2: Up=0, Down=1, Left=2, Right=3, Fire=4
        let joy_bit = match event.button {
            Button::Up    => Some(0u8),
            Button::Down  => Some(1),
            Button::Left  => Some(2),
            Button::Right => Some(3),
            Button::Fire | Button::A | Button::B => Some(4),
            _ => None,
        };
        if let Some(bit) = joy_bit {
            if event.pressed {
                self.cpu.bus.cia1.joy2_down(bit);
            } else {
                self.cpu.bus.cia1.joy2_up(bit);
            }
            return;
        }

        if let Button::Key(ascii) = event.button {
            if let Some((row, col)) = ascii_to_matrix(ascii) {
                if event.pressed {
                    self.cpu.bus.cia1.key_down(row, col);
                    self.pending_releases.retain(|pr| pr.row != row || pr.col != col);
                    self.pending_releases.push(PendingRelease {
                        row, col, frames_left: 3,
                    });
                } else {
                    self.pending_releases.retain(|pr| pr.row != row || pr.col != col);
                    self.cpu.bus.cia1.key_up(row, col);
                }
            } else if let Some((row, col)) = shifted_ascii_to_matrix(ascii) {
                if event.pressed {
                    self.cpu.bus.cia1.key_down(LEFT_SHIFT.0, LEFT_SHIFT.1);
                    self.cpu.bus.cia1.key_down(row, col);
                    self.pending_releases.retain(|pr| !(pr.row == row && pr.col == col)
                        && !(pr.row == LEFT_SHIFT.0 && pr.col == LEFT_SHIFT.1));
                    self.pending_releases.push(PendingRelease {
                        row, col, frames_left: 3,
                    });
                    self.pending_releases.push(PendingRelease {
                        row: LEFT_SHIFT.0, col: LEFT_SHIFT.1, frames_left: 3,
                    });
                }
            }
        }
    }

    fn reset(&mut self) {
        self.cpu.reset();
        if let Some(ref mut dcpu) = self.drive_cpu {
            dcpu.reset();
        }
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.cpu.bus.sid.set_sample_rate(rate);
    }

    fn display_width(&self) -> u32 { vic_ii::SCREEN_WIDTH }
    fn display_height(&self) -> u32 { vic_ii::SCREEN_HEIGHT }
    fn target_fps(&self) -> f64 { 50.0 } // PAL
    fn system_name(&self) -> &str { "Commodore 64" }

    fn cpu_state(&self) -> CpuDebugState {
        CpuDebugState {
            pc:     self.cpu.pc,
            sp:     self.cpu.sp,
            a:      self.cpu.a,
            x:      self.cpu.x,
            y:      self.cpu.y,
            flags:  self.cpu.p.bits(),
            cycles: self.cpu.total_cycles,
        }
    }

    fn peek_memory(&self, addr: u16) -> u8 {
        self.cpu.bus.peek(addr)
    }

    fn disassemble(&self, addr: u16) -> (String, u16) {
        emu_cpu::disassemble_6502(|a| self.cpu.bus.peek(a), addr)
    }

    fn step_instruction(&mut self) {
        self.cpu.step();
    }

    fn system_debug_panels(&self) -> Vec<DebugSection> {
        let vic = &self.cpu.bus.vic;
        let regs = &vic.registers;

        // VIC-II
        let mode_str = match (regs[0x11] >> 5) & 0x3 {
            0 => if regs[0x16] & 0x10 != 0 { "MCM Text" } else { "Text" },
            1 => "MCM Bitmap",
            2 => if regs[0x16] & 0x10 != 0 { "MCM Bitmap" } else { "Bitmap" },
            3 => "Extended Color",
            _ => "?",
        };
        let raster = ((regs[0x11] as u16 & 0x80) << 1) | regs[0x12] as u16;
        let spr_en = regs[0x15];
        let vic_sec = DebugSection::new("VIC-II")
            .row("Mode",        mode_str)
            .row("Raster",      format!("{}", raster))
            .row("Scroll X/Y",  format!("{} / {}", regs[0x16] & 7, regs[0x11] & 7))
            .row("Sprites",     format!("{:08b}", spr_en))
            .row("Border",      format!("${:02X}", regs[0x20]))
            .row("Background",  format!("${:02X}", regs[0x21]));

        // SID
        let voices = self.cpu.bus.sid.voice_debug();
        let mut sid_sec = DebugSection::new("SID");
        for (i, (freq, pw, wave, env, state)) in voices.iter().enumerate() {
            sid_sec = sid_sec.row(
                format!("V{}", i + 1),
                format!("{:.0}Hz  {}  pw:{:03X}  env:{:3}/255 ({})", freq, wave, pw, env, state),
            );
        }

        // CIA1
        let c1 = &self.cpu.bus.cia1;
        let cia1_sec = DebugSection::new("CIA1 (IRQ/Kbd)")
            .row("Timer A", format!("${:04X}/{:04X} {}", c1.timer_a_counter(), c1.timer_a_latch(),
                                    if c1.timer_a_running() { "run" } else { "stop" }))
            .row("Timer B", format!("${:04X}/{:04X} {}", c1.timer_b_counter(), c1.timer_b_latch(),
                                    if c1.timer_b_running() { "run" } else { "stop" }))
            .row("ICR",     format!("data:${:02X} mask:${:02X}", c1.icr_data, c1.icr_mask));

        // CIA2
        let c2 = &self.cpu.bus.cia2;
        let cia2_sec = DebugSection::new("CIA2 (NMI/IEC)")
            .row("Timer A", format!("${:04X}/{:04X} {}", c2.timer_a_counter(), c2.timer_a_latch(),
                                    if c2.timer_a_running() { "run" } else { "stop" }))
            .row("Timer B", format!("${:04X}/{:04X} {}", c2.timer_b_counter(), c2.timer_b_latch(),
                                    if c2.timer_b_running() { "run" } else { "stop" }))
            .row("PRA/DDRA", format!("${:02X}/${:02X}", c2.pra, c2.ddra));

        vec![vic_sec, sid_sec, cia1_sec, cia2_sec]
    }
}
