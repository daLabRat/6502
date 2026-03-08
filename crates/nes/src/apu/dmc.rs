/// DMC (Delta Modulation Channel) - Sample playback via 1-bit delta encoding.
/// Reads sample bytes from memory via DMA, shifts out bits to adjust output level.
pub struct Dmc {
    // Timer
    timer_period: u16,
    timer_counter: u16,

    // Output
    output_level: u8, // 0-127

    // Sample
    sample_addr: u16,    // $C000-$FFFF (set via $4012)
    sample_length: u16,  // 1-4081 bytes (set via $4013)
    current_addr: u16,   // current read position
    bytes_remaining: u16,

    // Shift register
    shift_register: u8,
    bits_remaining: u8,
    sample_buffer: Option<u8>,
    silence_flag: bool,

    // Flags
    pub(crate) irq_enabled: bool,
    pub(crate) loop_flag: bool,
    pub(crate) irq_pending: bool,

    // DMA interface: when set, bus should read this address and call receive_dma_byte
    pub(crate) dma_request: Option<u16>,
}

/// NTSC rate table: timer period indexed by rate index (0-15).
static RATE_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214,
    190, 160, 142, 128, 106,  84,  72,  54,
];

impl Dmc {
    pub fn new() -> Self {
        Self {
            timer_period: RATE_TABLE[0],
            timer_counter: RATE_TABLE[0],
            output_level: 0,
            sample_addr: 0xC000,
            sample_length: 1,
            current_addr: 0xC000,
            bytes_remaining: 0,
            shift_register: 0,
            bits_remaining: 0,
            sample_buffer: None,
            silence_flag: true,
            irq_enabled: false,
            loop_flag: false,
            irq_pending: false,
            dma_request: None,
        }
    }

    pub fn write_register(&mut self, reg: u16, val: u8) {
        match reg {
            // $4010: Flags and rate
            0 => {
                self.irq_enabled = val & 0x80 != 0;
                self.loop_flag = val & 0x40 != 0;
                self.timer_period = RATE_TABLE[(val & 0x0F) as usize];
                if !self.irq_enabled {
                    self.irq_pending = false;
                }
            }
            // $4011: Direct load (7-bit)
            1 => {
                self.output_level = val & 0x7F;
            }
            // $4012: Sample address
            2 => {
                // Address = %11AAAAAA.AA000000 = $C000 + A * 64
                self.sample_addr = 0xC000 + (val as u16) * 64;
            }
            // $4013: Sample length
            3 => {
                // Length = %LLLL.LLLL0001 = L * 16 + 1
                self.sample_length = (val as u16) * 16 + 1;
            }
            _ => {}
        }
    }

    /// Called when writing $4015 with bit 4.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.irq_pending = false;
        if !enabled {
            self.bytes_remaining = 0;
        } else if self.bytes_remaining == 0 {
            self.restart();
        }
    }

    /// Restart sample playback from the beginning.
    fn restart(&mut self) {
        self.current_addr = self.sample_addr;
        self.bytes_remaining = self.sample_length;
    }

    /// Tick the DMC timer. Called once per CPU cycle.
    pub fn tick(&mut self) {
        // Request DMA if sample buffer is empty and bytes remain
        if self.sample_buffer.is_none() && self.bytes_remaining > 0 && self.dma_request.is_none() {
            self.dma_request = Some(self.current_addr);
        }

        self.timer_counter = self.timer_counter.wrapping_sub(1);
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            self.clock_output();
        }
    }

    /// Clock the output unit (shift register).
    fn clock_output(&mut self) {
        if self.bits_remaining == 0 {
            // Load shift register from sample buffer
            if let Some(byte) = self.sample_buffer.take() {
                self.shift_register = byte;
                self.silence_flag = false;
            } else {
                self.silence_flag = true;
            }
            self.bits_remaining = 8;
        }

        if !self.silence_flag {
            // Bit 0: 1 = increment, 0 = decrement (by 2)
            if self.shift_register & 1 != 0 {
                if self.output_level <= 125 {
                    self.output_level += 2;
                }
            } else if self.output_level >= 2 {
                self.output_level -= 2;
            }
        }

        self.shift_register >>= 1;
        self.bits_remaining -= 1;
    }

    /// Receive a byte from DMA read. Called by bus after servicing dma_request.
    pub fn receive_dma_byte(&mut self, byte: u8) {
        self.sample_buffer = Some(byte);

        // Advance address (wraps around $FFFF → $8000)
        self.current_addr = if self.current_addr == 0xFFFF {
            0x8000
        } else {
            self.current_addr + 1
        };

        self.bytes_remaining -= 1;
        if self.bytes_remaining == 0 {
            if self.loop_flag {
                self.restart();
            } else if self.irq_enabled {
                self.irq_pending = true;
            }
        }
    }

    pub fn output(&self) -> u8 {
        self.output_level
    }

    pub fn bytes_remaining(&self) -> u16 {
        self.bytes_remaining
    }

    pub fn snapshot(&self) -> crate::snapshot::DmcSnapshot {
        crate::snapshot::DmcSnapshot {
            timer_period: self.timer_period,
            timer_counter: self.timer_counter,
            output_level: self.output_level,
            sample_addr: self.sample_addr,
            sample_length: self.sample_length,
            current_addr: self.current_addr,
            bytes_remaining: self.bytes_remaining,
            shift_register: self.shift_register,
            bits_remaining: self.bits_remaining,
            sample_buffer: self.sample_buffer,
            silence_flag: self.silence_flag,
            irq_enabled: self.irq_enabled,
            loop_flag: self.loop_flag,
            irq_pending: self.irq_pending,
        }
    }

    pub fn restore(&mut self, s: &crate::snapshot::DmcSnapshot) {
        self.timer_period = s.timer_period;
        self.timer_counter = s.timer_counter;
        self.output_level = s.output_level;
        self.sample_addr = s.sample_addr;
        self.sample_length = s.sample_length;
        self.current_addr = s.current_addr;
        self.bytes_remaining = s.bytes_remaining;
        self.shift_register = s.shift_register;
        self.bits_remaining = s.bits_remaining;
        self.sample_buffer = s.sample_buffer;
        self.silence_flag = s.silence_flag;
        self.irq_enabled = s.irq_enabled;
        self.loop_flag = s.loop_flag;
        self.irq_pending = s.irq_pending;
        self.dma_request = None;
    }
}
