#[cfg(feature = "audio")]
mod inner {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, StreamConfig};
    use rtrb::{Producer, RingBuffer};

    pub struct AudioOutput {
        pub producer: Producer<f32>,
        pub sample_rate: u32,
        _stream: cpal::Stream,
    }

    impl AudioOutput {
        pub fn new() -> Option<Self> {
            let host = cpal::default_host();
            let device = host.default_output_device()?;
            let supported = device.default_output_config().ok()?;
            let sample_rate = supported.sample_rate();
            let channels = supported.channels() as usize;
            let config = StreamConfig {
                channels: channels as u16,
                sample_rate,
                buffer_size: cpal::BufferSize::Default,
            };

            let (producer, mut consumer) = RingBuffer::<f32>::new(4096);

            let stream = match supported.sample_format() {
                SampleFormat::F32 => device.build_output_stream(
                    &config,
                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        for frame in data.chunks_mut(channels) {
                            let sample = consumer.pop().unwrap_or(0.0);
                            for s in frame.iter_mut() { *s = sample; }
                        }
                    },
                    |err| log::error!("Audio error: {}", err),
                    None,
                ).ok()?,
                SampleFormat::I16 => device.build_output_stream(
                    &config,
                    move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                        for frame in data.chunks_mut(channels) {
                            let sample = consumer.pop().unwrap_or(0.0);
                            let s16 = (sample * 32767.0) as i16;
                            for s in frame.iter_mut() { *s = s16; }
                        }
                    },
                    |err| log::error!("Audio error: {}", err),
                    None,
                ).ok()?,
                _ => return None,
            };

            stream.play().ok()?;
            log::info!("Audio initialized: {}Hz, {} ch", sample_rate.0, channels);
            Some(Self { producer, sample_rate: sample_rate.0, _stream: stream })
        }

        pub fn push_samples(&mut self, samples: &[f32], volume: f32) {
            for &s in samples {
                let _ = self.producer.push(s * volume);
            }
        }
    }
}

#[cfg(feature = "audio")]
pub use inner::AudioOutput;

/// Stub audio output when the audio feature is disabled.
#[cfg(not(feature = "audio"))]
pub struct AudioOutput {
    pub sample_rate: u32,
}

#[cfg(not(feature = "audio"))]
impl AudioOutput {
    pub fn new() -> Option<Self> {
        log::info!("Audio disabled (compile with --features audio)");
        Some(Self { sample_rate: emu_common::SAMPLE_RATE })
    }
    pub fn push_samples(&mut self, _samples: &[f32], _volume: f32) {}
}
