//! Stereo resonant filter with envelope and low-frequency modulation.

use core::f32::consts::{PI, TAU};

use crate::dsp::{clamp, flush_denormal};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parameters {
    pub mode: Mode,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub drive_db: f32,
    pub envelope_amount_octaves: f32,
    pub envelope_attack_seconds: f32,
    pub envelope_release_seconds: f32,
    pub lfo_rate_hz: f32,
    pub lfo_amount_octaves: f32,
    pub mix: f32,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            mode: Mode::LowPass,
            cutoff_hz: 1_000.0,
            resonance: 0.707,
            drive_db: 0.0,
            envelope_amount_octaves: 0.0,
            envelope_attack_seconds: 0.01,
            envelope_release_seconds: 0.1,
            lfo_rate_hz: 0.0,
            lfo_amount_octaves: 0.0,
            mix: 1.0,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct State {
    integrator_one: f32,
    integrator_two: f32,
}

pub struct AutoFilter {
    sample_rate: f32,
    parameters: Parameters,
    state: [State; 2],
    envelope: f32,
    phase: f32,
    attack_coefficient: f32,
    release_coefficient: f32,
}

impl AutoFilter {
    #[must_use]
    pub fn new(sample_rate: f32, parameters: Parameters) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let parameters = sanitize(parameters, sample_rate);
        let mut result = Self {
            sample_rate,
            parameters,
            state: [State::default(); 2],
            envelope: 0.0,
            phase: 0.0,
            attack_coefficient: 0.0,
            release_coefficient: 0.0,
        };
        result.update_coefficients();
        result
    }

    #[must_use]
    pub fn parameters(&self) -> Parameters {
        self.parameters
    }

    pub fn set_parameters(&mut self, parameters: Parameters) {
        self.parameters = sanitize(parameters, self.sample_rate);
        self.update_coefficients();
    }

    pub fn reset(&mut self) {
        self.state = [State::default(); 2];
        self.envelope = 0.0;
        self.phase = 0.0;
    }

    #[inline]
    #[must_use]
    pub fn process_sample(&mut self, input: [f32; 2], sidechain: [f32; 2]) -> [f32; 2] {
        let input = input.map(finite_or_zero);
        let sidechain = sidechain.map(finite_or_zero);
        let detector = sidechain[0].abs().max(sidechain[1].abs());
        let coefficient = if detector > self.envelope {
            self.attack_coefficient
        } else {
            self.release_coefficient
        };
        self.envelope = flush_denormal(detector + coefficient * (self.envelope - detector));

        let modulation = self.parameters.envelope_amount_octaves * self.envelope
            + self.parameters.lfo_amount_octaves * self.phase.sin();
        let cutoff = clamp(
            self.parameters.cutoff_hz * modulation.exp2(),
            10.0,
            self.sample_rate * 0.45,
        );
        let g = (PI * cutoff / self.sample_rate).tan();
        let k = 1.0 / self.parameters.resonance;
        let denominator = 1.0 / (1.0 + g * (g + k));
        let a2 = g * denominator;
        let a3 = g * a2;
        let drive = 10.0_f32.powf(self.parameters.drive_db / 20.0);
        let normalization = drive.tanh().recip();
        let mut output = [0.0; 2];
        for channel in 0..2 {
            let driven = (input[channel] * drive).tanh() * normalization;
            let state = &mut self.state[channel];
            let v3 = driven - state.integrator_two;
            let band = denominator * state.integrator_one + a2 * v3;
            let low = state.integrator_two + a2 * state.integrator_one + a3 * v3;
            state.integrator_one = flush_denormal(2.0 * band - state.integrator_one);
            state.integrator_two = flush_denormal(2.0 * low - state.integrator_two);
            let high = driven - k * band - low;
            let wet = match self.parameters.mode {
                Mode::LowPass => low,
                Mode::HighPass => high,
                Mode::BandPass => band,
                Mode::Notch => low + high,
            };
            output[channel] =
                flush_denormal(input[channel] + (wet - input[channel]) * self.parameters.mix);
        }
        self.phase += TAU * self.parameters.lfo_rate_hz / self.sample_rate;
        if self.phase >= TAU {
            self.phase -= TAU;
        }
        output
    }

    pub fn process_block(&mut self, audio: &mut [[f32; 2]], sidechain: Option<&[[f32; 2]]>) {
        for (index, frame) in audio.iter_mut().enumerate() {
            let detector = sidechain
                .and_then(|frames| frames.get(index))
                .copied()
                .unwrap_or(*frame);
            *frame = self.process_sample(*frame, detector);
        }
    }

    fn update_coefficients(&mut self) {
        self.attack_coefficient =
            time_coefficient(self.parameters.envelope_attack_seconds, self.sample_rate);
        self.release_coefficient =
            time_coefficient(self.parameters.envelope_release_seconds, self.sample_rate);
    }
}

fn sanitize(mut parameters: Parameters, sample_rate: f32) -> Parameters {
    parameters.cutoff_hz = clamp(parameters.cutoff_hz, 10.0, sample_rate * 0.45);
    parameters.resonance = clamp(parameters.resonance, 0.1, 20.0);
    parameters.drive_db = clamp(parameters.drive_db, 0.0, 36.0);
    parameters.envelope_amount_octaves = clamp(parameters.envelope_amount_octaves, -8.0, 8.0);
    parameters.envelope_attack_seconds = clamp(parameters.envelope_attack_seconds, 0.0, 10.0);
    parameters.envelope_release_seconds = clamp(parameters.envelope_release_seconds, 0.0, 10.0);
    parameters.lfo_rate_hz = clamp(parameters.lfo_rate_hz, 0.0, 40.0);
    parameters.lfo_amount_octaves = clamp(parameters.lfo_amount_octaves, 0.0, 8.0);
    parameters.mix = clamp(parameters.mix, 0.0, 1.0);
    parameters
}

fn time_coefficient(seconds: f32, sample_rate: f32) -> f32 {
    if seconds <= 0.0 {
        0.0
    } else {
        (-1.0 / (seconds * sample_rate)).exp()
    }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameters() -> Parameters {
        Parameters {
            cutoff_hz: 800.0,
            resonance: 0.8,
            drive_db: 6.0,
            envelope_amount_octaves: 2.0,
            envelope_attack_seconds: 0.002,
            envelope_release_seconds: 0.05,
            lfo_rate_hz: 1.5,
            lfo_amount_octaves: 0.75,
            mix: 0.8,
            ..Parameters::default()
        }
    }

    #[test]
    fn dry_mix_passes_input_exactly() {
        let mut settings = parameters();
        settings.mix = 0.0;
        let mut filter = AutoFilter::new(48_000.0, settings);
        let input = [0.25, -0.75];
        assert_eq!(filter.process_sample(input, [1.0, 1.0]), input);
    }

    #[test]
    fn low_pass_rejects_a_nyquist_pattern() {
        let settings = Parameters {
            cutoff_hz: 200.0,
            ..Parameters::default()
        };
        let mut filter = AutoFilter::new(48_000.0, settings);
        let mut peak = 0.0_f32;
        for index in 0..12_000 {
            let value = if index & 1 == 0 { 1.0 } else { -1.0 };
            let output = filter.process_sample([value; 2], [value; 2]);
            if index > 6_000 {
                peak = peak.max(output[0].abs());
            }
        }
        assert!(peak < 0.01, "{peak}");
    }

    #[test]
    fn block_and_sample_paths_match() {
        let source = (0..257)
            .map(|index| {
                let value = ((index as f32) * 0.071).sin();
                [value, -value * 0.5]
            })
            .collect::<Vec<_>>();
        let sidechain = (0..257)
            .map(|index| [((index as f32) * 0.017).cos().abs(); 2])
            .collect::<Vec<_>>();
        let mut by_sample = AutoFilter::new(48_000.0, parameters());
        let expected = source
            .iter()
            .zip(&sidechain)
            .map(|(input, detector)| by_sample.process_sample(*input, *detector))
            .collect::<Vec<_>>();
        let mut actual = source;
        AutoFilter::new(48_000.0, parameters()).process_block(&mut actual, Some(&sidechain));
        assert_eq!(actual, expected);
    }

    #[test]
    fn reset_repeats_the_same_render() {
        let mut filter = AutoFilter::new(48_000.0, parameters());
        let input = [[0.4, -0.2]; 128];
        let first = input.map(|frame| filter.process_sample(frame, [0.7; 2]));
        filter.reset();
        let second = input.map(|frame| filter.process_sample(frame, [0.7; 2]));
        assert_eq!(first, second);
    }

    #[test]
    fn invalid_controls_and_samples_stay_finite() {
        let settings = Parameters {
            cutoff_hz: f32::NAN,
            resonance: f32::INFINITY,
            drive_db: f32::NEG_INFINITY,
            envelope_amount_octaves: f32::NAN,
            envelope_attack_seconds: -1.0,
            envelope_release_seconds: f32::INFINITY,
            lfo_rate_hz: f32::NAN,
            lfo_amount_octaves: f32::INFINITY,
            mix: f32::NAN,
            ..Parameters::default()
        };
        let mut filter = AutoFilter::new(f32::NAN, settings);
        for _ in 0..512 {
            let output =
                filter.process_sample([f32::NAN, f32::INFINITY], [f32::NEG_INFINITY, f32::NAN]);
            assert!(output.into_iter().all(f32::is_finite));
        }
        let sanitized = filter.parameters();
        assert_eq!(sanitized.cutoff_hz, 10.0);
        assert_eq!(sanitized.resonance, 20.0);
        assert_eq!(sanitized.mix, 0.0);
    }
}
