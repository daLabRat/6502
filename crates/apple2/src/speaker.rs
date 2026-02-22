use emu_common::AudioSample;

/// Apple II speaker - 1-bit toggle audio.
/// Accessing $C030 toggles the speaker state.
pub struct Speaker {
    state: bool,
    sample_buffer: Vec<AudioSample>,
    cycle_count: u64,
    cycles_per_sample: f64,
    sample_accumulator: f64,
}

impl Speaker {
    pub fn new() -> Self {
        let cpu_freq = 1_023_000.0; // Apple II CPU frequency
        let sample_rate = emu_common::SAMPLE_RATE as f64;
        Self {
            state: false,
            sample_buffer: Vec::with_capacity(1024),
            cycle_count: 0,
            cycles_per_sample: cpu_freq / sample_rate,
            sample_accumulator: 0.0,
        }
    }

    /// Update the output sample rate.
    pub fn set_sample_rate(&mut self, rate: u32) {
        let cpu_freq = 1_023_000.0;
        self.cycles_per_sample = cpu_freq / rate as f64;
    }

    /// Toggle the speaker (called on $C030 access).
    pub fn toggle(&mut self) {
        self.state = !self.state;
    }

    /// Step the speaker by one CPU cycle.
    pub fn step(&mut self) {
        self.cycle_count += 1;
        self.sample_accumulator += 1.0;

        if self.sample_accumulator >= self.cycles_per_sample {
            self.sample_accumulator -= self.cycles_per_sample;
            let sample = if self.state { 0.5 } else { -0.5 };
            self.sample_buffer.push(sample);
        }
    }

    /// Drain audio samples into the provided buffer.
    pub fn drain_samples(&mut self, out: &mut [AudioSample]) -> usize {
        let count = out.len().min(self.sample_buffer.len());
        out[..count].copy_from_slice(&self.sample_buffer[..count]);
        self.sample_buffer.drain(..count);
        count
    }
}
