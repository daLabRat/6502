use emu_common::AudioSample;
use crate::snapshot::{SidSnapshot, VoiceSnapshot};

/// SID (Sound Interface Device) - 3-voice synthesizer.
/// Implements oscillators with combined waveforms, ring modulation,
/// hard sync, proper noise LFSR, ADSR envelopes, and state-variable filter.
pub struct Sid {
    voices: [Voice; 3],
    // Filter state
    filter_cutoff: u16,     // 11-bit cutoff ($D415-$D416)
    filter_resonance: u8,   // High nibble of $D417
    filter_mode: u8,        // Bits 4-6 of $D418: LP/BP/HP
    filter_routing: u8,     // Bits 0-2 of $D417: which voices routed through filter
    voice3_off: bool,       // Bit 7 of $D418: disconnect voice 3 from output
    volume: u8,

    // Filter internal state (state-variable)
    filter_bp: f32,
    filter_lp: f32,

    sample_buffer: Vec<AudioSample>,
    cycles_per_sample: f64,
    sample_accumulator: f64,
}

struct Voice {
    frequency: u16,
    pulse_width: u16,
    control: u8,     // Waveform + gate + sync + ring mod + test
    // ADSR
    attack: u8,
    decay: u8,
    sustain: u8,
    release: u8,
    gate: bool,
    // Internal
    accumulator: u32,    // 24-bit phase accumulator
    prev_msb: bool,      // Previous MSB for hard sync detection
    noise_lfsr: u32,     // 23-bit Galois LFSR for noise
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
            control: 0,
            attack: 0,
            decay: 0,
            sustain: 0,
            release: 0,
            gate: false,
            accumulator: 0,
            prev_msb: false,
            noise_lfsr: 0x7FFFFF, // 23-bit LFSR initialized to all 1s
            envelope: 0,
            envelope_state: EnvelopeState::Release,
            envelope_counter: 0,
        }
    }

    /// Advance the oscillator by one clock cycle.
    fn tick(&mut self, sync_source_overflow: bool) {
        let old_msb = self.accumulator & 0x800000 != 0;

        // Hard sync: reset accumulator when sync source overflows
        if self.control & 0x02 != 0 && sync_source_overflow {
            self.accumulator = 0;
        }

        self.accumulator = (self.accumulator + self.frequency as u32) & 0xFFFFFF;

        // Detect MSB transition (for hard sync of the next voice)
        self.prev_msb = old_msb;

        // Clock noise LFSR when accumulator bit 19 transitions
        let new_bit19 = self.accumulator & 0x080000 != 0;
        let old_bit19 = self.accumulator.wrapping_sub(self.frequency as u32) & 0x080000 != 0;
        if new_bit19 && !old_bit19 {
            // 23-bit Galois LFSR: polynomial x^23 + x^18 + 1
            let feedback = self.noise_lfsr & 1;
            self.noise_lfsr >>= 1;
            if feedback != 0 {
                self.noise_lfsr ^= 0x440000; // bits 22 and 18
            }
        }

        // Envelope
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
            EnvelopeState::Sustain => {}
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

    /// Did the accumulator MSB transition from 1→0 this cycle? (for hard sync)
    fn msb_overflow(&self) -> bool {
        self.prev_msb && (self.accumulator & 0x800000 == 0)
    }

    /// Compute the raw waveform output (12-bit, 0-4095).
    fn waveform_output(&self, ring_mod_source_msb: bool) -> u16 {
        let wf = self.control & 0xF0;
        if wf == 0 {
            return 0;
        }

        // Individual waveform outputs (each 12-bit)
        let phase = (self.accumulator >> 12) as u16; // top 12 bits

        let tri = {
            let msb = if self.control & 0x04 != 0 {
                // Ring modulation: XOR with source voice MSB
                (self.accumulator >> 23 != 0) ^ ring_mod_source_msb
            } else {
                self.accumulator >> 23 != 0
            };
            let val = (self.accumulator >> 11) as u16 & 0xFFF;
            if msb { val ^ 0xFFF } else { val }
        };

        let saw = phase;

        let pulse = {
            let pw = (self.pulse_width as u32) << 12; // scale to 24-bit
            if self.accumulator >= pw { 0xFFF } else { 0 }
        };

        let noise = {
            // Extract bits from LFSR to form 12-bit output
            let lfsr = self.noise_lfsr;
            let b = |pos: u32| -> u16 { ((lfsr >> pos) & 1) as u16 };
            (b(22) << 11) | (b(20) << 10) | (b(16) << 9) | (b(13) << 8)
            | (b(11) << 7) | (b(7) << 6) | (b(4) << 5) | (b(2) << 4)
        };

        // Combined waveforms: AND individual outputs together
        let mut result = 0xFFF_u16;
        if wf & 0x10 != 0 { result &= tri; }
        if wf & 0x20 != 0 { result &= saw; }
        if wf & 0x40 != 0 { result &= pulse; }
        if wf & 0x80 != 0 { result &= noise; }

        result
    }

    /// Final voice output (signed, envelope-scaled).
    fn output(&self, ring_mod_source_msb: bool) -> i16 {
        let wf = self.waveform_output(ring_mod_source_msb) as i32 - 0x800; // center around 0
        ((wf * self.envelope as i32) >> 8) as i16
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
            filter_routing: 0,
            voice3_off: false,
            volume: 0,
            filter_bp: 0.0,
            filter_lp: 0.0,
            sample_buffer: Vec::with_capacity(1024),
            cycles_per_sample: cpu_freq / emu_common::SAMPLE_RATE as f64,
            sample_accumulator: 0.0,
        }
    }

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
                    voice.control = val;
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
                0x17 => {
                    self.filter_resonance = (val >> 4) & 0x0F;
                    self.filter_routing = val & 0x07;
                }
                0x18 => {
                    self.volume = val & 0x0F;
                    self.filter_mode = (val >> 4) & 0x07;
                    self.voice3_off = val & 0x80 != 0;
                }
                _ => {}
            }
        }
    }

    pub fn read_register(&self, addr: u16) -> u8 {
        let reg = (addr & 0x1F) as usize;
        match reg {
            0x1B => (self.voices[2].accumulator >> 16) as u8,
            0x1C => self.voices[2].envelope,
            _ => 0,
        }
    }

    pub fn step(&mut self) {
        // Detect overflow for hard sync (voice N syncs from voice N-1, wrapping)
        let overflow0 = self.voices[2].msb_overflow(); // voice 0 syncs from voice 2
        let overflow1 = self.voices[0].msb_overflow();
        let overflow2 = self.voices[1].msb_overflow();

        self.voices[0].tick(overflow0);
        self.voices[1].tick(overflow1);
        self.voices[2].tick(overflow2);

        self.sample_accumulator += 1.0;
        if self.sample_accumulator >= self.cycles_per_sample {
            self.sample_accumulator -= self.cycles_per_sample;

            // Get ring mod source MSBs (voice N uses voice N-1's accumulator MSB)
            let ring_msb0 = self.voices[2].accumulator & 0x800000 != 0;
            let ring_msb1 = self.voices[0].accumulator & 0x800000 != 0;
            let ring_msb2 = self.voices[1].accumulator & 0x800000 != 0;

            let v0 = self.voices[0].output(ring_msb0) as f32;
            let v1 = self.voices[1].output(ring_msb1) as f32;
            let v2 = self.voices[2].output(ring_msb2) as f32;

            // Route voices through filter or direct
            let mut filtered = 0.0f32;
            let mut direct = 0.0f32;

            if self.filter_routing & 0x01 != 0 { filtered += v0; } else { direct += v0; }
            if self.filter_routing & 0x02 != 0 { filtered += v1; } else { direct += v1; }
            if self.filter_routing & 0x04 != 0 {
                filtered += v2;
            } else if !self.voice3_off {
                direct += v2;
            }

            // State-variable filter (12 dB/octave)
            let cutoff_freq = self.compute_cutoff();
            let resonance = 1.0 - (self.filter_resonance as f32 / 17.0); // Q factor

            // Two integrators (simplified state-variable filter)
            let hp = filtered - self.filter_lp - resonance * self.filter_bp;
            self.filter_bp += cutoff_freq * hp;
            self.filter_lp += cutoff_freq * self.filter_bp;

            // Mix filter outputs based on mode
            let mut filter_out = 0.0f32;
            if self.filter_mode & 0x01 != 0 { filter_out += self.filter_lp; } // Lowpass
            if self.filter_mode & 0x02 != 0 { filter_out += self.filter_bp; } // Bandpass
            if self.filter_mode & 0x04 != 0 { filter_out += hp; }             // Highpass

            let output = (direct + filter_out) * (self.volume as f32 / 15.0);
            let sample = output / 32768.0;
            self.sample_buffer.push(sample.clamp(-1.0, 1.0));
        }
    }

    /// Compute the normalized cutoff frequency for the state-variable filter.
    fn compute_cutoff(&self) -> f32 {
        // Map 11-bit cutoff register to frequency (~30Hz to ~12kHz)
        // Using a simplified mapping: f = cutoff * base_freq_step
        let fc = self.filter_cutoff as f32;
        // Approximate: cutoff ranges from ~0 to ~2047
        // At sample rate ~985kHz, we need small coefficients
        let freq = (fc * 5.8) + 30.0; // ~30Hz to ~11900Hz
        // Convert to normalized frequency: 2 * sin(pi * f / fs)
        let fs = 985248.0; // PAL clock
        let w = (std::f32::consts::PI * freq / fs).sin() * 2.0;
        w.clamp(0.0, 0.9)
    }

    pub fn snapshot(&self) -> SidSnapshot {
        let vs = |v: &Voice| VoiceSnapshot {
            frequency: v.frequency,
            pulse_width: v.pulse_width,
            control: v.control,
            attack: v.attack,
            decay: v.decay,
            sustain: v.sustain,
            release: v.release,
            gate: v.gate,
            accumulator: v.accumulator,
            prev_msb: v.prev_msb,
            noise_lfsr: v.noise_lfsr,
            envelope: v.envelope,
            envelope_state: match v.envelope_state {
                EnvelopeState::Attack  => 0,
                EnvelopeState::Decay   => 1,
                EnvelopeState::Sustain => 2,
                EnvelopeState::Release => 3,
            },
            envelope_counter: v.envelope_counter,
        };
        SidSnapshot {
            voices: [vs(&self.voices[0]), vs(&self.voices[1]), vs(&self.voices[2])],
            filter_cutoff: self.filter_cutoff,
            filter_resonance: self.filter_resonance,
            filter_mode: self.filter_mode,
            filter_routing: self.filter_routing,
            voice3_off: self.voice3_off,
            volume: self.volume,
            filter_bp: self.filter_bp,
            filter_lp: self.filter_lp,
        }
    }

    pub fn restore(&mut self, s: &SidSnapshot) {
        let rv = |v: &mut Voice, sv: &VoiceSnapshot| {
            v.frequency = sv.frequency;
            v.pulse_width = sv.pulse_width;
            v.control = sv.control;
            v.attack = sv.attack;
            v.decay = sv.decay;
            v.sustain = sv.sustain;
            v.release = sv.release;
            v.gate = sv.gate;
            v.accumulator = sv.accumulator;
            v.prev_msb = sv.prev_msb;
            v.noise_lfsr = sv.noise_lfsr;
            v.envelope = sv.envelope;
            v.envelope_state = match sv.envelope_state {
                0 => EnvelopeState::Attack,
                1 => EnvelopeState::Decay,
                2 => EnvelopeState::Sustain,
                _ => EnvelopeState::Release,
            };
            v.envelope_counter = sv.envelope_counter;
        };
        rv(&mut self.voices[0], &s.voices[0]);
        rv(&mut self.voices[1], &s.voices[1]);
        rv(&mut self.voices[2], &s.voices[2]);
        self.filter_cutoff = s.filter_cutoff;
        self.filter_resonance = s.filter_resonance;
        self.filter_mode = s.filter_mode;
        self.filter_routing = s.filter_routing;
        self.voice3_off = s.voice3_off;
        self.volume = s.volume;
        self.filter_bp = s.filter_bp;
        self.filter_lp = s.filter_lp;
        if !self.filter_bp.is_finite() { self.filter_bp = 0.0; }
        if !self.filter_lp.is_finite() { self.filter_lp = 0.0; }
    }

    pub fn drain_samples(&mut self, out: &mut [AudioSample]) -> usize {
        let count = out.len().min(self.sample_buffer.len());
        out[..count].copy_from_slice(&self.sample_buffer[..count]);
        self.sample_buffer.drain(..count);
        count
    }

    /// Per-voice debug snapshot: (frequency_hz, pulse_width, waveform_name, env_level, state_name).
    pub fn voice_debug(&self) -> [(f64, u16, &'static str, u8, &'static str); 3] {
        self.voices.iter().map(|v| {
            let freq_hz = v.frequency as f64 * 985248.0 / 16_777_216.0;
            let wave = match (v.control >> 4) & 0xF {
                0x1 => "TRI",
                0x2 => "SAW",
                0x4 => "PUL",
                0x8 => "NOI",
                0x3 => "T+S",
                0x6 => "S+P",
                _   => "OFF",
            };
            let state = match v.envelope_state {
                EnvelopeState::Attack  => "ATK",
                EnvelopeState::Decay   => "DEC",
                EnvelopeState::Sustain => "SUS",
                EnvelopeState::Release => "REL",
            };
            (freq_hz, v.pulse_width, wave, v.envelope, state)
        }).collect::<Vec<_>>().try_into().unwrap()
    }
}
