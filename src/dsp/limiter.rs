//! Lookahead stereo peak limiting.
//!
//! Detection links both channels. A fixed hold keeps the gain reduction in
//! place until the detected peak reaches the output, then an exponential
//! release returns toward unity. Delay storage is supplied by the caller so
//! processing does not allocate.

use super::db;
use super::flush_denormal;

/// Maximum supported lookahead in seconds.
pub const MAX_LOOKAHEAD_SECONDS: f32 = 0.02;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parameters {
    /// Maximum sample level in decibels full scale.
    pub ceiling_db: f32,
    /// Time to return toward unity after a limited peak.
    pub release_seconds: f32,
    /// Delay available for reducing gain before a peak reaches the output.
    pub lookahead_seconds: f32,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            ceiling_db: -0.3,
            release_seconds: 0.1,
            lookahead_seconds: 0.005,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimiterError {
    InvalidSampleRate,
    InvalidParameter,
    StorageTooSmall,
    BufferSize,
}

/// State for one linked stereo limiter.
#[derive(Clone, Copy, Debug)]
pub struct Limiter {
    parameters: Parameters,
    ceiling: f32,
    release_coefficient: f32,
    delay_frames: usize,
    position: usize,
    gain: f32,
    hold_frames: usize,
}

impl Limiter {
    /// Builds validated state. Storage is checked when processing starts.
    pub fn new(sample_rate: f32, parameters: Parameters) -> Result<Self, LimiterError> {
        validate(sample_rate, parameters)?;
        let delay_frames = (parameters.lookahead_seconds * sample_rate).round() as usize;
        Ok(Self {
            parameters,
            ceiling: db::to_linear(parameters.ceiling_db),
            release_coefficient: (-1.0 / (parameters.release_seconds * sample_rate)).exp(),
            delay_frames,
            position: 0,
            gain: 1.0,
            hold_frames: 0,
        })
    }

    #[must_use]
    pub const fn parameters(&self) -> Parameters {
        self.parameters
    }

    #[must_use]
    pub const fn latency_frames(&self) -> usize {
        self.delay_frames
    }

    #[must_use]
    pub const fn required_storage_frames(&self) -> usize {
        self.delay_frames
    }

    #[must_use]
    pub fn gain_reduction_db(&self) -> f32 {
        -db::from_linear(self.gain)
    }

    /// Clears the envelope and delay cursor. The caller clears storage when
    /// old audio must also be discarded.
    pub fn reset(&mut self) {
        self.position = 0;
        self.gain = 1.0;
        self.hold_frames = 0;
    }

    /// Processes one linked stereo sample.
    pub fn process_stereo(
        &mut self,
        storage: &mut [[f32; 2]],
        left: f32,
        right: f32,
    ) -> Result<(f32, f32), LimiterError> {
        if storage.len() < self.required_storage_frames() {
            return Err(LimiterError::StorageTooSmall);
        }
        let input = [finite(left), finite(right)];
        let peak = input[0].abs().max(input[1].abs());
        let target = if peak > self.ceiling {
            self.ceiling / peak
        } else {
            1.0
        };
        if target < self.gain {
            self.gain = target;
            self.hold_frames = self.delay_frames;
        }

        let delayed = if self.delay_frames == 0 {
            input
        } else {
            let delayed = storage[self.position];
            storage[self.position] = input;
            self.position += 1;
            if self.position == self.delay_frames {
                self.position = 0;
            }
            delayed
        };
        let output = (
            flush_denormal(delayed[0] * self.gain),
            flush_denormal(delayed[1] * self.gain),
        );
        if self.hold_frames > 0 {
            self.hold_frames -= 1;
        } else {
            self.gain = 1.0 - (1.0 - self.gain) * self.release_coefficient;
        }
        Ok(output)
    }

    /// Processes matching input and output blocks.
    pub fn process_block(
        &mut self,
        storage: &mut [[f32; 2]],
        input: &[[f32; 2]],
        output: &mut [[f32; 2]],
    ) -> Result<(), LimiterError> {
        if input.len() != output.len() {
            return Err(LimiterError::BufferSize);
        }
        if storage.len() < self.required_storage_frames() {
            return Err(LimiterError::StorageTooSmall);
        }
        for (input, output) in input.iter().zip(output) {
            let (left, right) = self.process_stereo(storage, input[0], input[1])?;
            *output = [left, right];
        }
        Ok(())
    }

    /// Processes one interleaved stereo block in place.
    pub fn process_in_place(
        &mut self,
        storage: &mut [[f32; 2]],
        audio: &mut [[f32; 2]],
    ) -> Result<(), LimiterError> {
        if storage.len() < self.required_storage_frames() {
            return Err(LimiterError::StorageTooSmall);
        }
        for frame in audio {
            let (left, right) = self.process_stereo(storage, frame[0], frame[1])?;
            *frame = [left, right];
        }
        Ok(())
    }
}

fn validate(sample_rate: f32, parameters: Parameters) -> Result<(), LimiterError> {
    if !sample_rate.is_finite() || !(8_000.0..=192_000.0).contains(&sample_rate) {
        return Err(LimiterError::InvalidSampleRate);
    }
    if !parameters.ceiling_db.is_finite()
        || !(-24.0..=0.0).contains(&parameters.ceiling_db)
        || !parameters.release_seconds.is_finite()
        || !(0.001..=10.0).contains(&parameters.release_seconds)
        || !parameters.lookahead_seconds.is_finite()
        || !(0.0..=MAX_LOOKAHEAD_SECONDS).contains(&parameters.lookahead_seconds)
    {
        return Err(LimiterError::InvalidParameter);
    }
    Ok(())
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn immediate(ceiling_db: f32) -> Limiter {
        Limiter::new(
            RATE,
            Parameters {
                ceiling_db,
                release_seconds: 0.1,
                lookahead_seconds: 0.0,
            },
        )
        .unwrap()
    }

    #[test]
    fn zero_lookahead_holds_every_sample_to_the_ceiling() {
        let mut limiter = immediate(-6.020_6);
        let mut storage = [];
        for input in [0.1, 1.0, -2.0, 0.25] {
            let (left, right) = limiter
                .process_stereo(&mut storage, input, -input * 0.5)
                .unwrap();
            assert!(left.abs() <= 0.500_001, "{left}");
            assert!(right.abs() <= 0.500_001, "{right}");
        }
    }

    #[test]
    fn lookahead_delays_audio_and_holds_reduction_until_the_peak() {
        let mut limiter = Limiter::new(
            1_000.0_f32.max(8_000.0),
            Parameters {
                ceiling_db: -6.020_6,
                release_seconds: 0.01,
                lookahead_seconds: 0.001,
            },
        )
        .unwrap();
        assert_eq!(limiter.latency_frames(), 8);
        let mut storage = vec![[0.0; 2]; limiter.required_storage_frames()];
        let mut output = Vec::new();
        output.push(limiter.process_stereo(&mut storage, 2.0, 1.0).unwrap());
        for _ in 0..8 {
            output.push(limiter.process_stereo(&mut storage, 0.0, 0.0).unwrap());
        }
        assert_eq!(output[0], (0.0, 0.0));
        assert!((output[8].0 - 0.5).abs() < 1e-5, "{:?}", output[8]);
        assert!((output[8].1 - 0.25).abs() < 1e-5, "{:?}", output[8]);
    }

    #[test]
    fn stereo_link_preserves_channel_balance() {
        let mut limiter = immediate(-12.0);
        let mut storage = [];
        let (left, right) = limiter.process_stereo(&mut storage, 1.0, -0.25).unwrap();
        assert!((left / right + 4.0).abs() < 1e-5);
    }

    #[test]
    fn release_returns_monotonically_toward_unity() {
        let mut limiter = immediate(-12.0);
        let mut storage = [];
        let _ = limiter.process_stereo(&mut storage, 1.0, 1.0).unwrap();
        let mut previous = limiter.gain_reduction_db();
        for _ in 0..100 {
            let _ = limiter.process_stereo(&mut storage, 0.0, 0.0).unwrap();
            let current = limiter.gain_reduction_db();
            assert!(current <= previous);
            previous = current;
        }
        assert!(previous < 12.0 && previous > 0.0);
    }

    #[test]
    fn invalid_configuration_and_storage_are_rejected() {
        for parameters in [
            Parameters {
                ceiling_db: f32::NAN,
                ..Parameters::default()
            },
            Parameters {
                release_seconds: 0.0,
                ..Parameters::default()
            },
            Parameters {
                lookahead_seconds: 0.1,
                ..Parameters::default()
            },
        ] {
            assert!(matches!(
                Limiter::new(RATE, parameters),
                Err(LimiterError::InvalidParameter)
            ));
        }
        assert!(matches!(
            Limiter::new(0.0, Parameters::default()),
            Err(LimiterError::InvalidSampleRate)
        ));
        let mut limiter = Limiter::new(RATE, Parameters::default()).unwrap();
        assert!(matches!(
            limiter.process_stereo(&mut [], 1.0, 1.0),
            Err(LimiterError::StorageTooSmall)
        ));
    }

    #[test]
    fn block_processing_is_deterministic_and_sanitizes_input() {
        let parameters = Parameters::default();
        let mut first = Limiter::new(RATE, parameters).unwrap();
        let mut second = first;
        let mut first_storage = vec![[0.0; 2]; first.required_storage_frames()];
        let mut second_storage = first_storage.clone();
        let input: Vec<[f32; 2]> = (0..512)
            .map(|index| {
                if index == 17 {
                    [f32::NAN, f32::INFINITY]
                } else {
                    [
                        (index as f32 * 0.071).sin() * 2.0,
                        (index as f32 * 0.13).cos(),
                    ]
                }
            })
            .collect();
        let mut a = vec![[0.0; 2]; input.len()];
        let mut b = a.clone();
        first
            .process_block(&mut first_storage, &input, &mut a)
            .unwrap();
        second
            .process_block(&mut second_storage, &input, &mut b)
            .unwrap();
        assert_eq!(a, b);
        assert!(a.iter().flatten().all(|sample| sample.is_finite()));
    }
}
