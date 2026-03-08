use serde::{Serialize, Deserialize};
use emu_cpu::Cpu6502Snapshot;

#[derive(Serialize, Deserialize)]
pub struct VoiceSnapshot {
    pub frequency: u16,
    pub pulse_width: u16,
    pub control: u8,
    pub attack: u8,
    pub decay: u8,
    pub sustain: u8,
    pub release: u8,
    pub gate: bool,
    pub accumulator: u32,
    pub prev_msb: bool,
    pub noise_lfsr: u32,
    pub envelope: u8,
    pub envelope_state: u8,   // 0=Attack, 1=Decay, 2=Sustain, 3=Release
    pub envelope_counter: u32,
}

#[derive(Serialize, Deserialize)]
pub struct SidSnapshot {
    pub voices: [VoiceSnapshot; 3],
    pub filter_cutoff: u16,
    pub filter_resonance: u8,
    pub filter_mode: u8,
    pub filter_routing: u8,
    pub voice3_off: bool,
    pub volume: u8,
    pub filter_bp: f32,
    pub filter_lp: f32,
}

#[derive(Serialize, Deserialize)]
pub struct VicSnapshot {
    pub registers: Vec<u8>,        // [u8; 64]
    pub raster_line: u16,
    pub raster_irq_line: u16,
    pub cycle: u16,
    pub irq_pending: bool,
    pub color_ram: Vec<u8>,        // [u8; 1024]
    pub vic_bank_base: u16,
    pub sprite_sprite_collision: u8,
    pub sprite_bg_collision: u8,
    pub stall_cycles: u8,
}

#[derive(Serialize, Deserialize)]
pub struct CiaSnapshot {
    pub pra: u8,
    pub prb: u8,
    pub ddra: u8,
    pub ddrb: u8,
    pub timer_a_latch: u16,
    pub timer_a_counter: u16,
    pub timer_a_running: bool,
    pub timer_a_oneshot: bool,
    pub timer_b_latch: u16,
    pub timer_b_counter: u16,
    pub timer_b_running: bool,
    pub timer_b_oneshot: bool,
    pub icr_data: u8,
    pub icr_mask: u8,
    pub irq_pending: bool,
    pub is_cia1: bool,
    pub keyboard_matrix: [u8; 8],
    pub joy2: u8,
}

#[derive(Serialize, Deserialize)]
pub struct C64Snapshot {
    pub cpu: Cpu6502Snapshot,
    pub ram: Vec<u8>,              // [u8; 65536]
    pub cpu_port: u8,
    pub cpu_port_dir: u8,
    pub vic: VicSnapshot,
    pub sid: SidSnapshot,
    pub cia1: CiaSnapshot,
    pub cia2: CiaSnapshot,
}
