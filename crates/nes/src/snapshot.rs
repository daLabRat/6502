use serde::{Serialize, Deserialize};
use emu_cpu::Cpu6502Snapshot;

#[derive(Serialize, Deserialize)]
pub struct PpuSnapshot {
    pub nametable_ram: Vec<u8>,  // [u8; 2048]
    pub palette_ram: [u8; 32],
    pub oam: Vec<u8>,            // [u8; 256]
    pub ctrl: u8,
    pub mask: u8,
    pub status: u8,
    pub oam_addr: u8,
    pub v: u16,
    pub t: u16,
    pub fine_x: u8,
    pub w: bool,
    pub data_buffer: u8,
    pub scanline: i16,
    pub cycle: u16,
    pub frame_count: u64,
    pub nmi_pending: bool,
}

#[derive(Serialize, Deserialize)]
pub struct PulseSnapshot {
    pub duty: u8,
    pub duty_pos: u8,
    pub timer_period: u16,
    pub timer_counter: u16,
    pub length_counter: u8,
    pub length_halt: bool,
    pub envelope_start: bool,
    pub envelope_loop: bool,
    pub constant_volume: bool,
    pub envelope_period: u8,
    pub envelope_counter: u8,
    pub envelope_decay: u8,
    pub sweep_enabled: bool,
    pub sweep_period: u8,
    pub sweep_negate: bool,
    pub sweep_shift: u8,
    pub sweep_counter: u8,
    pub sweep_reload: bool,
    pub is_pulse1: bool,
}

#[derive(Serialize, Deserialize)]
pub struct TriangleSnapshot {
    pub timer_period: u16,
    pub timer_counter: u16,
    pub sequence_pos: u8,
    pub length_counter: u8,
    pub length_halt: bool,
    pub linear_counter: u8,
    pub linear_reload_value: u8,
    pub linear_reload_flag: bool,
    pub control_flag: bool,
}

#[derive(Serialize, Deserialize)]
pub struct NoiseSnapshot {
    pub timer_period: u16,
    pub timer_counter: u16,
    pub shift: u16,
    pub mode: bool,
    pub length_counter: u8,
    pub length_halt: bool,
    pub envelope_start: bool,
    pub envelope_loop: bool,
    pub constant_volume: bool,
    pub envelope_period: u8,
    pub envelope_counter: u8,
    pub envelope_decay: u8,
}

#[derive(Serialize, Deserialize)]
pub struct DmcSnapshot {
    pub timer_period: u16,
    pub timer_counter: u16,
    pub output_level: u8,
    pub sample_addr: u16,
    pub sample_length: u16,
    pub current_addr: u16,
    pub bytes_remaining: u16,
    pub shift_register: u8,
    pub bits_remaining: u8,
    pub sample_buffer: Option<u8>,
    pub silence_flag: bool,
    pub irq_enabled: bool,
    pub loop_flag: bool,
    pub irq_pending: bool,
}

#[derive(Serialize, Deserialize)]
pub struct ApuSnapshot {
    pub pulse1: PulseSnapshot,
    pub pulse2: PulseSnapshot,
    pub triangle: TriangleSnapshot,
    pub noise: NoiseSnapshot,
    pub dmc: DmcSnapshot,
    pub frame_counter_mode: u8,
    pub frame_counter: u32,
    pub frame_irq_inhibit: bool,
    pub frame_irq_pending: bool,
    pub enabled: [bool; 5],
}

#[derive(Serialize, Deserialize)]
pub struct NesSnapshot {
    pub cpu: Cpu6502Snapshot,
    pub ram: Vec<u8>,            // [u8; 2048]
    pub ppu: PpuSnapshot,
    pub apu: ApuSnapshot,
    pub mapper_state: Vec<u8>,
    pub oam_dma_pending: bool,
    pub oam_dma_page: u8,
    pub ppu_nmi_pending: bool,
}
