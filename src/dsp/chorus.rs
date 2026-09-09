//! Stereo chorus over caller-owned delay storage.

use super::{clamp, delay::DelayLine, flush_denormal};

/// Chorus controls. Time values are seconds and phase offset is cycles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parameters {
    pub rate_hz: f32,
    pub center_seconds: f32,
    pub depth_seconds: f32,
    pub feedback: f32,
    pub mix: f32,
    pub stereo_phase: f32,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            rate_hz: 0.8,
            center_seconds: 0.012,
            depth_seconds: 0.003,
            feedback: 0.1,
            mix: 0.5,
            stereo_phase: 0.25,
        }
    }
}

/// Fixed-size chorus state. Delay samples live outside this value.
#[derive(Clone, Copy, Debug)]
pub struct Chorus {
    left: DelayLine,
    right: DelayLine,
    parameters: Parameters,
    sample_rate: f32,
    phase: f32,
}

impl Chorus {
    #[must_use]
    pub fn new(sample_rate: f32, parameters: Parameters) -> Self {
        Self {
            left: DelayLine::new(),
            right: DelayLine::new(),
            parameters: sanitize(parameters),
            sample_rate: if sample_rate.is_finite() && sample_rate > 0.0 {
                sample_rate
            } else {
                48_000.0
            },
            phase: 0.0,
        }
    }

    /// Minimum samples needed in each delay channel.
    #[must_use]
    pub fn required_storage_frames(self) -> usize {
        ((self.parameters.center_seconds + self.parameters.depth_seconds) * self.sample_rate).ceil()
            as usize
            + 2
    }

    #[must_use]
    pub const fn parameters(self) -> Parameters {
        self.parameters
    }

    /// Processes one stereo sample and advances both modulated delay lines.
    #[inline]
    pub fn process_stereo(
        &mut self,
        left_storage: &mut [f32],
        right_storage: &mut [f32],
        left: f32,
        right: f32,
    ) -> (f32, f32) {
        let left = finite(left);
        let right = finite(right);
        let center = self.parameters.center_seconds * self.sample_rate;
        let depth = self.parameters.depth_seconds * self.sample_rate;
        let left_lfo = (self.phase * core::f32::consts::TAU).sin();
        let right_phase = (self.phase + self.parameters.stereo_phase).fract();
        let right_lfo = (right_phase * core::f32::consts::TAU).sin();
        let wet_left = self
            .left
            .read_interpolated(left_storage, center + depth * left_lfo);
        let wet_right = self
            .right
            .read_interpolated(right_storage, center + depth * right_lfo);
        self.left.write(
            left_storage,
            flush_denormal(left + wet_left * self.parameters.feedback),
        );
        self.right.write(
            right_storage,
            flush_denormal(right + wet_right * self.parameters.feedback),
        );
        self.phase += self.parameters.rate_hz / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        (
            left + (wet_left - left) * self.parameters.mix,
            right + (wet_right - right) * self.parameters.mix,
        )
    }

    pub fn process_block(
        &mut self,
        left_storage: &mut [f32],
        right_storage: &mut [f32],
        audio: &mut [[f32; 2]],
    ) {
        for frame in audio {
            (frame[0], frame[1]) =
                self.process_stereo(left_storage, right_storage, frame[0], frame[1]);
        }
    }

    pub fn reset(&mut self, left_storage: &mut [f32], right_storage: &mut [f32]) {
        self.left.reset(left_storage);
        self.right.reset(right_storage);
        self.phase = 0.0;
    }
}

#[inline]
fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn sanitize(parameters: Parameters) -> Parameters {
    Parameters {
        rate_hz: clamp(parameters.rate_hz, 0.01, 20.0),
        center_seconds: clamp(parameters.center_seconds, 0.000_1, 0.1),
        depth_seconds: clamp(parameters.depth_seconds, 0.0, 0.05),
        feedback: clamp(parameters.feedback, -0.95, 0.95),
        mix: clamp(parameters.mix, 0.0, 1.0),
        stereo_phase: clamp(parameters.stereo_phase, 0.0, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn render(parameters: Parameters, samples: usize) -> Vec<[f32; 2]> {
        let mut chorus = Chorus::new(RATE, parameters);
        let size = chorus.required_storage_frames();
        let mut left = vec![0.0; size];
        let mut right = vec![0.0; size];
        let mut audio = vec![[0.0; 2]; samples];
        audio[0] = [1.0, 1.0];
        chorus.process_block(&mut left, &mut right, &mut audio);
        audio
    }

    #[test]
    fn dry_mix_passes_input_exactly() {
        let parameters = Parameters {
            mix: 0.0,
            ..Parameters::default()
        };
        let output = render(parameters, 2_048);
        assert_eq!(output[0], [1.0, 1.0]);
        assert!(output[1..].iter().all(|frame| *frame == [0.0; 2]));
    }

    #[test]
    fn an_impulse_reaches_both_modulated_delays() {
        let output = render(Parameters::default(), 4_096);
        assert!(output[1..].iter().any(|frame| frame[0].abs() > 0.01));
        assert!(output[1..].iter().any(|frame| frame[1].abs() > 0.01));
        assert_ne!(
            output.iter().map(|frame| frame[0]).collect::<Vec<_>>(),
            output.iter().map(|frame| frame[1]).collect::<Vec<_>>()
        );
    }

    #[test]
    fn reset_repeats_the_same_render() {
        let parameters = Parameters::default();
        let mut chorus = Chorus::new(RATE, parameters);
        let size = chorus.required_storage_frames();
        let mut left = vec![0.0; size];
        let mut right = vec![0.0; size];
        let mut first = vec![[0.25, -0.5]; 4_096];
        chorus.process_block(&mut left, &mut right, &mut first);
        chorus.reset(&mut left, &mut right);
        let mut second = vec![[0.25, -0.5]; 4_096];
        chorus.process_block(&mut left, &mut right, &mut second);
        assert_eq!(first, second);
    }

    #[test]
    fn invalid_values_are_bounded_and_samples_stay_finite() {
        let chorus = Chorus::new(
            RATE,
            Parameters {
                rate_hz: f32::NAN,
                center_seconds: f32::INFINITY,
                depth_seconds: -1.0,
                feedback: 3.0,
                mix: -2.0,
                stereo_phase: 5.0,
            },
        );
        let parameters = chorus.parameters();
        assert_eq!(parameters.rate_hz, 0.01);
        assert_eq!(parameters.center_seconds, 0.1);
        assert_eq!(parameters.depth_seconds, 0.0);
        assert_eq!(parameters.feedback, 0.95);
        assert_eq!(parameters.mix, 0.0);
        assert_eq!(parameters.stereo_phase, 1.0);
        let output = render(parameters, 1_024);
        assert!(output.iter().flatten().all(|sample| sample.is_finite()));
    }
}
