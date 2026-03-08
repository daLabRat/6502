use serde::{Serialize, Deserialize, Serializer, Deserializer};
use emu_cpu::Cpu6502Snapshot;

/// serde helper for [u8; 128] — serde 1.x only auto-impls up to [T; 32].
mod serde_u8_128 {
    use super::*;
    use serde::de::{SeqAccess, Visitor};
    use std::fmt;

    pub fn serialize<S: Serializer>(arr: &[u8; 128], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(arr)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 128], D::Error> {
        struct A128Visitor;
        impl<'de> Visitor<'de> for A128Visitor {
            type Value = [u8; 128];
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("byte array of length 128")
            }
            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                v.try_into().map_err(|_| E::invalid_length(v.len(), &self))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut arr = [0u8; 128];
                for (i, slot) in arr.iter_mut().enumerate() {
                    *slot = seq.next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(i, &self))?;
                }
                Ok(arr)
            }
        }
        d.deserialize_bytes(A128Visitor)
    }
}

#[derive(Serialize, Deserialize)]
pub struct AudioChannelSnapshot {
    pub audc: u8,
    pub audf: u8,
    pub audv: u8,
    pub freq_counter: u8,
    pub poly4: u8,
    pub poly5: u8,
    pub poly9: u16,
    pub div_counter: u8,
    pub output: bool,
}

#[derive(Serialize, Deserialize)]
pub struct TiaSnapshot {
    pub pf0: u8, pub pf1: u8, pub pf2: u8,
    pub pf_reflect: bool, pub pf_score: bool, pub pf_priority: bool,
    pub grp0: u8, pub grp1: u8, pub grp0_old: u8, pub grp1_old: u8,
    pub resp0: u8, pub resp1: u8,
    pub refp0: bool, pub refp1: bool,
    pub vdelp0: bool, pub vdelp1: bool,
    pub enam0: bool, pub enam1: bool,
    pub resm0: u8, pub resm1: u8,
    pub resmp0: bool, pub resmp1: bool,
    pub enabl: bool, pub enabl_old: bool, pub resbl: u8, pub vdelbl: bool,
    pub colup0: u8, pub colup1: u8, pub colupf: u8, pub colubk: u8,
    pub nusiz0: u8, pub nusiz1: u8, pub ctrlpf: u8,
    pub hmp0: i8, pub hmp1: i8, pub hmm0: i8, pub hmm1: i8, pub hmbl: i8,
    pub hmove_pending: bool, pub hmove_blanking: u8,
    pub resp0_delay: u8, pub resp0_pending: u8,
    pub resp1_delay: u8, pub resp1_pending: u8,
    pub resm0_delay: u8, pub resm0_pending: u8,
    pub resm1_delay: u8, pub resm1_pending: u8,
    pub resbl_delay: u8, pub resbl_pending: u8,
    pub inpt4: bool, pub inpt5: bool,
    pub collision: [u8; 8],
    pub scanline: u16, pub clock: u16,
    pub wsync: bool, pub vsync: bool, pub vblank: bool, pub frame_ready: bool,
    pub audio_clock_counter: u16,
    pub audio_ch: [AudioChannelSnapshot; 2],
}

#[derive(Serialize, Deserialize)]
pub struct RiotSnapshot {
    #[serde(with = "serde_u8_128")]
    pub ram: [u8; 128],
    pub swcha: u8, pub swcha_out: u8, pub swacnt: u8,
    pub swchb: u8, pub swbcnt: u8,
    pub timer_value: u8,
    pub timer_interval: u32,
    pub timer_subcycles: u32,
    pub timer_expired: bool,
    pub timer_flag: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Atari2600Snapshot {
    pub cpu: Cpu6502Snapshot,
    pub tia: TiaSnapshot,
    pub riot: RiotSnapshot,
}
