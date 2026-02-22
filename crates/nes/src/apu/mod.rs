pub mod pulse;
pub mod triangle;
pub mod noise;
pub mod dmc;

use emu_common::AudioSample;

/// NES APU (Audio Processing Unit).
pub struct Apu {
    pub pulse1: pulse::Pulse,
    pub pulse2: pulse::Pulse,
    pub triangle: triangle::Triangle,
    pub noise: noise::Noise,
    pub dmc: dmc::Dmc,

    // Frame counter
    frame_counter_mode: u8, // 0 = 4-step, 1 = 5-step
    frame_counter: u32,
    frame_irq_inhibit: bool,
    frame_irq_pending: bool,

    // Status
    enabled: [bool; 5],

    // Audio output buffer
    pub(crate) sample_buffer: Vec<AudioSample>,
    cpu_cycles: u64,
    _sample_rate: f64,
    cycles_per_sample: f64,
    sample_accumulator: f64,
}

impl Apu {
    pub fn new() -> Self {
        let sample_rate = emu_common::SAMPLE_RATE as f64;
        let cpu_freq = 1_789_773.0; // NTSC CPU frequency
        Self {
            pulse1: pulse::Pulse::new(true),
            pulse2: pulse::Pulse::new(false),
            triangle: triangle::Triangle::new(),
            noise: noise::Noise::new(),
            dmc: dmc::Dmc::new(),
            frame_counter_mode: 0,
            frame_counter: 0,
            frame_irq_inhibit: false,
            frame_irq_pending: false,
            enabled: [false; 5],
            sample_buffer: Vec::with_capacity(1024),
            cpu_cycles: 0,
            _sample_rate: sample_rate,
            cycles_per_sample: cpu_freq / sample_rate,
            sample_accumulator: 0.0,
        }
    }

    /// Update the output sample rate (called when audio device is initialized).
    pub fn set_sample_rate(&mut self, rate: u32) {
        let cpu_freq = 1_789_773.0;
        self.cycles_per_sample = cpu_freq / rate as f64;
    }

    /// Write to an APU register ($4000-$4017).
    pub fn write_register(&mut self, addr: u16, val: u8) {
        match addr {
            0x4000..=0x4003 => self.pulse1.write_register(addr - 0x4000, val),
            0x4004..=0x4007 => self.pulse2.write_register(addr - 0x4004, val),
            0x4008..=0x400B => self.triangle.write_register(addr - 0x4008, val),
            0x400C..=0x400F => self.noise.write_register(addr - 0x400C, val),
            0x4010..=0x4013 => self.dmc.write_register(addr - 0x4010, val),
            0x4015 => {
                // Status register
                self.enabled[0] = val & 0x01 != 0;
                self.enabled[1] = val & 0x02 != 0;
                self.enabled[2] = val & 0x04 != 0;
                self.enabled[3] = val & 0x08 != 0;
                self.enabled[4] = val & 0x10 != 0;

                if !self.enabled[0] { self.pulse1.length_counter = 0; }
                if !self.enabled[1] { self.pulse2.length_counter = 0; }
                if !self.enabled[2] { self.triangle.length_counter = 0; }
                if !self.enabled[3] { self.noise.length_counter = 0; }
                // DMC flag handling omitted for now
                self.frame_irq_pending = false;
            }
            0x4017 => {
                // Frame counter
                self.frame_counter_mode = (val >> 7) & 1;
                self.frame_irq_inhibit = val & 0x40 != 0;
                if self.frame_irq_inhibit {
                    self.frame_irq_pending = false;
                }
                self.frame_counter = 0;
                if self.frame_counter_mode == 1 {
                    self.clock_half_frame();
                    self.clock_quarter_frame();
                }
            }
            _ => {}
        }
    }

    /// Read the status register ($4015).
    pub fn read_status(&mut self) -> u8 {
        let mut val = 0u8;
        if self.pulse1.length_counter > 0 { val |= 0x01; }
        if self.pulse2.length_counter > 0 { val |= 0x02; }
        if self.triangle.length_counter > 0 { val |= 0x04; }
        if self.noise.length_counter > 0 { val |= 0x08; }
        if self.frame_irq_pending { val |= 0x40; }
        self.frame_irq_pending = false;
        val
    }

    /// Step the APU by one CPU cycle.
    pub fn step(&mut self) {
        self.cpu_cycles += 1;

        // Triangle ticks at CPU rate
        self.triangle.tick_timer();

        // Other channels tick at half CPU rate
        if self.cpu_cycles % 2 == 0 {
            self.pulse1.tick_timer();
            self.pulse2.tick_timer();
            self.noise.tick_timer();
        }

        // Frame counter (every ~7457 CPU cycles for 240Hz)
        self.frame_counter += 1;
        match self.frame_counter_mode {
            0 => {
                // 4-step sequence
                match self.frame_counter {
                    3729 => self.clock_quarter_frame(),
                    7457 => {
                        self.clock_quarter_frame();
                        self.clock_half_frame();
                    }
                    11186 => self.clock_quarter_frame(),
                    14915 => {
                        self.clock_quarter_frame();
                        self.clock_half_frame();
                        if !self.frame_irq_inhibit {
                            self.frame_irq_pending = true;
                        }
                        self.frame_counter = 0;
                    }
                    _ => {}
                }
            }
            1 => {
                // 5-step sequence
                match self.frame_counter {
                    3729 => self.clock_quarter_frame(),
                    7457 => {
                        self.clock_quarter_frame();
                        self.clock_half_frame();
                    }
                    11186 => self.clock_quarter_frame(),
                    18641 => {
                        self.clock_quarter_frame();
                        self.clock_half_frame();
                        self.frame_counter = 0;
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        // Generate audio sample
        self.sample_accumulator += 1.0;
        if self.sample_accumulator >= self.cycles_per_sample {
            self.sample_accumulator -= self.cycles_per_sample;
            let sample = self.mix();
            self.sample_buffer.push(sample);
        }
    }

    fn clock_quarter_frame(&mut self) {
        self.pulse1.clock_envelope();
        self.pulse2.clock_envelope();
        self.triangle.clock_linear_counter();
        self.noise.clock_envelope();
    }

    fn clock_half_frame(&mut self) {
        self.pulse1.clock_length_counter();
        self.pulse1.clock_sweep();
        self.pulse2.clock_length_counter();
        self.pulse2.clock_sweep();
        self.triangle.clock_length_counter();
        self.noise.clock_length_counter();
    }

    /// Mix all channels into a single sample.
    fn mix(&self) -> AudioSample {
        let p1 = if self.enabled[0] { self.pulse1.output() as f32 } else { 0.0 };
        let p2 = if self.enabled[1] { self.pulse2.output() as f32 } else { 0.0 };
        let tri = if self.enabled[2] { self.triangle.output() as f32 } else { 0.0 };
        let noise = if self.enabled[3] { self.noise.output() as f32 } else { 0.0 };
        let _dmc = if self.enabled[4] { self.dmc.output() as f32 } else { 0.0 };

        // Linear approximation mixing
        let pulse_out = if p1 + p2 > 0.0 {
            95.88 / ((8128.0 / (p1 + p2)) + 100.0)
        } else {
            0.0
        };

        let tnd_out = if tri + noise > 0.0 {
            159.79 / ((1.0 / (tri / 8227.0 + noise / 12241.0)) + 100.0)
        } else {
            0.0
        };

        pulse_out + tnd_out
    }

    /// Drain audio samples into the provided buffer.
    pub fn drain_samples(&mut self, out: &mut [AudioSample]) -> usize {
        let count = out.len().min(self.sample_buffer.len());
        out[..count].copy_from_slice(&self.sample_buffer[..count]);
        self.sample_buffer.drain(..count);
        count
    }

    /// Check if the APU is asserting IRQ.
    pub fn irq_pending(&self) -> bool {
        self.frame_irq_pending
    }
}

/// Length counter lookup table.
pub static LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14,
    12, 16, 24, 18, 48, 20, 96, 22, 192, 24, 72, 26, 16, 28, 32, 30,
];
