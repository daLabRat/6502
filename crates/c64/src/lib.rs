pub mod bus;
pub mod cia;
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

use emu_common::{AudioSample, Button, FrameBuffer, InputEvent, SystemEmulator};
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
        })
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
        // C64 → IEC bus: push CIA2 PA output to IEC bus lines
        let cia2_pa = self.cpu.bus.cia2.pra & self.cpu.bus.cia2.ddra;
        self.iec_bus.update_from_cia2(cia2_pa);

        // IEC bus → 1541 drive
        if let Some(ref mut dcpu) = self.drive_cpu {
            dcpu.bus.sync_iec_input(&self.iec_bus);
            // 1541 → IEC bus
            dcpu.bus.sync_iec_output(&mut self.iec_bus);
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
        _ => None,
    }
}

const LEFT_SHIFT: (u8, u8) = (1, 7);

impl SystemEmulator for C64 {
    fn step_frame(&mut self) -> usize {
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
                // Sync IEC after drive step so C64 sees current drive output
                self.sync_iec();
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
}
