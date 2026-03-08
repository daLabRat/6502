pub mod bus;
pub mod cartridge;
pub mod riot;
pub mod tia;
mod snapshot;

use emu_common::{AudioSample, Bus, Button, CpuDebugState, FrameBuffer, InputEvent, SystemEmulator};
use crate::snapshot::Atari2600Snapshot;
use emu_cpu::Cpu6502;
use bus::Atari2600Bus;

/// Atari 2600 system emulator.
pub struct Atari2600 {
    cpu: Cpu6502<Atari2600Bus>,
}

impl Atari2600 {
    pub fn from_rom(rom_data: &[u8]) -> Result<Self, String> {
        let cart = cartridge::Cartridge::new(rom_data)?;
        let bus = Atari2600Bus::new(cart);
        let mut cpu = Cpu6502::new(bus);
        cpu.bcd_enabled = true; // 6507 supports BCD
        cpu.reset();
        Ok(Self { cpu })
    }
}

impl SystemEmulator for Atari2600 {
    fn step_frame(&mut self) -> usize {
        self.cpu.bus.tia.frame_ready = false;

        loop {
            // Handle WSYNC: TIA halts CPU until end of scanline
            if self.cpu.bus.tia.wsync {
                while self.cpu.bus.tia.wsync {
                    self.cpu.bus.tia.step_clock();
                    self.cpu.bus.tia.step_clock();
                    self.cpu.bus.tia.step_clock();
                    self.cpu.bus.riot.step();
                }
            }

            self.cpu.step();

            if self.cpu.bus.tia.frame_ready {
                break;
            }
        }

        0
    }

    fn framebuffer(&self) -> &FrameBuffer {
        &self.cpu.bus.tia.framebuffer
    }

    fn audio_samples(&mut self, out: &mut [AudioSample]) -> usize {
        self.cpu.bus.tia.drain_samples(out)
    }

    fn handle_input(&mut self, event: InputEvent) {
        // Joystick directions through RIOT SWCHA
        // P0: bits 4-7 (active low), P1: bits 0-3 (active low)
        let mask = if event.port == 0 {
            match event.button {
                Button::Up => Some(0x10u8),
                Button::Down => Some(0x20),
                Button::Left => Some(0x40),
                Button::Right => Some(0x80),
                _ => None,
            }
        } else {
            match event.button {
                Button::Up => Some(0x01),
                Button::Down => Some(0x02),
                Button::Left => Some(0x04),
                Button::Right => Some(0x08),
                _ => None,
            }
        };

        if let Some(mask) = mask {
            if event.pressed {
                self.cpu.bus.riot.swcha &= !mask; // Active low
            } else {
                self.cpu.bus.riot.swcha |= mask;
            }
        }

        // Fire button → TIA INPT4 (P0) / INPT5 (P1)
        if event.button == Button::Fire || event.button == Button::A {
            if event.port == 0 {
                // inpt4: true = not pressed, false = pressed
                self.cpu.bus.tia.inpt4 = !event.pressed;
            } else {
                self.cpu.bus.tia.inpt5 = !event.pressed;
            }
        }

        // Console switches (active low)
        match event.button {
            Button::Start => {
                if event.pressed {
                    self.cpu.bus.riot.swchb &= !0x01; // Reset switch
                } else {
                    self.cpu.bus.riot.swchb |= 0x01;
                }
            }
            Button::Select => {
                if event.pressed {
                    self.cpu.bus.riot.swchb &= !0x02; // Select switch
                } else {
                    self.cpu.bus.riot.swchb |= 0x02;
                }
            }
            _ => {}
        }
    }

    fn supports_save_states(&self) -> bool { true }

    fn save_state(&self) -> Result<Vec<u8>, String> {
        let snap = Atari2600Snapshot {
            cpu:  self.cpu.snapshot(),
            tia:  self.cpu.bus.tia.snapshot(),
            riot: self.cpu.bus.riot.snapshot(),
        };
        let bytes = bincode::serde::encode_to_vec(&snap, bincode::config::standard())
            .map_err(|e| e.to_string())?;
        // Use a hardcoded stable identifier — NOT self.system_name() — so renaming the
        // display name never breaks existing save files.
        Ok(emu_common::save_encode("Atari2600", &bytes))
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        let payload = emu_common::save_decode("Atari2600", data)?;
        let (snap, _): (Atari2600Snapshot, _) =
            bincode::serde::decode_from_slice(payload, bincode::config::standard())
                .map_err(|e| e.to_string())?;
        self.cpu.restore(&snap.cpu);
        self.cpu.bus.tia.restore(&snap.tia);
        self.cpu.bus.riot.restore(&snap.riot);
        Ok(())
    }

    fn reset(&mut self) {
        self.cpu.reset();
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.cpu.bus.tia.set_sample_rate(rate);
    }

    fn display_width(&self) -> u32 { tia::SCREEN_WIDTH }
    fn display_height(&self) -> u32 { tia::SCREEN_HEIGHT }
    fn target_fps(&self) -> f64 { 59.94 }
    fn system_name(&self) -> &str { "Atari 2600" }
    fn save_state_system_id(&self) -> &str { "Atari2600" }

    fn cpu_state(&self) -> CpuDebugState {
        CpuDebugState { pc: self.cpu.pc, sp: self.cpu.sp, a: self.cpu.a,
            x: self.cpu.x, y: self.cpu.y, flags: self.cpu.p.bits(), cycles: self.cpu.total_cycles }
    }
    fn peek_memory(&self, addr: u16) -> u8 { self.cpu.bus.peek(addr) }
    fn disassemble(&self, addr: u16) -> (String, u16) {
        emu_cpu::disassemble_6502(|a| self.cpu.bus.peek(a), addr)
    }
    fn step_instruction(&mut self) { self.cpu.step(); }
}
