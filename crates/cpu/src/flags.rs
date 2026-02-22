use bitflags::bitflags;

bitflags! {
    /// 6502 processor status register (P).
    /// Bit layout: NV-BDIZC
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct StatusFlags: u8 {
        const CARRY     = 0b0000_0001; // C - bit 0
        const ZERO      = 0b0000_0010; // Z - bit 1
        const IRQ_DISABLE = 0b0000_0100; // I - bit 2
        const DECIMAL   = 0b0000_1000; // D - bit 3
        const BREAK     = 0b0001_0000; // B - bit 4 (not a real flag, only exists on stack)
        const UNUSED    = 0b0010_0000; // bit 5 - always 1 when pushed
        const OVERFLOW  = 0b0100_0000; // V - bit 6
        const NEGATIVE  = 0b1000_0000; // N - bit 7
    }
}

impl StatusFlags {
    /// Create flags from a raw byte (as read from stack).
    /// Bit 5 is always set, bit 4 is ignored.
    pub fn from_stack(val: u8) -> Self {
        Self::from_bits_truncate(val) | Self::UNUSED
    }

    /// Value to push onto the stack. Bit 5 always set.
    /// `brk` parameter controls the B flag.
    pub fn to_stack(self, brk: bool) -> u8 {
        let mut val = self.bits() | Self::UNUSED.bits();
        if brk {
            val |= Self::BREAK.bits();
        } else {
            val &= !Self::BREAK.bits();
        }
        val
    }

    /// Update N and Z flags based on a value.
    #[inline]
    pub fn set_nz(&mut self, val: u8) -> u8 {
        self.set(Self::ZERO, val == 0);
        self.set(Self::NEGATIVE, val & 0x80 != 0);
        val
    }
}

impl Default for StatusFlags {
    fn default() -> Self {
        Self::IRQ_DISABLE | Self::UNUSED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_flags() {
        let f = StatusFlags::default();
        assert!(f.contains(StatusFlags::IRQ_DISABLE));
        assert!(f.contains(StatusFlags::UNUSED));
        assert!(!f.contains(StatusFlags::CARRY));
    }

    #[test]
    fn test_nz_zero() {
        let mut f = StatusFlags::default();
        f.set_nz(0);
        assert!(f.contains(StatusFlags::ZERO));
        assert!(!f.contains(StatusFlags::NEGATIVE));
    }

    #[test]
    fn test_nz_negative() {
        let mut f = StatusFlags::default();
        f.set_nz(0x80);
        assert!(!f.contains(StatusFlags::ZERO));
        assert!(f.contains(StatusFlags::NEGATIVE));
    }

    #[test]
    fn test_stack_push_brk() {
        let f = StatusFlags::CARRY | StatusFlags::UNUSED;
        let pushed = f.to_stack(true);
        assert_eq!(pushed & 0x30, 0x30); // B and unused both set
        assert_eq!(pushed & 0x01, 0x01); // carry preserved
    }

    #[test]
    fn test_stack_push_irq() {
        let f = StatusFlags::CARRY | StatusFlags::UNUSED;
        let pushed = f.to_stack(false);
        assert_eq!(pushed & 0x30, 0x20); // unused set, B clear
    }
}
