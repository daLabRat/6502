use serde::{Serialize, Deserialize};
use emu_cpu::Cpu6502Snapshot;

#[derive(Serialize, Deserialize)]
pub struct MemorySnapshot {
    pub ram: Vec<u8>,          // [u8; 49152]
    pub lc_ram: Vec<u8>,       // [u8; 16384]
    pub lc_bank2: Vec<u8>,     // [u8; 4096]
    pub lc_read_enable: bool,
    pub lc_write_enable: bool,
    pub lc_prewrite: bool,
    pub lc_bank1: bool,
    pub aux_ram: Vec<u8>,
    pub aux_lc_ram: Vec<u8>,
    pub aux_lc_bank2: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct SoftSwitchSnapshot {
    pub text_mode: bool, pub mixed_mode: bool, pub page2: bool, pub hires: bool,
    pub an0: bool, pub an1: bool, pub an2: bool, pub an3: bool,
    pub store80: bool, pub ramrd: bool, pub ramwrt: bool, pub altzp: bool,
    pub intcxrom: bool, pub slotc3rom: bool, pub col80: bool, pub altcharset: bool,
    pub lc_bank2: bool, pub lc_read_enable: bool, pub vbl: bool,
    pub is_iie: bool, pub intc8rom: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Apple2Snapshot {
    pub cpu: Cpu6502Snapshot,
    pub memory: MemorySnapshot,
    pub switches: SoftSwitchSnapshot,
    pub keyboard_latch: u8,
    pub keyboard_strobe: bool,
    pub speaker_state: bool,
    pub speaker_active: bool,
    pub speaker_cycles_since_toggle: u64,
    pub speaker_cycle_count: u64,
    pub bus_cycle_count: u64,
}
