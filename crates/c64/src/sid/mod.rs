use emu_common::AudioSample;

/// SID (Sound Interface Device) - 3-voice synthesizer.
/// Simplified implementation: oscillators + ADSR + basic filter.
pub struct Sid {
    voices: [Voice; 3],
    filter_cutoff: u16,
    filter_resonance: u8,
    filter_mode: u8,
    volume: u8,

    sample_buffer: Vec<AudioSample>,
    _cycle_count: u64,
    cycles_per_sample: f64,
    sample_accumulator: f64,
}

struct Voice {
    frequency: u16,
    pulse_width: u16,
    waveform: u8,
    // ADSR
    attack: u8,
    decay: u8,
    sustain: u8,
    release: u8,
    gate: bool,
    // Internal
    accumulator: u32,
    envelope: u8,
    envelope_state: EnvelopeState,
    envelope_counter: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum EnvelopeState {
    Attack,
    Decay,
    Sustain,
    Release,
}

impl Voice {
    fn new() -> Self {
        Self {
            frequency: 0,
            pulse_width: 0,
            waveform: 0,
            attack: 0,
            decay: 0,
            sustain: 0,
            release: 0,
            gate: false,
            accumulator: 0,
            envelope: 0,
            envelope_state: EnvelopeState::Release,
            envelope_counter: 0,
        }
    }

    fn tick(&mut self) {
        self.accumulator = self.accumulator.wrapping_add(self.frequency as u32);

        // Simple envelope
        self.envelope_counter += 1;
        match self.envelope_state {
            EnvelopeState::Attack => {
                let rate = ATTACK_RATES[self.attack as usize];
                if self.envelope_counter >= rate {
                    self.envelope_counter = 0;
                    if self.envelope < 255 {
                        self.envelope += 1;
                    } else {
                        self.envelope_state = EnvelopeState::Decay;
                    }
                }
            }
            EnvelopeState::Decay => {
                let rate = DECAY_RATES[self.decay as usize];
                let sustain_level = self.sustain << 4 | self.sustain;
                if self.envelope_counter >= rate {
                    self.envelope_counter = 0;
                    if self.envelope > sustain_level {
                        self.envelope -= 1;
                    } else {
                        self.envelope_state = EnvelopeState::Sustain;
                    }
                }
            }
            EnvelopeState::Sustain => {
                // Hold at sustain level
            }
            EnvelopeState::Release => {
                let rate = DECAY_RATES[self.release as usize];
                if self.envelope_counter >= rate {
                    self.envelope_counter = 0;
                    if self.envelope > 0 {
                        self.envelope -= 1;
                    }
                }
            }
        }
    }

    fn output(&self) -> i16 {
        let osc = match self.waveform & 0xF0 {
            0x10 => {
                // Triangle
                let phase = (self.accumulator >> 16) as i16;
                if phase < 0 { !phase * 2 } else { phase * 2 }
            }
            0x20 => {
                // Sawtooth
                ((self.accumulator >> 16) as i16).wrapping_sub(0x4000)
            }
            0x40 => {
                // Pulse
                let phase = (self.accumulator >> 8) & 0xFFF;
                if phase < self.pulse_width as u32 { 0x7FFF } else { -0x7FFF_i16 }
            }
            0x80 => {
                // Noise (simplified)
                let bit = ((self.accumulator >> 22) ^ (self.accumulator >> 17)) as i16;
                (bit & 1) * 0x7FFF - 0x4000
            }
            _ => 0,
        };

        ((osc as i32 * self.envelope as i32) >> 8) as i16
    }
}

// Approximate envelope rate tables (in cycles)
static ATTACK_RATES: [u32; 16] = [
    2, 8, 16, 24, 38, 56, 68, 80, 100, 250, 500, 800, 1000, 3000, 5000, 8000,
];
static DECAY_RATES: [u32; 16] = [
    6, 24, 48, 72, 114, 168, 204, 240, 300, 750, 1500, 2400, 3000, 9000, 15000, 24000,
];

impl Sid {
    pub fn new() -> Self {
        let cpu_freq = 985_248.0; // PAL
        Self {
            voices: [Voice::new(), Voice::new(), Voice::new()],
            filter_cutoff: 0,
            filter_resonance: 0,
            filter_mode: 0,
            volume: 0,
            sample_buffer: Vec::with_capacity(1024),
            _cycle_count: 0,
            cycles_per_sample: cpu_freq / emu_common::SAMPLE_RATE as f64,
            sample_accumulator: 0.0,
        }
    }

    /// Update the output sample rate.
    pub fn set_sample_rate(&mut self, rate: u32) {
        let cpu_freq = 985_248.0;
        self.cycles_per_sample = cpu_freq / rate as f64;
    }

    pub fn write_register(&mut self, addr: u16, val: u8) {
        let reg = (addr & 0x1F) as usize;
        let voice_idx = reg / 7;

        if voice_idx < 3 && reg % 7 < 7 {
            let voice = &mut self.voices[voice_idx];
            match reg % 7 {
                0 => voice.frequency = (voice.frequency & 0xFF00) | val as u16,
                1 => voice.frequency = (voice.frequency & 0x00FF) | ((val as u16) << 8),
                2 => voice.pulse_width = (voice.pulse_width & 0x0F00) | val as u16,
                3 => voice.pulse_width = (voice.pulse_width & 0x00FF) | (((val as u16) & 0x0F) << 8),
                4 => {
                    let new_gate = val & 0x01 != 0;
                    if new_gate && !voice.gate {
                        voice.envelope_state = EnvelopeState::Attack;
                        voice.envelope_counter = 0;
                    } else if !new_gate && voice.gate {
                        voice.envelope_state = EnvelopeState::Release;
                    }
                    voice.gate = new_gate;
                    voice.waveform = val;
                }
                5 => {
                    voice.attack = (val >> 4) & 0x0F;
                    voice.decay = val & 0x0F;
                }
                6 => {
                    voice.sustain = (val >> 4) & 0x0F;
                    voice.release = val & 0x0F;
                }
                _ => {}
            }
        } else {
            match reg {
                0x15 => self.filter_cutoff = (self.filter_cutoff & 0x7F8) | (val & 0x07) as u16,
                0x16 => self.filter_cutoff = (self.filter_cutoff & 0x07) | ((val as u16) << 3),
                0x17 => self.filter_resonance = val,
                0x18 => {
                    self.volume = val & 0x0F;
                    self.filter_mode = val >> 4;
                }
                _ => {}
            }
        }
    }

    pub fn read_register(&self, addr: u16) -> u8 {
        // Most SID registers are write-only
        let reg = (addr & 0x1F) as usize;
        match reg {
            0x1B => (self.voices[2].accumulator >> 16) as u8, // OSC3/Random
            0x1C => self.voices[2].envelope,                   // ENV3
            _ => 0,
        }
    }

    pub fn step(&mut self) {
        for voice in &mut self.voices {
            voice.tick();
        }

        self.sample_accumulator += 1.0;
        if self.sample_accumulator >= self.cycles_per_sample {
            self.sample_accumulator -= self.cycles_per_sample;

            let mut output = 0i32;
            for voice in &self.voices {
                output += voice.output() as i32;
            }

            output = (output * self.volume as i32) / 15;
            let sample = (output as f32) / 32768.0;
            self.sample_buffer.push(sample);
        }
    }

    pub fn drain_samples(&mut self, out: &mut [AudioSample]) -> usize {
        let count = out.len().min(self.sample_buffer.len());
        out[..count].copy_from_slice(&self.sample_buffer[..count]);
        self.sample_buffer.drain(..count);
        count
    }
}
