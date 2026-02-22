/// DMC (Delta Modulation Channel) - stub implementation.
/// Full implementation requires bus access for DMA reads.
pub struct Dmc {
    output_level: u8,
}

impl Dmc {
    pub fn new() -> Self {
        Self { output_level: 0 }
    }

    pub fn write_register(&mut self, reg: u16, val: u8) {
        match reg {
            1 => self.output_level = val & 0x7F,
            _ => {} // TODO: full DMC implementation
        }
    }

    pub fn output(&self) -> u8 {
        self.output_level
    }
}
