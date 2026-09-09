//! Modulated stereo all-pass phaser.

use core::f32::consts::{PI, TAU};

use crate::dsp::{clamp, flush_denormal};

pub const MAX_STAGES: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parameters {
    pub rate_hz: f32,
    pub center_hz: f32,
    pub depth_octaves: f32,
    pub feedback: f32,
    pub mix: f32,
    pub stereo_phase: f32,
    pub stages: u8,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            rate_hz: 0.35,
            center_hz: 700.0,
            depth_octaves: 2.0,
            feedback: 0.25,
            mix: 0.5,
            stereo_phase: 0.25,
            stages: 6,
        }
    }
}

pub struct Phaser {
    sample_rate: f32,
    parameters: Parameters,
    state: [[f32; MAX_STAGES]; 2],
    feedback: [f32; 2],
    phase: f32,
}

impl Phaser {
    #[must_use]
    pub fn new(sample_rate: f32, parameters: Parameters) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        Self {
            sample_rate,
            parameters: sanitize(parameters, sample_rate),
            state: [[0.0; MAX_STAGES]; 2],
            feedback: [0.0; 2],
            phase: 0.0,
        }
    }

    #[must_use]
    pub fn parameters(&self) -> Parameters {
        self.parameters
    }

    pub fn set_parameters(&mut self, parameters: Parameters) {
        self.parameters = sanitize(parameters, self.sample_rate);
    }

    pub fn reset(&mut self) {
        self.state = [[0.0; MAX_STAGES]; 2];
        self.feedback = [0.0; 2];
        self.phase = 0.0;
    }

    #[inline]
    #[must_use]
    pub fn process_sample(&mut self, input: [f32; 2]) -> [f32; 2] {
        let input = input.map(finite_or_zero);
        let mut output = [0.0; 2];
        for channel in 0..2 {
            let phase = self.phase + channel as f32 * self.parameters.stereo_phase * TAU;
            let frequency = clamp(
                self.parameters.center_hz * (self.parameters.depth_octaves * phase.sin()).exp2(),
                20.0,
                self.sample_rate * 0.45,
            );
            let tangent = (PI * frequency / self.sample_rate).tan();
            let coefficient = (1.0 - tangent) / (1.0 + tangent);
            let mut wet = input[channel] + self.feedback[channel] * self.parameters.feedback;
            for stage in 0..usize::from(self.parameters.stages) {
                let previous = self.state[channel][stage];
                let next = coefficient * wet + previous;
                self.state[channel][stage] = flush_denormal(wet - coefficient * next);
                wet = next;
            }
            self.feedback[channel] = flush_denormal(wet);
            output[channel] =
                flush_denormal(input[channel] + (wet - input[channel]) * self.parameters.mix);
        }
        self.phase += TAU * self.parameters.rate_hz / self.sample_rate;
        if self.phase >= TAU {
            self.phase -= TAU;
        }
        output
    }

    pub fn process_block(&mut self, audio: &mut [[f32; 2]]) {
        for frame in audio {
            *frame = self.process_sample(*frame);
        }
    }
}

fn sanitize(mut parameters: Parameters, sample_rate: f32) -> Parameters {
    parameters.rate_hz = clamp(parameters.rate_hz, 0.01, 20.0);
    parameters.center_hz = clamp(parameters.center_hz, 20.0, sample_rate * 0.45);
    parameters.depth_octaves = clamp(parameters.depth_octaves, 0.0, 8.0);
    parameters.feedback = clamp(parameters.feedback, -0.95, 0.95);
    parameters.mix = clamp(parameters.mix, 0.0, 1.0);
    parameters.stereo_phase = clamp(parameters.stereo_phase, 0.0, 1.0);
    parameters.stages = parameters.stages.clamp(2, MAX_STAGES as u8);
    parameters
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameters() -> Parameters {
        Parameters {
            rate_hz: 1.25,
            center_hz: 900.0,
            depth_octaves: 2.5,
            feedback: 0.6,
            mix: 0.75,
            stereo_phase: 0.3,
            stages: 8,
        }
    }

    #[test]
    fn dry_mix_passes_input_exactly() {
        let mut settings = parameters();
        settings.mix = 0.0;
        let mut phaser = Phaser::new(48_000.0, settings);
        let input = [0.25, -0.75];
        assert_eq!(phaser.process_sample(input), input);
    }

    #[test]
    fn impulse_produces_a_stereo_tail() {
        let mut phaser = Phaser::new(48_000.0, parameters());
        let mut audio = [[0.0; 2]; 512];
        audio[0] = [1.0, 1.0];
        phaser.process_block(&mut audio);
        assert!(
            audio[1..]
                .iter()
                .flatten()
                .any(|sample| sample.abs() > 1.0e-4)
        );
        assert!(
            audio
                .iter()
                .any(|frame| (frame[0] - frame[1]).abs() > 1.0e-5)
        );
    }

    #[test]
    fn block_and_sample_paths_match() {
        let source = (0..257)
            .map(|index| {
                let value = ((index as f32) * 0.071).sin();
                [value, -value * 0.5]
            })
            .collect::<Vec<_>>();
        let mut by_sample = Phaser::new(48_000.0, parameters());
        let expected = source
            .iter()
            .map(|frame| by_sample.process_sample(*frame))
            .collect::<Vec<_>>();
        let mut actual = source;
        Phaser::new(48_000.0, parameters()).process_block(&mut actual);
        assert_eq!(actual, expected);
    }

    #[test]
    fn reset_repeats_the_same_render() {
        let mut phaser = Phaser::new(48_000.0, parameters());
        let input = [[0.4, -0.2]; 128];
        let first = input.map(|frame| phaser.process_sample(frame));
        phaser.reset();
        let second = input.map(|frame| phaser.process_sample(frame));
        assert_eq!(first, second);
    }

    #[test]
    fn invalid_controls_and_samples_stay_finite() {
        let settings = Parameters {
            rate_hz: f32::NAN,
            center_hz: f32::INFINITY,
            depth_octaves: f32::NEG_INFINITY,
            feedback: f32::NAN,
            mix: f32::INFINITY,
            stereo_phase: f32::NAN,
            stages: u8::MAX,
        };
        let mut phaser = Phaser::new(f32::NAN, settings);
        for _ in 0..512 {
            let output = phaser.process_sample([f32::NAN, f32::INFINITY]);
            assert!(output.into_iter().all(f32::is_finite));
        }
        let sanitized = phaser.parameters();
        assert_eq!(sanitized.rate_hz, 0.01);
        assert_eq!(sanitized.center_hz, 21_600.0);
        assert_eq!(sanitized.stages, 12);
    }
}
