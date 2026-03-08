use emu_common::{AudioSample, Bus, Button, CpuDebugState, DebugSection, FrameBuffer, InputEvent, SystemEmulator};
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
    fn save_state_system_id(&self) -> &str { "NES" }

    fn cpu_state(&self) -> CpuDebugState {
        CpuDebugState { pc: self.cpu.pc, sp: self.cpu.sp, a: self.cpu.a,
            x: self.cpu.x, y: self.cpu.y, flags: self.cpu.p.bits(), cycles: self.cpu.total_cycles }
    }
    fn peek_memory(&self, addr: u16) -> u8 { self.cpu.bus.peek(addr) }
    fn disassemble(&self, addr: u16) -> (String, u16) {
        emu_cpu::disassemble_6502(|a| self.cpu.bus.peek(a), addr)
    }
    fn step_instruction(&mut self) { self.cpu.step(); }

    fn system_debug_panels(&self) -> Vec<DebugSection> {
        let apu  = &self.cpu.bus.apu;
        let ppu  = &self.cpu.bus.ppu;
        let cart = &self.cpu.bus.cartridge;

        // PPU
        let ppu_sec = DebugSection::new("PPU")
            .row("Scanline", format!("{}", ppu.scanline))
            .row("Cycle",    format!("{}", ppu.cycle))
            .row("CTRL",     format!("${:02X}", ppu.ctrl))
            .row("MASK",     format!("${:02X}", ppu.mask))
            .row("V addr",   format!("${:04X}", ppu.v))
            .row("Sprites",  format!("{} active", {
                (0..64usize).filter(|i| ppu.oam[i * 4] < 0xEF).count()
            }));

        // OAM
        let mut oam_sec = DebugSection::new("OAM Sprites");
        for i in 0..64usize {
            let y    = ppu.oam[i * 4];
            let tile = ppu.oam[i * 4 + 1];
            let attr = ppu.oam[i * 4 + 2];
            let x    = ppu.oam[i * 4 + 3];
            if y < 0xEF {
                let flip = format!("{}{}",
                    if attr & 0x40 != 0 { "H" } else { "" },
                    if attr & 0x80 != 0 { "V" } else { "" },
                );
                oam_sec = oam_sec.row(
                    format!("#{:02}", i),
                    format!("({:3},{:3}) tile:{:02X} pal:{} flip:{}", x, y, tile, attr & 3, flip),
                );
            }
        }

        // APU
        let apu_sec = DebugSection::new("APU")
            .row("Pulse1",   format!("len={:3} period={:4} vol={}",
                apu.pulse1.length_counter, apu.pulse1.timer_period, apu.pulse1.output()))
            .row("Pulse2",   format!("len={:3} period={:4} vol={}",
                apu.pulse2.length_counter, apu.pulse2.timer_period, apu.pulse2.output()))
            .row("Triangle", format!("len={:3} period={:4}",
                apu.triangle.length_counter, apu.triangle.timer_period))
            .row("Noise",    format!("len={:3} period={:4} vol={}",
                apu.noise.length_counter, apu.noise.timer_period, apu.noise.output()))
            .row("DMC",      format!("remain={} level={} irq={}",
                apu.dmc.bytes_remaining(), apu.dmc.output(), apu.dmc.irq_pending))
            .row("Frame IRQ", format!("{}", apu.frame_irq_pending));

        // Mapper + pattern table peek summary
        let ctrl = ppu.ctrl;
        let bg_half  = (ctrl >> 4) & 1;
        let spr_half = (ctrl >> 3) & 1;
        let mut chr_rows: Vec<(u8, String)> = Vec::new();
        for half in [bg_half, spr_half] {
            let base = (half as u16) * 0x1000;
            // Sample first byte of each of the 256 tiles
            let non_zero = (0..256u16)
                .filter(|t| cart.mapper.ppu_peek(base + t * 16) != 0)
                .count();
            chr_rows.push((half, format!("{}/256 tiles non-zero", non_zero)));
        }
        let mapper_sec = DebugSection::new("Mapper")
            .row("Mirroring",  format!("{:?}", cart.mapper.mirroring()))
            .row("IRQ",        format!("{}", cart.mapper.irq_pending()))
            .row("CHR BG",     chr_rows[0].1.clone())
            .row("CHR SPR",    chr_rows[1].1.clone());

        vec![ppu_sec, oam_sec, apu_sec, mapper_sec]
    }

    fn supports_save_states(&self) -> bool { true }

    fn save_state(&self) -> Result<Vec<u8>, String> {
        let snap = crate::snapshot::NesSnapshot {
            cpu:             self.cpu.snapshot(),
            ram:             self.cpu.bus.ram.to_vec(),
            ppu:             self.cpu.bus.ppu.snapshot(),
            apu:             self.cpu.bus.apu.snapshot(),
            mapper_state:    self.cpu.bus.cartridge.mapper.mapper_state(),
            oam_dma_pending: self.cpu.bus.oam_dma_pending,
            oam_dma_page:    self.cpu.bus.oam_dma_page,
            ppu_nmi_pending: self.cpu.bus.ppu_nmi_pending,
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
        if snap.ram.len() != self.cpu.bus.ram.len() {
            return Err(format!(
                "NES save state: expected {} bytes for RAM, got {}",
                self.cpu.bus.ram.len(),
                snap.ram.len()
            ));
        }
        self.cpu.bus.ram.copy_from_slice(&snap.ram);
        self.cpu.bus.ppu.restore(&snap.ppu);
        self.cpu.bus.apu.restore(&snap.apu);
        self.cpu.bus.cartridge.mapper.restore_mapper_state(&snap.mapper_state);
        self.cpu.bus.oam_dma_pending = snap.oam_dma_pending;
        self.cpu.bus.oam_dma_page    = snap.oam_dma_page;
        self.cpu.bus.ppu_nmi_pending = snap.ppu_nmi_pending;
        Ok(())
    }
}
