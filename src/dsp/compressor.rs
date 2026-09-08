//! Feed-forward dynamic range compression.
//!
//! The detector links both channels by their greater absolute level. The
//! gain computer provides a continuously differentiable soft knee, then an
//! attack/release envelope smooths gain reduction before makeup gain is
//! applied. Processing uses fixed-size state and performs no allocation.

use super::clamp;
use super::db::{from_linear, to_linear};

/// User-facing compressor parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parameters {
    /// Level where compression begins, in decibels full scale.
    pub threshold_db: f32,
    /// Input-to-output slope above the threshold. One disables compression.
    pub ratio: f32,
    /// Width of the soft transition around the threshold, in decibels.
    pub knee_db: f32,
    /// Time for increasing gain reduction, in seconds.
    pub attack_seconds: f32,
    /// Time for decreasing gain reduction, in seconds.
    pub release_seconds: f32,
    /// Gain applied after compression, in decibels.
    pub makeup_db: f32,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            threshold_db: -18.0,
            ratio: 4.0,
            knee_db: 6.0,
            attack_seconds: 0.01,
            release_seconds: 0.1,
            makeup_db: 0.0,
        }
    }
}

impl Parameters {
    #[must_use]
    fn sanitized(self) -> Self {
        Self {
            threshold_db: clamp(self.threshold_db, -96.0, 0.0),
            ratio: clamp(self.ratio, 1.0, 100.0),
            knee_db: clamp(self.knee_db, 0.0, 48.0),
            attack_seconds: clamp(self.attack_seconds, 0.0, 10.0),
            release_seconds: clamp(self.release_seconds, 0.0, 30.0),
            makeup_db: clamp(self.makeup_db, -48.0, 48.0),
        }
    }
}

/// Stereo-linked compressor with one gain-reduction envelope.
#[derive(Clone, Copy, Debug)]
pub struct Compressor {
    parameters: Parameters,
    attack_coefficient: f32,
    release_coefficient: f32,
    makeup_gain: f32,
    gain_reduction_db: f32,
}

impl Compressor {
    /// Creates a compressor for `sample_rate`. Invalid rates fall back to
    /// 48 kHz and invalid parameters are clamped into the supported range.
    #[must_use]
    pub fn new(sample_rate: f32, parameters: Parameters) -> Self {
        let mut compressor = Self {
            parameters: Parameters::default(),
            attack_coefficient: 0.0,
            release_coefficient: 0.0,
            makeup_gain: 1.0,
            gain_reduction_db: 0.0,
        };
        compressor.set_parameters(sample_rate, parameters);
        compressor
    }

    /// Replaces the parameters while preserving the active envelope.
    pub fn set_parameters(&mut self, sample_rate: f32, parameters: Parameters) {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.parameters = parameters.sanitized();
        self.attack_coefficient = time_coefficient(self.parameters.attack_seconds, sample_rate);
        self.release_coefficient = time_coefficient(self.parameters.release_seconds, sample_rate);
        self.makeup_gain = to_linear(self.parameters.makeup_db);
    }

    /// Current sanitized parameters.
    #[must_use]
    pub const fn parameters(&self) -> Parameters {
        self.parameters
    }

    /// Current positive gain reduction in decibels.
    #[must_use]
    pub const fn gain_reduction_db(&self) -> f32 {
        -self.gain_reduction_db
    }

    /// Clears the detector envelope.
    pub fn reset(&mut self) {
        self.gain_reduction_db = 0.0;
    }

    /// Processes a linked stereo sample.
    #[inline]
    #[must_use]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let left = if left.is_finite() { left } else { 0.0 };
        let right = if right.is_finite() { right } else { 0.0 };
        let detector = left.abs().max(right.abs());
        let input_db = from_linear(detector);
        let target = self.static_gain_db(input_db);
        let coefficient = if target < self.gain_reduction_db {
            self.attack_coefficient
        } else {
            self.release_coefficient
        };
        self.gain_reduction_db = target + coefficient * (self.gain_reduction_db - target);
        if !self.gain_reduction_db.is_finite() {
            self.gain_reduction_db = 0.0;
        }
        let gain = to_linear(self.gain_reduction_db) * self.makeup_gain;
        (
            super::flush_denormal(left * gain),
            super::flush_denormal(right * gain),
        )
    }

    /// Processes matching stereo blocks in place. The shorter input bounds
    /// processing if channel lengths differ.
    #[inline]
    pub fn process_block(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            (*left, *right) = self.process_stereo(*left, *right);
        }
    }

    #[inline]
    fn static_gain_db(&self, input_db: f32) -> f32 {
        if !input_db.is_finite() {
            return 0.0;
        }
        let threshold = self.parameters.threshold_db;
        let knee = self.parameters.knee_db;
        let slope = 1.0 / self.parameters.ratio - 1.0;
        if knee <= 0.0 {
            if input_db <= threshold {
                0.0
            } else {
                slope * (input_db - threshold)
            }
        } else {
            let distance = input_db - threshold;
            if distance <= -knee * 0.5 {
                0.0
            } else if distance >= knee * 0.5 {
                slope * distance
            } else {
                let within = distance + knee * 0.5;
                slope * within * within / (2.0 * knee)
            }
        }
    }
}

#[inline]
fn time_coefficient(seconds: f32, sample_rate: f32) -> f32 {
    if seconds <= 0.0 {
        0.0
    } else {
        (-1.0 / (seconds * sample_rate)).exp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn hard(threshold_db: f32, ratio: f32) -> Parameters {
        Parameters {
            threshold_db,
            ratio,
            knee_db: 0.0,
            attack_seconds: 0.0,
            release_seconds: 0.0,
            makeup_db: 0.0,
        }
    }

    fn close(actual: f32, expected: f32, tolerance: f32) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} != {expected}"
        );
    }

    #[test]
    fn below_threshold_passes_unchanged() {
        let mut compressor = Compressor::new(RATE, hard(-12.0, 4.0));
        let input = to_linear(-18.0);
        let (left, right) = compressor.process_stereo(input, -input * 0.5);
        close(left, input, 1e-6);
        close(right, -input * 0.5, 1e-6);
        assert_eq!(compressor.gain_reduction_db(), 0.0);
    }

    #[test]
    fn ratio_sets_the_output_slope() {
        let mut compressor = Compressor::new(RATE, hard(-20.0, 4.0));
        let (output, _) = compressor.process_stereo(to_linear(0.0), 0.0);
        close(from_linear(output), -15.0, 1e-4);
        close(compressor.gain_reduction_db(), 15.0, 1e-4);
    }

    #[test]
    fn soft_knee_is_continuous() {
        let parameters = Parameters {
            knee_db: 8.0,
            ..hard(-20.0, 4.0)
        };
        let compressor = Compressor::new(RATE, parameters);
        close(compressor.static_gain_db(-24.0), 0.0, 1e-6);
        close(compressor.static_gain_db(-20.0), -0.75, 1e-6);
        close(compressor.static_gain_db(-16.0), -3.0, 1e-6);
        let before = compressor.static_gain_db(-20.001);
        let after = compressor.static_gain_db(-19.999);
        assert!((before - after).abs() < 0.01);
    }

    #[test]
    fn attack_and_release_move_in_the_expected_directions() {
        let parameters = Parameters {
            attack_seconds: 0.01,
            release_seconds: 0.1,
            ..hard(-20.0, 10.0)
        };
        let mut compressor = Compressor::new(RATE, parameters);
        let _ = compressor.process_stereo(1.0, 1.0);
        let first = compressor.gain_reduction_db();
        assert!(first > 0.0 && first < 18.0);
        for _ in 0..4_800 {
            let _ = compressor.process_stereo(1.0, 1.0);
        }
        let settled = compressor.gain_reduction_db();
        assert!(settled > 17.9, "{settled}");
        let _ = compressor.process_stereo(0.0, 0.0);
        let released = compressor.gain_reduction_db();
        assert!(released < settled && released > 17.0, "{released}");
    }

    #[test]
    fn stereo_link_preserves_channel_balance() {
        let mut compressor = Compressor::new(RATE, hard(-20.0, 4.0));
        let (left, right) = compressor.process_stereo(1.0, 0.25);
        close(left / right, 4.0, 1e-5);
    }

    #[test]
    fn makeup_gain_is_applied_after_reduction() {
        let parameters = Parameters {
            makeup_db: 6.0,
            ..hard(-20.0, 4.0)
        };
        let mut compressor = Compressor::new(RATE, parameters);
        let (output, _) = compressor.process_stereo(to_linear(-20.0), 0.0);
        close(from_linear(output), -14.0, 1e-3);
    }

    #[test]
    fn block_path_matches_sample_path() {
        let parameters = Parameters::default();
        let original_left: [f32; 128] = core::array::from_fn(|i| (i as f32 * 0.13).sin());
        let original_right: [f32; 128] = core::array::from_fn(|i| (i as f32 * 0.09).cos() * 0.4);
        let mut expected_left = original_left;
        let mut expected_right = original_right;
        let mut sample = Compressor::new(RATE, parameters);
        for (left, right) in expected_left.iter_mut().zip(expected_right.iter_mut()) {
            (*left, *right) = sample.process_stereo(*left, *right);
        }
        let mut actual_left = original_left;
        let mut actual_right = original_right;
        let mut block = Compressor::new(RATE, parameters);
        block.process_block(&mut actual_left, &mut actual_right);
        assert_eq!(actual_left, expected_left);
        assert_eq!(actual_right, expected_right);
    }

    #[test]
    fn invalid_parameters_are_sanitized_and_output_stays_finite() {
        let parameters = Parameters {
            threshold_db: f32::NAN,
            ratio: f32::NAN,
            knee_db: f32::INFINITY,
            attack_seconds: -1.0,
            release_seconds: f32::NAN,
            makeup_db: f32::NAN,
        };
        let mut compressor = Compressor::new(f32::NAN, parameters);
        assert_eq!(
            compressor.parameters(),
            Parameters {
                threshold_db: -96.0,
                ratio: 1.0,
                knee_db: 48.0,
                attack_seconds: 0.0,
                release_seconds: 0.0,
                makeup_db: -48.0,
            }
        );
        for input in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, 1.0] {
            let (left, right) = compressor.process_stereo(input, 0.25);
            assert!(left.is_finite());
            assert!(right.is_finite());
            assert!(compressor.gain_reduction_db().is_finite());
        }
    }

    #[test]
    fn reset_clears_gain_reduction() {
        let mut compressor = Compressor::new(RATE, hard(-20.0, 4.0));
        let _ = compressor.process_stereo(1.0, 1.0);
        assert!(compressor.gain_reduction_db() > 0.0);
        compressor.reset();
        assert_eq!(compressor.gain_reduction_db(), 0.0);
    }

    #[test]
    fn mismatched_blocks_process_only_paired_samples() {
        let mut compressor = Compressor::new(RATE, hard(-20.0, 4.0));
        let mut left = [1.0, 1.0, 0.5];
        let mut right = [1.0, 1.0];
        compressor.process_block(&mut left, &mut right);
        assert_eq!(left[2], 0.5);
    }
}
