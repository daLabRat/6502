pub mod ay3_8910;
pub mod bus;
pub mod disk_ii;
pub mod keyboard;
pub mod memory;
mod snapshot;
pub mod soft_switch;
pub mod speaker;
pub mod video;

use emu_common::{AudioSample, Bus, Button, CpuDebugState, FrameBuffer, InputEvent, SystemEmulator};
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
    fn save_state_system_id(&self) -> &str { "Apple2" }

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

    fn take_modified_disk_image(&mut self) -> Option<Vec<u8>> {
        if self.cpu.bus.disk_ii.is_dirty() {
            let data = self.cpu.bus.disk_ii.get_modified_dsk();
            if data.is_some() {
                self.cpu.bus.disk_ii.clear_dirty();
            }
            data
        } else {
            None
        }
    }

    fn save_state(&self) -> Result<Vec<u8>, String> {
        let snap = crate::snapshot::Apple2Snapshot {
            cpu: self.cpu.snapshot(),
            memory: self.cpu.bus.memory.snapshot(),
            switches: self.cpu.bus.switches.snapshot(),
            keyboard_latch: self.cpu.bus.keyboard.latch,
            keyboard_strobe: self.cpu.bus.keyboard.strobe,
            speaker_state: self.cpu.bus.speaker.state,
            speaker_active: self.cpu.bus.speaker.active,
            speaker_cycles_since_toggle: self.cpu.bus.speaker.cycles_since_toggle,
            speaker_cycle_count: self.cpu.bus.speaker.cycle_count,
            bus_cycle_count: self.cpu.bus.cycle_count,
        };
        let bytes = bincode::serde::encode_to_vec(&snap, bincode::config::standard())
            .map_err(|e| e.to_string())?;
        // Use hardcoded stable identifier — NOT self.system_name() — so renaming
        // the display name never breaks existing save files.
        Ok(emu_common::save_encode("Apple2", &bytes))
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        let payload = emu_common::save_decode("Apple2", data)?;
        let (snap, _): (crate::snapshot::Apple2Snapshot, _) =
            bincode::serde::decode_from_slice(payload, bincode::config::standard())
                .map_err(|e| e.to_string())?;
        self.cpu.restore(&snap.cpu);
        self.cpu.bus.memory.restore(&snap.memory);
        self.cpu.bus.switches.restore(&snap.switches);
        self.cpu.bus.keyboard.latch = snap.keyboard_latch;
        self.cpu.bus.keyboard.strobe = snap.keyboard_strobe;
        self.cpu.bus.speaker.state = snap.speaker_state;
        self.cpu.bus.speaker.active = snap.speaker_active;
        self.cpu.bus.speaker.cycles_since_toggle = snap.speaker_cycles_since_toggle;
        self.cpu.bus.speaker.cycle_count = snap.speaker_cycle_count;
        self.cpu.bus.cycle_count = snap.bus_cycle_count;
        Ok(())
    }
}
