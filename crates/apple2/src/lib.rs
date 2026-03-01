pub mod bus;
pub mod disk_ii;
pub mod keyboard;
pub mod memory;
pub mod soft_switch;
pub mod speaker;
pub mod video;

use emu_common::{AudioSample, Button, FrameBuffer, InputEvent, SystemEmulator};
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
