pub mod bus;
pub mod disk_ii;
pub mod keyboard;
pub mod memory;
pub mod soft_switch;
pub mod speaker;
pub mod video;

use emu_common::{AudioSample, Bus, Button, FrameBuffer, InputEvent, SystemEmulator};
use emu_cpu::Cpu6502;
use bus::Apple2Bus;

/// Apple II system emulator.
pub struct Apple2 {
    cpu: Cpu6502<Apple2Bus>,
}

impl Apple2 {
    /// Create an Apple II from ROM data.
    /// The ROM should be the Apple II+ or IIe ROM image.
    pub fn from_rom(rom_data: &[u8]) -> Result<Self, String> {
        if rom_data.is_empty() {
            return Err("ROM data is empty".into());
        }

        let mut bus = Apple2Bus::new();
        bus.memory.load_rom(rom_data);

        let is_iie = rom_data.len() >= 32768;
        bus.switches.is_iie = is_iie;
        let mut cpu = Cpu6502::new(bus);
        cpu.bcd_enabled = true; // Apple II uses BCD
        cpu.cmos_mode = is_iie; // IIe uses 65C02 (CMOS)
        cpu.reset();

        Ok(Self { cpu })
    }

    /// Create an Apple II with a Disk II controller and a .dsk image loaded.
    /// `system_rom` is the Apple II+ or IIe ROM.
    /// `disk_rom` is the P5 boot PROM (256 bytes, diskII.c600.c6ff.bin).
    /// `dsk_data` is the .dsk disk image (143360 bytes).
    pub fn with_disk(
        system_rom: &[u8],
        disk_rom: &[u8],
        dsk_data: &[u8],
    ) -> Result<Self, String> {
        if system_rom.is_empty() {
            return Err("System ROM data is empty".into());
        }

        let is_iie = system_rom.len() >= 32768;
        let mut bus = Apple2Bus::new();
        bus.switches.is_iie = is_iie;
        bus.memory.load_rom(system_rom);
        bus.disk_ii.load_boot_rom(disk_rom);
        bus.disk_ii.load_dsk(dsk_data)?;

        log::info!("Apple II{}: system ROM {} bytes, disk ROM {} bytes",
            if is_iie { "e" } else { "+" },
            system_rom.len(), disk_rom.len());
        let reset_lo = bus.memory.read(0xFFFC);
        let reset_hi = bus.memory.read(0xFFFD);
        let reset_vec = (reset_hi as u16) << 8 | reset_lo as u16;
        log::info!("Apple II: reset vector = ${:04X}", reset_vec);
        log::info!("Apple II: boot ROM $C600 first byte = ${:02X}",
            bus.disk_ii.read_rom(0xC600));

        if is_iie {
            // Log IIe identification and 80-column firmware bytes
            let fbb3 = bus.memory.rom[(0xFBB3 - 0xC000) as usize];
            let fbc0 = bus.memory.rom[(0xFBC0 - 0xC000) as usize];
            log::info!("IIe ID: $FBB3={:02X} (expect 06), $FBC0={:02X} (00=orig, EA=enhanced)", fbb3, fbc0);
            // 80-col firmware at $C300
            let c300_bytes: Vec<u8> = (0..16).map(|i| bus.memory.rom[0x300 + i]).collect();
            log::info!("IIe $C300-$C30F: {:02X?}", c300_bytes);
            // $C305 should be $38 (SEC) for 80-col card ID
            log::info!("IIe $C305={:02X} (expect 38 for 80-col)", bus.memory.rom[0x305]);
            // Also check $C800 expansion firmware
            let c800_bytes: Vec<u8> = (0..16).map(|i| bus.memory.rom[0x800 + i]).collect();
            log::info!("IIe $C800-$C80F: {:02X?}", c800_bytes);
        }

        let mut cpu = Cpu6502::new(bus);
        cpu.bcd_enabled = true;
        cpu.cmos_mode = is_iie; // IIe uses 65C02 (CMOS)
        cpu.reset();

        Ok(Self { cpu })
    }
}

impl SystemEmulator for Apple2 {
    fn step_frame(&mut self) -> usize {
        loop {
            self.cpu.bus.debug_pc = self.cpu.pc;
            self.cpu.bus.debug_sp = self.cpu.sp;
            self.cpu.bus.debug_x = self.cpu.x;
            // Trace VTAB code path: log all instructions from JSR $FC22 through return
            // Only trace first 200 instructions in VTAB during frame 508
            let fc = self.cpu.bus.frame_count;
            if fc == 508 {
                let pc = self.cpu.pc;
                static VTAB_TRACE_COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                static VTAB_TRACING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                let tracing = VTAB_TRACING.load(std::sync::atomic::Ordering::Relaxed);
                let count = VTAB_TRACE_COUNT.load(std::sync::atomic::Ordering::Relaxed);
                if pc == 0xFC22 && !tracing && count == 0 {
                    VTAB_TRACING.store(true, std::sync::atomic::Ordering::Relaxed);
                    log::info!("=== VTAB ENTRY === PC=${:04X} A=${:02X} X=${:02X} Y=${:02X} SP=${:02X}",
                        pc, self.cpu.a, self.cpu.x, self.cpu.y, self.cpu.sp);
                }
                if tracing && count < 200 {
                    let opcode = self.cpu.bus.peek(pc);
                    let b1 = self.cpu.bus.peek(pc.wrapping_add(1));
                    let b2 = self.cpu.bus.peek(pc.wrapping_add(2));
                    log::info!("  VT {:04X}: {:02X} {:02X} {:02X}  A={:02X} X={:02X} Y={:02X} SP={:02X}",
                        pc, opcode, b1, b2, self.cpu.a, self.cpu.x, self.cpu.y, self.cpu.sp);
                    VTAB_TRACE_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    // Stop tracing when we return to Bitsy Bye code
                    if pc < 0xC000 && pc != 0xFC22 && count > 5 {
                        log::info!("=== VTAB RETURN === PC=${:04X} A=${:02X} X=${:02X} Y=${:02X}",
                            pc, self.cpu.a, self.cpu.x, self.cpu.y);
                        VTAB_TRACING.store(false, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
            self.cpu.step();
            if self.cpu.bus.is_frame_ready() {
                break;
            }
        }
        0
    }

    fn framebuffer(&self) -> &FrameBuffer {
        &self.cpu.bus.framebuffer
    }

    fn audio_samples(&mut self, out: &mut [AudioSample]) -> usize {
        self.cpu.bus.speaker.drain_samples(out)
    }

    fn handle_input(&mut self, event: InputEvent) {
        if event.pressed {
            if let Button::Key(ascii) = event.button {
                self.cpu.bus.keyboard.key_press(ascii);
            }
        }
    }

    fn reset(&mut self) {
        self.cpu.reset();
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.cpu.bus.speaker.set_sample_rate(rate);
    }

    fn display_width(&self) -> u32 { 560 }
    fn display_height(&self) -> u32 { 192 }
    fn target_fps(&self) -> f64 { 60.0 }
    fn system_name(&self) -> &str { "Apple II" }
}
