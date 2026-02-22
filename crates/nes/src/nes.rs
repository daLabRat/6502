use emu_common::{AudioSample, Button, FrameBuffer, InputEvent, SystemEmulator};
use emu_cpu::Cpu6502;
use crate::bus::NesBus;
use crate::cartridge;

/// NES system emulator.
pub struct Nes {
    cpu: Cpu6502<NesBus>,
}

impl Nes {
    /// Create a new NES from ROM data.
    pub fn from_rom(rom_data: &[u8]) -> Result<Self, String> {
        let cart = cartridge::ines::parse(rom_data)?;
        let bus = NesBus::new(cart);
        let mut cpu = Cpu6502::new(bus);
        cpu.bcd_enabled = false; // NES 2A03 has no BCD
        cpu.reset();
        Ok(Self { cpu })
    }
}

impl SystemEmulator for Nes {
    fn step_frame(&mut self) -> usize {
        self.cpu.bus.ppu.frame_ready = false;

        while !self.cpu.bus.ppu.frame_ready {
            self.cpu.step();
        }

        self.cpu.bus.apu.sample_buffer.len()
    }

    fn framebuffer(&self) -> &FrameBuffer {
        &self.cpu.bus.ppu.framebuffer
    }

    fn audio_samples(&mut self, out: &mut [AudioSample]) -> usize {
        self.cpu.bus.apu.drain_samples(out)
    }

    fn handle_input(&mut self, event: InputEvent) {
        let controller = if event.port == 0 {
            &mut self.cpu.bus.controller1
        } else {
            &mut self.cpu.bus.controller2
        };

        let bit = match event.button {
            Button::A => 0x01,
            Button::B => 0x02,
            Button::Select => 0x04,
            Button::Start => 0x08,
            Button::Up => 0x10,
            Button::Down => 0x20,
            Button::Left => 0x40,
            Button::Right => 0x80,
            _ => return,
        };

        if event.pressed {
            controller.buttons |= bit;
        } else {
            controller.buttons &= !bit;
        }
    }

    fn reset(&mut self) {
        self.cpu.reset();
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.cpu.bus.apu.set_sample_rate(rate);
    }

    fn display_width(&self) -> u32 { 256 }
    fn display_height(&self) -> u32 { 240 }
    fn target_fps(&self) -> f64 { 60.0988 }
    fn system_name(&self) -> &str { "NES" }
}
