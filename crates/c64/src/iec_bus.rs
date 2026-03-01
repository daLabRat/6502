/// IEC serial bus shared state.
///
/// The IEC bus is an open-collector bus with 3 signal lines:
/// - ATN (Attention): driven only by the C64
/// - CLK (Clock): driven by both C64 and drive
/// - DATA: driven by both C64 and drive
///
/// Open-collector logic: line is LOW if ANY device pulls it low.
/// Reading a line returns true (released/high) only if NO device pulls it low.
pub struct IecBus {
    // C64 side outputs (active low — true means pulling the line low)
    pub c64_atn: bool,
    pub c64_clk: bool,
    pub c64_data: bool,

    // Drive side outputs (active low — true means pulling the line low)
    pub drive_clk: bool,
    pub drive_data: bool,
}

impl IecBus {
    pub fn new() -> Self {
        Self {
            c64_atn: false,
            c64_clk: false,
            c64_data: false,
            drive_clk: false,
            drive_data: false,
        }
    }

    /// Read the ATN line state (active low: true = released/high, false = asserted/low).
    pub fn atn(&self) -> bool {
        !self.c64_atn
    }

    /// Read the CLK line state (active low: true = released/high, false = asserted/low).
    pub fn clk(&self) -> bool {
        !(self.c64_clk || self.drive_clk)
    }

    /// Read the DATA line state (active low: true = released/high, false = asserted/low).
    pub fn data(&self) -> bool {
        !(self.c64_data || self.drive_data)
    }

    /// Update C64 side from CIA2 Port A output.
    /// Bits: 3=ATN out, 4=CLK out, 5=DATA out (active high in register = pull line low).
    pub fn update_from_cia2(&mut self, port_a: u8) {
        self.c64_atn = port_a & 0x08 != 0;
        self.c64_clk = port_a & 0x10 != 0;
        self.c64_data = port_a & 0x20 != 0;
    }

    /// Read IEC bus state for CIA2 Port A input.
    /// Bit 6 = CLK in (active low), Bit 7 = DATA in (active low).
    /// Returns the bits to merge into CIA2 PA read.
    pub fn cia2_input_bits(&self) -> u8 {
        let clk_bit = if self.clk() { 0x40 } else { 0x00 };
        let data_bit = if self.data() { 0x80 } else { 0x00 };
        clk_bit | data_bit
    }

    /// Read IEC bus state for drive VIA1.
    /// VIA1 Port A reads: bit 7 = ATN in (directly from bus, active low).
    /// VIA1 Port B reads: bit 0 = DATA in, bit 2 = CLK in (active low sense).
    pub fn drive_via1_port_a_input(&self) -> u8 {
        if self.atn() { 0x00 } else { 0x80 }
    }

    pub fn drive_via1_port_b_input(&self) -> u8 {
        let data_in = if self.data() { 0x00 } else { 0x01 };
        let clk_in = if self.clk() { 0x00 } else { 0x04 };
        data_in | clk_in
    }
}
