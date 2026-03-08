use emu_common::{AudioSample, Bus, Button, CpuDebugState, FrameBuffer, InputEvent, SystemEmulator};
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

    fn cpu_state(&self) -> CpuDebugState {
        CpuDebugState { pc: self.cpu.pc, sp: self.cpu.sp, a: self.cpu.a,
            x: self.cpu.x, y: self.cpu.y, flags: self.cpu.p.bits(), cycles: self.cpu.total_cycles }
    }
    fn peek_memory(&self, addr: u16) -> u8 { self.cpu.bus.peek(addr) }
    fn disassemble(&self, addr: u16) -> (String, u16) {
        emu_cpu::disassemble_6502(|a| self.cpu.bus.peek(a), addr)
    }
    fn step_instruction(&mut self) { self.cpu.step(); }

    fn supports_save_states(&self) -> bool { true }

    fn save_state(&self) -> Result<Vec<u8>, String> {
        let snap = crate::snapshot::NesSnapshot {
            cpu:          self.cpu.snapshot(),
            ram:          self.cpu.bus.ram.to_vec(),
            ppu:          self.cpu.bus.ppu.snapshot(),
            apu:          self.cpu.bus.apu.snapshot(),
            mapper_state: self.cpu.bus.cartridge.mapper.mapper_state(),
        };
        let bytes = bincode::serde::encode_to_vec(&snap, bincode::config::standard())
            .map_err(|e| e.to_string())?;
        Ok(emu_common::save_encode("NES", &bytes))
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        let payload = emu_common::save_decode("NES", data)?;
        let (snap, _): (crate::snapshot::NesSnapshot, _) =
            bincode::serde::decode_from_slice(payload, bincode::config::standard())
                .map_err(|e| e.to_string())?;
        self.cpu.restore(&snap.cpu);
        self.cpu.bus.ram.copy_from_slice(&snap.ram);
        self.cpu.bus.ppu.restore(&snap.ppu);
        self.cpu.bus.apu.restore(&snap.apu);
        self.cpu.bus.cartridge.mapper.restore_mapper_state(&snap.mapper_state);
        Ok(())
    }
}
