//! Algorithmic stereo reverb over caller-owned storage.

use super::{clamp, flush_denormal};

const COMBS: usize = 4;
const ALLPASSES: usize = 2;
const CHANNELS: usize = 2;
const PRE_LINES: usize = CHANNELS;
const COMB_LINES: usize = COMBS * CHANNELS;
const ALLPASS_LINES: usize = ALLPASSES * CHANNELS;
const LINES: usize = PRE_LINES + COMB_LINES + ALLPASS_LINES;

const COMB_SECONDS: [f32; COMBS] = [0.029_7, 0.037_1, 0.041_1, 0.043_7];
const ALLPASS_SECONDS: [f32; ALLPASSES] = [0.005, 0.001_7];

/// Reverb controls. Time values are seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parameters {
    pub size: f32,
    pub decay_seconds: f32,
    pub damping: f32,
    pub diffusion: f32,
    pub pre_delay_seconds: f32,
    pub width: f32,
    pub mix: f32,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            size: 0.55,
            decay_seconds: 2.4,
            damping: 0.35,
            diffusion: 0.7,
            pre_delay_seconds: 0.015,
            width: 1.0,
            mix: 0.3,
        }
    }
}

/// Fixed-size reverb state. Sample memory is supplied during processing.
#[derive(Clone, Copy, Debug)]
pub struct Reverb {
    parameters: Parameters,
    offsets: [usize; LINES],
    lengths: [usize; LINES],
    positions: [usize; LINES],
    damping_state: [f32; COMB_LINES],
    feedback: [f32; COMB_LINES],
    storage_frames: usize,
}

impl Reverb {
    #[must_use]
    pub fn new(sample_rate: f32, parameters: Parameters) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let parameters = sanitize(parameters);
        let mut lengths = [1_usize; LINES];
        let pre = (parameters.pre_delay_seconds * sample_rate).ceil() as usize;
        lengths[0] = pre.max(1);
        lengths[1] = pre.max(1);
        let scale = 0.5 + parameters.size;
        for channel in 0..CHANNELS {
            for (index, seconds) in COMB_SECONDS.iter().enumerate() {
                let spread = channel as f32 * 0.001_3;
                lengths[PRE_LINES + channel * COMBS + index] =
                    ((seconds * scale + spread) * sample_rate).round() as usize;
            }
            for (index, seconds) in ALLPASS_SECONDS.iter().enumerate() {
                let spread = channel as f32 * 0.000_7;
                lengths[PRE_LINES + COMB_LINES + channel * ALLPASSES + index] =
                    ((seconds * scale + spread) * sample_rate).round() as usize;
            }
        }
        let mut offsets = [0_usize; LINES];
        let mut storage_frames = 0_usize;
        for index in 0..LINES {
            lengths[index] = lengths[index].max(1);
            offsets[index] = storage_frames;
            storage_frames = storage_frames.saturating_add(lengths[index]);
        }
        let mut feedback = [0.0_f32; COMB_LINES];
        for channel in 0..CHANNELS {
            for comb in 0..COMBS {
                let state = channel * COMBS + comb;
                let line = PRE_LINES + state;
                let delay_seconds = lengths[line] as f32 / sample_rate;
                feedback[state] = 10.0_f32
                    .powf(-3.0 * delay_seconds / parameters.decay_seconds)
                    .min(0.98);
            }
        }
        Self {
            parameters,
            offsets,
            lengths,
            positions: [0; LINES],
            damping_state: [0.0; COMB_LINES],
            feedback,
            storage_frames,
        }
    }

    #[must_use]
    pub const fn parameters(self) -> Parameters {
        self.parameters
    }

    #[must_use]
    pub const fn required_storage_frames(self) -> usize {
        self.storage_frames
    }

    /// Processes one stereo sample. A short storage slice returns dry audio.
    #[inline]
    pub fn process_stereo(&mut self, storage: &mut [f32], left: f32, right: f32) -> (f32, f32) {
        let left = finite(left);
        let right = finite(right);
        if storage.len() < self.storage_frames {
            return (left, right);
        }
        let mut input = [left, right];
        if self.parameters.pre_delay_seconds > 0.0 {
            for (channel, sample) in input.iter_mut().enumerate() {
                *sample = self.line_tick(storage, channel, *sample);
            }
        }
        let mono = (input[0] + input[1]) * 0.5;
        let mut wet = [0.0_f32; CHANNELS];
        for (channel, wet_sample) in wet.iter_mut().enumerate() {
            let mut sum = 0.0;
            for comb in 0..COMBS {
                let line = PRE_LINES + channel * COMBS + comb;
                let delayed = self.line_read(storage, line);
                let state = channel * COMBS + comb;
                let filtered = delayed * (1.0 - self.parameters.damping)
                    + self.damping_state[state] * self.parameters.damping;
                self.damping_state[state] = flush_denormal(filtered);
                self.line_write(storage, line, mono + filtered * self.feedback[state]);
                sum += delayed;
            }
            let mut value = sum * (1.0 / COMBS as f32);
            let coefficient = 0.2 + self.parameters.diffusion * 0.55;
            for stage in 0..ALLPASSES {
                let line = PRE_LINES + COMB_LINES + channel * ALLPASSES + stage;
                let delayed = self.line_read(storage, line);
                self.line_write(storage, line, value + delayed * coefficient);
                value = delayed - value;
            }
            *wet_sample = value;
        }
        let direct = 0.5 + self.parameters.width * 0.5;
        let cross = 0.5 - self.parameters.width * 0.5;
        let wet_left = wet[0] * direct + wet[1] * cross;
        let wet_right = wet[1] * direct + wet[0] * cross;
        (
            left + (wet_left - left) * self.parameters.mix,
            right + (wet_right - right) * self.parameters.mix,
        )
    }

    pub fn process_block(&mut self, storage: &mut [f32], audio: &mut [[f32; 2]]) {
        for frame in audio {
            (frame[0], frame[1]) = self.process_stereo(storage, frame[0], frame[1]);
        }
    }

    pub fn reset(&mut self, storage: &mut [f32]) {
        storage.fill(0.0);
        self.positions.fill(0);
        self.damping_state.fill(0.0);
    }

    #[inline]
    fn line_read(&self, storage: &[f32], line: usize) -> f32 {
        storage[self.offsets[line] + self.positions[line]]
    }

    #[inline]
    fn line_write(&mut self, storage: &mut [f32], line: usize, sample: f32) {
        storage[self.offsets[line] + self.positions[line]] = flush_denormal(finite(sample));
        self.positions[line] += 1;
        if self.positions[line] == self.lengths[line] {
            self.positions[line] = 0;
        }
    }

    #[inline]
    fn line_tick(&mut self, storage: &mut [f32], line: usize, sample: f32) -> f32 {
        let delayed = self.line_read(storage, line);
        self.line_write(storage, line, sample);
        delayed
    }
}

#[inline]
fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn sanitize(parameters: Parameters) -> Parameters {
    Parameters {
        size: clamp(parameters.size, 0.0, 1.0),
        decay_seconds: clamp(parameters.decay_seconds, 0.1, 30.0),
        damping: clamp(parameters.damping, 0.0, 1.0),
        diffusion: clamp(parameters.diffusion, 0.0, 1.0),
        pre_delay_seconds: clamp(parameters.pre_delay_seconds, 0.0, 0.25),
        width: clamp(parameters.width, 0.0, 1.0),
        mix: clamp(parameters.mix, 0.0, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn impulse(parameters: Parameters, frames: usize) -> Vec<[f32; 2]> {
        let mut reverb = Reverb::new(RATE, parameters);
        let mut storage = vec![0.0; reverb.required_storage_frames()];
        let mut audio = vec![[0.0; 2]; frames];
        audio[0] = [1.0, 1.0];
        reverb.process_block(&mut storage, &mut audio);
        audio
    }

    #[test]
    fn dry_mix_passes_input_exactly() {
        let parameters = Parameters {
            mix: 0.0,
            ..Parameters::default()
        };
        let output = impulse(parameters, 8_192);
        assert_eq!(output[0], [1.0, 1.0]);
        assert!(output[1..].iter().all(|frame| *frame == [0.0; 2]));
    }

    #[test]
    fn an_impulse_produces_a_decaying_stereo_tail() {
        let output = impulse(Parameters::default(), 96_000);
        let early: f32 = output[2_000..24_000]
            .iter()
            .flatten()
            .map(|sample| sample.abs())
            .sum();
        let late: f32 = output[72_000..94_000]
            .iter()
            .flatten()
            .map(|sample| sample.abs())
            .sum();
        assert!(early > 0.1, "{early}");
        assert!(late < early, "{late} >= {early}");
        assert!(output.iter().any(|frame| frame[0] != frame[1]));
    }

    #[test]
    fn longer_decay_retains_more_tail_energy() {
        let short = impulse(
            Parameters {
                decay_seconds: 0.4,
                ..Parameters::default()
            },
            96_000,
        );
        let long = impulse(
            Parameters {
                decay_seconds: 6.0,
                ..Parameters::default()
            },
            96_000,
        );
        let energy = |audio: &[[f32; 2]]| {
            audio[48_000..]
                .iter()
                .flatten()
                .map(|sample| sample.abs())
                .sum::<f32>()
        };
        assert!(energy(&long) > energy(&short));
    }

    #[test]
    fn reset_repeats_the_same_render() {
        let mut reverb = Reverb::new(RATE, Parameters::default());
        let mut storage = vec![0.0; reverb.required_storage_frames()];
        let mut first = vec![[0.2, -0.1]; 8_192];
        reverb.process_block(&mut storage, &mut first);
        reverb.reset(&mut storage);
        let mut second = vec![[0.2, -0.1]; 8_192];
        reverb.process_block(&mut storage, &mut second);
        assert_eq!(first, second);
    }

    #[test]
    fn invalid_controls_are_bounded_and_output_stays_finite() {
        let reverb = Reverb::new(
            RATE,
            Parameters {
                size: f32::NAN,
                decay_seconds: f32::INFINITY,
                damping: -1.0,
                diffusion: 4.0,
                pre_delay_seconds: -1.0,
                width: 3.0,
                mix: f32::NAN,
            },
        );
        assert_eq!(
            reverb.parameters(),
            Parameters {
                size: 0.0,
                decay_seconds: 30.0,
                damping: 0.0,
                diffusion: 1.0,
                pre_delay_seconds: 0.0,
                width: 1.0,
                mix: 0.0,
            }
        );
        let output = impulse(reverb.parameters(), 4_096);
        assert!(output.iter().flatten().all(|sample| sample.is_finite()));
    }

    #[test]
    fn short_storage_returns_clean_dry_samples() {
        let mut reverb = Reverb::new(RATE, Parameters::default());
        assert_eq!(
            reverb.process_stereo(&mut [], f32::NAN, f32::INFINITY),
            (0.0, 0.0)
        );
    }
}
