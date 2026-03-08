/// AY-3-8910 Programmable Sound Generator.
/// 3 tone channels (A, B, C), 1 noise generator, 1 envelope generator.
/// Used by the Mockingboard card (two chips per card).
pub struct Ay3_8910 {
    /// Registers $00-$0F
    pub(crate) regs: [u8; 16],

    /// Tone counters (12-bit, one per channel)
    tone_counter: [u16; 3],
    tone_state: [bool; 3],

    /// Noise
    noise_counter: u16,
    noise_state: u32, // 17-bit LFSR
    noise_output: bool,

    /// Envelope
    env_counter: u16,
    env_step: u8,
    env_attack: bool,
    env_hold: bool,
    env_alt: bool,
    env_cont: bool,
    pub(crate) env_vol: u8, // 0-15

    /// Audio output accumulation
    pub sample_buffer: Vec<f32>,
    sample_rate: f64,
    cpu_freq: f64,
    sample_acc: f64,
}

impl Ay3_8910 {
    pub fn new() -> Self {
        Self {
            regs: [0; 16],
            tone_counter: [0; 3],
            tone_state: [false; 3],
            noise_counter: 0,
            noise_state: 1, // LFSR must not be 0
            noise_output: false,
            env_counter: 0,
            env_step: 0,
            env_attack: false,
            env_hold: false,
            env_alt: false,
            env_cont: false,
            env_vol: 0,
            sample_buffer: Vec::with_capacity(1024),
            sample_rate: 44100.0,
            cpu_freq: 1_023_000.0, // Apple II CPU frequency
            sample_acc: 0.0,
        }
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.sample_rate = rate as f64;
    }

    pub fn write_reg(&mut self, reg: u8, val: u8) {
        let r = (reg & 0x0F) as usize;
        self.regs[r] = val;

        // Writing envelope shape register restarts the envelope
        if r == 13 {
            self.env_step = 0;
            self.env_counter = 0;
            let shape = val & 0x0F;
            self.env_attack = shape & 0x04 != 0;
            self.env_alt    = shape & 0x02 != 0;
            self.env_hold   = shape & 0x01 != 0;
            self.env_cont   = shape & 0x08 != 0;
            self.env_vol = if self.env_attack { 0 } else { 15 };
        }
    }

    pub fn read_reg(&self, reg: u8) -> u8 {
        self.regs[(reg & 0x0F) as usize]
    }

    /// Step one CPU cycle.
    pub fn step(&mut self) {
        // Tone channels: each tone half-period = tone_period * 8 CPU cycles
        for ch in 0..3 {
            let period = self.tone_period(ch);
            if period == 0 {
                continue;
            }
            if self.tone_counter[ch] == 0 {
                self.tone_counter[ch] = period;
                self.tone_state[ch] = !self.tone_state[ch];
            } else {
                self.tone_counter[ch] -= 1;
            }
        }

        // Noise: 5-bit period in regs[6], 17-bit Galois LFSR
        let noise_period = (self.regs[6] & 0x1F) as u16;
        if noise_period > 0 {
            if self.noise_counter == 0 {
                self.noise_counter = noise_period * 8;
                let bit = (self.noise_state ^ (self.noise_state >> 3)) & 1;
                self.noise_state = (self.noise_state >> 1) | (bit << 16);
                self.noise_output = self.noise_state & 1 != 0;
            } else {
                self.noise_counter -= 1;
            }
        }

        // Envelope: period from regs[11:12] (16-bit)
        let env_period = ((self.regs[12] as u16) << 8) | self.regs[11] as u16;
        if env_period > 0 {
            if self.env_counter == 0 {
                self.env_counter = env_period;
                self.clock_envelope();
            } else {
                self.env_counter -= 1;
            }
        }

        // Sample generation at target sample rate
        self.sample_acc += 1.0;
        if self.sample_acc >= self.cpu_freq / self.sample_rate {
            self.sample_acc -= self.cpu_freq / self.sample_rate;
            self.sample_buffer.push(self.mix());
        }
    }

    fn tone_period(&self, ch: usize) -> u16 {
        let lo = self.regs[ch * 2] as u16;
        let hi = (self.regs[ch * 2 + 1] & 0x0F) as u16;
        ((hi << 8) | lo) * 8
    }

    fn clock_envelope(&mut self) {
        self.env_step = self.env_step.wrapping_add(1);
        if self.env_step >= 16 {
            self.env_step = 0;
            if !self.env_cont {
                self.env_vol = if self.env_attack { 0 } else { 15 };
                return;
            } else if self.env_alt {
                self.env_attack = !self.env_attack;
            }
        }
        self.env_vol = if self.env_attack { self.env_step } else { 15 - self.env_step };
    }

    fn mix(&self) -> f32 {
        let mixer = self.regs[7];
        let mut out = 0.0f32;

        for ch in 0..3 {
            let tone_en  = mixer & (1 << ch) == 0;
            let noise_en = mixer & (1 << (ch + 3)) == 0;
            let active = (tone_en && self.tone_state[ch]) || (noise_en && self.noise_output);

            let amp_reg = self.regs[8 + ch];
            let vol = if amp_reg & 0x10 != 0 {
                self.env_vol
            } else {
                amp_reg & 0x0F
            };

            if active {
                out += (vol as f32) / 15.0 / 3.0; // Normalize across 3 channels
            }
        }
        out
    }
}
