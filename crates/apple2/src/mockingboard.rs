/// Mockingboard (Sweet Micro Systems) in slot 4.
///
/// Two 6522 VIA chips, each controlling one AY-3-8910 PSG.
/// Slot 4 I/O at $C0C0-$C0CF (VIA A) and $C0D0-$C0DF (VIA B).
///
/// VIA Port B bits 0-1 control the AY bus mode:
///   BDIR=bit1, BC1=bit0
///   (false,false)=Inactive, (false,true)=Read, (true,false)=Write, (true,true)=LatchAddr
/// VIA Port A is the AY data bus.
use crate::ay3_8910::Ay3_8910;

#[derive(Clone, Copy)]
enum AyMode { Inactive, ReadReg, WriteReg, LatchAddr }

fn ay_mode(bdir: bool, bc1: bool) -> AyMode {
    match (bdir, bc1) {
        (false, false) => AyMode::Inactive,
        (false, true)  => AyMode::ReadReg,
        (true,  false) => AyMode::WriteReg,
        (true,  true)  => AyMode::LatchAddr,
    }
}

pub struct Mockingboard {
    pub ay0: Ay3_8910,
    pub ay1: Ay3_8910,
    // VIA Port B output registers (control BDIR/BC1)
    via_a_orb: u8, // controls ay0
    via_b_orb: u8, // controls ay1
    // AY address latches
    ay0_addr: u8,
    ay1_addr: u8,
}

impl Mockingboard {
    pub fn new() -> Self {
        Self {
            ay0: Ay3_8910::new(),
            ay1: Ay3_8910::new(),
            via_a_orb: 0,
            via_b_orb: 0,
            ay0_addr: 0,
            ay1_addr: 0,
        }
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.ay0.set_sample_rate(rate);
        self.ay1.set_sample_rate(rate);
    }

    /// I/O read from slot 4 space ($C0C0-$C0CF for VIA A, $C0D0-$C0DF for VIA B).
    pub fn io_read(&mut self, addr: u16) -> u8 {
        match addr & 0x1F {
            0x00 => { // VIA A Port B
                let bdir = self.via_a_orb & 0x02 != 0;
                let bc1  = self.via_a_orb & 0x01 != 0;
                if matches!(ay_mode(bdir, bc1), AyMode::ReadReg) {
                    self.ay0.read_reg(self.ay0_addr)
                } else {
                    self.via_a_orb
                }
            }
            0x01 => self.ay0.read_reg(self.ay0_addr), // VIA A Port A (AY data)
            0x08 => { // VIA B Port B
                let bdir = self.via_b_orb & 0x02 != 0;
                let bc1  = self.via_b_orb & 0x01 != 0;
                if matches!(ay_mode(bdir, bc1), AyMode::ReadReg) {
                    self.ay1.read_reg(self.ay1_addr)
                } else {
                    self.via_b_orb
                }
            }
            0x09 => self.ay1.read_reg(self.ay1_addr), // VIA B Port A (AY data)
            _ => 0,
        }
    }

    /// I/O write to slot 4 space ($C0C0-$C0CF for VIA A, $C0D0-$C0DF for VIA B).
    pub fn io_write(&mut self, addr: u16, val: u8) {
        match addr & 0x1F {
            0x00 => { // VIA A Port B: BC1=bit0, BDIR=bit1
                self.via_a_orb = val;
                if matches!(ay_mode(val & 0x02 != 0, val & 0x01 != 0), AyMode::LatchAddr) {
                    // Address will be set via next Port A write
                }
            }
            0x01 => { // VIA A Port A: AY0 data bus
                let bdir = self.via_a_orb & 0x02 != 0;
                let bc1  = self.via_a_orb & 0x01 != 0;
                match ay_mode(bdir, bc1) {
                    AyMode::LatchAddr => self.ay0_addr = val & 0x0F,
                    AyMode::WriteReg  => self.ay0.write_reg(self.ay0_addr, val),
                    _ => {}
                }
            }
            0x08 => { // VIA B Port B: BC1=bit0, BDIR=bit1
                self.via_b_orb = val;
            }
            0x09 => { // VIA B Port A: AY1 data bus
                let bdir = self.via_b_orb & 0x02 != 0;
                let bc1  = self.via_b_orb & 0x01 != 0;
                match ay_mode(bdir, bc1) {
                    AyMode::LatchAddr => self.ay1_addr = val & 0x0F,
                    AyMode::WriteReg  => self.ay1.write_reg(self.ay1_addr, val),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// Step both AY chips one CPU cycle.
    pub fn step(&mut self) {
        self.ay0.step();
        self.ay1.step();
    }

    /// Drain mixed audio from both AY chips into `out`. Returns samples written.
    pub fn drain_samples(&mut self, out: &mut [f32]) -> usize {
        let n = out.len()
            .min(self.ay0.sample_buffer.len())
            .min(self.ay1.sample_buffer.len());
        for i in 0..n {
            out[i] = (self.ay0.sample_buffer[i] + self.ay1.sample_buffer[i]) * 0.5;
        }
        if n > 0 {
            self.ay0.sample_buffer.drain(..n);
            self.ay1.sample_buffer.drain(..n);
        }
        n
    }
}
