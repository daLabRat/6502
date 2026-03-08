use serde::{Serialize, Deserialize};

/// Plain-data snapshot of Cpu6502 register state.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Cpu6502Snapshot {
    pub pc: u16,
    pub sp: u8,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub p: u8,           // StatusFlags bits
    pub bcd_enabled: bool,
    pub cmos_mode: bool,
    pub total_cycles: u64,
    pub jammed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_cpu_snapshot() {
        let snap = Cpu6502Snapshot {
            pc: 0x1234, sp: 0xFD, a: 0xAB, x: 0x01, y: 0x02,
            p: 0x24, // IRQ_DISABLE | UNUSED — power-on default
            bcd_enabled: true, cmos_mode: false,
            total_cycles: 12345, jammed: false,
        };
        let bytes = bincode::serde::encode_to_vec(&snap, bincode::config::standard()).unwrap();
        let (decoded, _): (Cpu6502Snapshot, _) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(decoded.pc, 0x1234);
        assert_eq!(decoded.sp, 0xFD);
        assert_eq!(decoded.a, 0xAB);
        assert_eq!(decoded.x, 0x01);
        assert_eq!(decoded.y, 0x02);
        assert_eq!(decoded.p, 0x24);
        assert_eq!(decoded.bcd_enabled, true);
        assert_eq!(decoded.cmos_mode, false);
        assert_eq!(decoded.total_cycles, 12345);
        assert_eq!(decoded.jammed, false);
    }
}
