//! Stereo nonlinear saturation with bounded oversampling.

use super::db::to_linear;

/// Nonlinear transfer curve.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Curve {
    /// Cubic soft clipping with a hard bound outside unit amplitude.
    #[default]
    SoftClip,
    /// Hyperbolic tangent saturation.
    Tanh,
    /// Abrupt clipping at unit amplitude.
    HardClip,
    /// Asymmetric curve that introduces even harmonics.
    Diode,
}

/// Oversampling factor used around the nonlinear transfer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Oversampling {
    /// One transfer evaluation per input sample.
    #[default]
    One,
    /// Two transfer evaluations per input sample.
    Two,
    /// Four transfer evaluations per input sample.
    Four,
}

impl Oversampling {
    const fn factor(self) -> usize {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Four => 4,
        }
    }
}

/// User-facing saturator settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parameters {
    /// Gain before the nonlinear stage, in decibels.
    pub drive_db: f32,
    /// Gain after the nonlinear stage, in decibels.
    pub output_db: f32,
    /// Processed proportion from zero through one.
    pub mix: f32,
    /// Transfer curve.
    pub curve: Curve,
    /// Nonlinear oversampling factor.
    pub oversampling: Oversampling,
    /// Removes DC introduced by an asymmetric curve.
    pub dc_filter: bool,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            drive_db: 6.0,
            output_db: -3.0,
            mix: 1.0,
            curve: Curve::SoftClip,
            oversampling: Oversampling::Two,
            dc_filter: true,
        }
    }
}

impl Parameters {
    fn sanitized(self) -> Self {
        Self {
            drive_db: super::clamp(self.drive_db, -24.0, 48.0),
            output_db: super::clamp(self.output_db, -48.0, 24.0),
            mix: super::clamp(self.mix, 0.0, 1.0),
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ChannelState {
    previous_input: f32,
    dc_input: f32,
    dc_output: f32,
}

/// Saturator with independent channel history and no allocated state.
#[derive(Clone, Copy, Debug)]
pub struct Saturator {
    parameters: Parameters,
    input_gain: f32,
    output_gain: f32,
    dc_coefficient: f32,
    channels: [ChannelState; 2],
}

impl Saturator {
    /// Creates a saturator. Invalid numeric settings are clamped, and an
    /// invalid sample rate falls back to 48 kHz.
    #[must_use]
    pub fn new(sample_rate: f32, parameters: Parameters) -> Self {
        let mut saturator = Self {
            parameters: Parameters::default(),
            input_gain: 1.0,
            output_gain: 1.0,
            dc_coefficient: 0.0,
            channels: [ChannelState::default(); 2],
        };
        saturator.set_parameters(sample_rate, parameters);
        saturator
    }

    /// Changes the transfer settings while preserving filter history.
    pub fn set_parameters(&mut self, sample_rate: f32, parameters: Parameters) {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.parameters = parameters.sanitized();
        self.input_gain = to_linear(self.parameters.drive_db);
        self.output_gain = to_linear(self.parameters.output_db);
        self.dc_coefficient = (-2.0 * core::f32::consts::PI * 8.0 / sample_rate).exp();
    }

    /// Current sanitized settings.
    #[must_use]
    pub const fn parameters(&self) -> Parameters {
        self.parameters
    }

    /// Clears interpolation and DC-filter history.
    pub fn reset(&mut self) {
        self.channels = [ChannelState::default(); 2];
    }

    /// Processes one stereo sample.
    #[inline]
    #[must_use]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let left = if left.is_finite() { left } else { 0.0 };
        let right = if right.is_finite() { right } else { 0.0 };
        let wet_left = process_channel(
            left,
            &mut self.channels[0],
            self.parameters,
            self.input_gain,
            self.dc_coefficient,
        );
        let wet_right = process_channel(
            right,
            &mut self.channels[1],
            self.parameters,
            self.input_gain,
            self.dc_coefficient,
        );
        let dry = 1.0 - self.parameters.mix;
        (
            super::flush_denormal((left * dry + wet_left * self.parameters.mix) * self.output_gain),
            super::flush_denormal(
                (right * dry + wet_right * self.parameters.mix) * self.output_gain,
            ),
        )
    }

    /// Processes paired channel buffers in place.
    #[inline]
    pub fn process_block(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            (*left, *right) = self.process_stereo(*left, *right);
        }
    }
}

#[inline]
fn process_channel(
    input: f32,
    state: &mut ChannelState,
    parameters: Parameters,
    input_gain: f32,
    dc_coefficient: f32,
) -> f32 {
    let factor = parameters.oversampling.factor();
    let mut accumulated = 0.0;
    for step in 1..=factor {
        let phase = step as f32 / factor as f32;
        let interpolated = state.previous_input + (input - state.previous_input) * phase;
        accumulated += transfer(interpolated * input_gain, parameters.curve);
    }
    state.previous_input = input;
    let shaped = accumulated / factor as f32;
    if !parameters.dc_filter {
        return shaped;
    }
    let filtered = shaped - state.dc_input + dc_coefficient * state.dc_output;
    state.dc_input = shaped;
    state.dc_output = super::flush_denormal(filtered);
    state.dc_output
}

#[inline]
fn transfer(input: f32, curve: Curve) -> f32 {
    match curve {
        Curve::SoftClip => {
            if input <= -1.0 {
                -1.0
            } else if input >= 1.0 {
                1.0
            } else {
                input * (1.5 - 0.5 * input * input)
            }
        }
        Curve::Tanh => input.tanh(),
        Curve::HardClip => input.clamp(-1.0, 1.0),
        Curve::Diode => {
            if input >= 0.0 {
                1.0 - (-input).exp()
            } else {
                -0.5 * (1.0 - (2.0 * input).exp())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn close(actual: f32, expected: f32, tolerance: f32) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} != {expected}"
        );
    }

    #[test]
    fn every_curve_is_bounded_and_finite() {
        for curve in [Curve::SoftClip, Curve::Tanh, Curve::HardClip, Curve::Diode] {
            let mut saturator = Saturator::new(
                RATE,
                Parameters {
                    drive_db: 48.0,
                    output_db: 0.0,
                    curve,
                    dc_filter: false,
                    ..Parameters::default()
                },
            );
            for input in [-100.0, -1.0, 0.0, 1.0, 100.0] {
                let (left, right) = saturator.process_stereo(input, -input);
                assert!(left.is_finite() && right.is_finite());
                assert!(left.abs() <= 1.0 && right.abs() <= 1.0);
            }
        }
    }

    #[test]
    fn zero_mix_is_a_trimmed_dry_path() {
        let mut saturator = Saturator::new(
            RATE,
            Parameters {
                drive_db: 40.0,
                output_db: -6.0,
                mix: 0.0,
                ..Parameters::default()
            },
        );
        let (left, right) = saturator.process_stereo(0.25, -0.5);
        close(left, 0.25 * to_linear(-6.0), 1e-6);
        close(right, -0.5 * to_linear(-6.0), 1e-6);
    }

    #[test]
    fn drive_increases_nonlinear_harmonics() {
        let settings = |drive_db| Parameters {
            drive_db,
            output_db: 0.0,
            mix: 1.0,
            curve: Curve::Tanh,
            oversampling: Oversampling::One,
            dc_filter: false,
        };
        let mut low = Saturator::new(RATE, settings(-12.0));
        let mut high = Saturator::new(RATE, settings(24.0));
        let low_level = low.process_stereo(0.2, 0.2).0;
        let high_level = high.process_stereo(0.2, 0.2).0;
        assert!(high_level > low_level * 2.0);
        assert!(high_level < 1.0);
    }

    #[test]
    fn asymmetric_curve_has_different_positive_and_negative_levels() {
        let parameters = Parameters {
            drive_db: 0.0,
            output_db: 0.0,
            mix: 1.0,
            curve: Curve::Diode,
            oversampling: Oversampling::One,
            dc_filter: false,
        };
        let mut positive = Saturator::new(RATE, parameters);
        let mut negative = Saturator::new(RATE, parameters);
        let above = positive.process_stereo(0.5, 0.5).0;
        let below = negative.process_stereo(-0.5, -0.5).0;
        assert!(above > below.abs());
    }

    #[test]
    fn block_and_sample_paths_match() {
        let mut block = Saturator::new(RATE, Parameters::default());
        let mut sample = block;
        let mut left = [0.1, 0.5, -0.75, 2.0];
        let mut right = [-0.2, 0.4, 0.8, -2.0];
        let expected: Vec<_> = left
            .iter()
            .zip(right.iter())
            .map(|(left, right)| sample.process_stereo(*left, *right))
            .collect();
        block.process_block(&mut left, &mut right);
        for (index, (expected_left, expected_right)) in expected.into_iter().enumerate() {
            close(left[index], expected_left, 1e-7);
            close(right[index], expected_right, 1e-7);
        }
    }

    #[test]
    fn invalid_numbers_are_sanitized_and_output_stays_finite() {
        let mut saturator = Saturator::new(
            f32::NAN,
            Parameters {
                drive_db: f32::NAN,
                output_db: f32::INFINITY,
                mix: f32::NEG_INFINITY,
                ..Parameters::default()
            },
        );
        assert_eq!(saturator.parameters().drive_db, -24.0);
        assert_eq!(saturator.parameters().output_db, 24.0);
        assert_eq!(saturator.parameters().mix, 0.0);
        let output = saturator.process_stereo(f32::NAN, f32::INFINITY);
        assert!(output.0.is_finite() && output.1.is_finite());
    }

    #[test]
    fn reset_clears_interpolation_and_filter_history() {
        let mut changed = Saturator::new(RATE, Parameters::default());
        let _ = changed.process_stereo(1.0, -1.0);
        changed.reset();
        let mut fresh = Saturator::new(RATE, Parameters::default());
        assert_eq!(
            changed.process_stereo(0.25, -0.25),
            fresh.process_stereo(0.25, -0.25)
        );
    }
}
