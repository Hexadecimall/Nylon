//! Stereo-linked noise gate with hysteresis and hold timing.

use super::db;

/// User-facing gate settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parameters {
    /// Level that opens the gate, in decibels.
    pub threshold_db: f32,
    /// Distance below the opening threshold that closes the gate.
    pub hysteresis_db: f32,
    /// Time to reach full gain after opening.
    pub attack_seconds: f32,
    /// Minimum time the gate remains open after the detector falls.
    pub hold_seconds: f32,
    /// Time to reach silence after closing.
    pub release_seconds: f32,
    /// Selects the sidechain samples as the detector signal.
    pub external_sidechain: bool,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            threshold_db: -40.0,
            hysteresis_db: 6.0,
            attack_seconds: 0.001,
            hold_seconds: 0.05,
            release_seconds: 0.1,
            external_sidechain: false,
        }
    }
}

impl Parameters {
    fn sanitized(self) -> Self {
        Self {
            threshold_db: super::clamp(self.threshold_db, -96.0, 0.0),
            hysteresis_db: super::clamp(self.hysteresis_db, 0.0, 48.0),
            attack_seconds: super::clamp(self.attack_seconds, 0.0, 10.0),
            hold_seconds: super::clamp(self.hold_seconds, 0.0, 10.0),
            release_seconds: super::clamp(self.release_seconds, 0.0, 30.0),
            ..self
        }
    }
}

/// Gate state. All timing storage is prepared at construction.
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    parameters: Parameters,
    open_threshold: f32,
    close_threshold: f32,
    attack_coefficient: f32,
    release_coefficient: f32,
    hold_frames: u32,
    hold_remaining: u32,
    gain: f32,
    open: bool,
}

impl Gate {
    /// Creates a closed gate. Invalid numbers are clamped and an invalid
    /// sample rate falls back to 48 kHz.
    #[must_use]
    pub fn new(sample_rate: f32, parameters: Parameters) -> Self {
        let mut gate = Self {
            parameters: Parameters::default(),
            open_threshold: 0.0,
            close_threshold: 0.0,
            attack_coefficient: 0.0,
            release_coefficient: 0.0,
            hold_frames: 0,
            hold_remaining: 0,
            gain: 0.0,
            open: false,
        };
        gate.set_parameters(sample_rate, parameters);
        gate
    }

    /// Changes settings while preserving the gain and open state.
    pub fn set_parameters(&mut self, sample_rate: f32, parameters: Parameters) {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        self.parameters = parameters.sanitized();
        self.open_threshold = db::to_linear(self.parameters.threshold_db);
        self.close_threshold =
            db::to_linear(self.parameters.threshold_db - self.parameters.hysteresis_db);
        self.attack_coefficient = time_coefficient(self.parameters.attack_seconds, sample_rate);
        self.release_coefficient = time_coefficient(self.parameters.release_seconds, sample_rate);
        self.hold_frames = seconds_to_frames(self.parameters.hold_seconds, sample_rate);
        self.hold_remaining = self.hold_remaining.min(self.hold_frames);
    }

    #[must_use]
    pub const fn parameters(&self) -> Parameters {
        self.parameters
    }

    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    #[must_use]
    pub const fn gain(&self) -> f32 {
        self.gain
    }

    /// Closes the gate and clears its envelope and hold timer.
    pub fn reset(&mut self) {
        self.hold_remaining = 0;
        self.gain = 0.0;
        self.open = false;
    }

    /// Processes one stereo sample and its optional external detector sample.
    #[inline]
    #[must_use]
    pub fn process_stereo(
        &mut self,
        left: f32,
        right: f32,
        sidechain_left: f32,
        sidechain_right: f32,
    ) -> (f32, f32) {
        let left = finite(left);
        let right = finite(right);
        let detector = if self.parameters.external_sidechain {
            finite(sidechain_left)
                .abs()
                .max(finite(sidechain_right).abs())
        } else {
            left.abs().max(right.abs())
        };
        self.update_state(detector);
        let target = f32::from(u8::from(self.open));
        let coefficient = if self.open {
            self.attack_coefficient
        } else {
            self.release_coefficient
        };
        self.gain = super::flush_denormal(target + coefficient * (self.gain - target));
        (left * self.gain, right * self.gain)
    }

    /// Processes paired stereo buffers in place. A missing sidechain is silence.
    #[inline]
    pub fn process_block(&mut self, audio: &mut [[f32; 2]], sidechain: &[[f32; 2]]) {
        for (index, frame) in audio.iter_mut().enumerate() {
            let detector = sidechain.get(index).copied().unwrap_or([0.0; 2]);
            (frame[0], frame[1]) =
                self.process_stereo(frame[0], frame[1], detector[0], detector[1]);
        }
    }

    #[inline]
    fn update_state(&mut self, detector: f32) {
        if detector >= self.open_threshold {
            self.open = true;
            self.hold_remaining = self.hold_frames;
        } else if self.open && detector < self.close_threshold {
            if self.hold_remaining == 0 {
                self.open = false;
            } else {
                self.hold_remaining -= 1;
            }
        } else if self.open {
            self.hold_remaining = self.hold_frames;
        }
    }
}

fn time_coefficient(seconds: f32, sample_rate: f32) -> f32 {
    if seconds == 0.0 {
        0.0
    } else {
        (-1.0 / (seconds * sample_rate)).exp()
    }
}

fn seconds_to_frames(seconds: f32, sample_rate: f32) -> u32 {
    (seconds * sample_rate).round().min(u32::MAX as f32) as u32
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 1_000.0;

    fn immediate(external_sidechain: bool) -> Parameters {
        Parameters {
            threshold_db: -20.0,
            hysteresis_db: 6.0,
            attack_seconds: 0.0,
            hold_seconds: 0.0,
            release_seconds: 0.0,
            external_sidechain,
        }
    }

    #[test]
    fn threshold_opens_and_hysteresis_prevents_chatter() {
        let mut gate = Gate::new(RATE, immediate(false));
        assert_eq!(gate.process_stereo(0.2, 0.1, 0.0, 0.0), (0.2, 0.1));
        assert!(gate.is_open());
        let between = db::to_linear(-23.0);
        assert_eq!(
            gate.process_stereo(between, between, 0.0, 0.0),
            (between, between)
        );
        assert!(gate.is_open());
        assert_eq!(gate.process_stereo(0.01, -0.01, 0.0, 0.0), (0.0, -0.0));
        assert!(!gate.is_open());
    }

    #[test]
    fn hold_delays_closing_by_the_requested_frames() {
        let mut gate = Gate::new(
            RATE,
            Parameters {
                hold_seconds: 0.003,
                ..immediate(false)
            },
        );
        let _ = gate.process_stereo(1.0, 1.0, 0.0, 0.0);
        for _ in 0..3 {
            let _ = gate.process_stereo(0.001, 0.001, 0.0, 0.0);
            assert!(gate.is_open());
        }
        let _ = gate.process_stereo(0.001, 0.001, 0.0, 0.0);
        assert!(!gate.is_open());
    }

    #[test]
    fn attack_and_release_move_in_the_expected_directions() {
        let mut gate = Gate::new(
            RATE,
            Parameters {
                attack_seconds: 0.01,
                release_seconds: 0.02,
                ..immediate(false)
            },
        );
        let first = gate.process_stereo(1.0, 1.0, 0.0, 0.0).0;
        let second = gate.process_stereo(1.0, 1.0, 0.0, 0.0).0;
        assert!(first > 0.0 && second > first && second < 1.0);
        let before_release = gate.gain();
        let _ = gate.process_stereo(0.001, 0.001, 0.0, 0.0);
        assert!(gate.gain() < before_release && gate.gain() > 0.0);
    }

    #[test]
    fn external_sidechain_opens_without_replacing_audio() {
        let mut gate = Gate::new(RATE, immediate(true));
        assert_eq!(gate.process_stereo(0.02, -0.01, 1.0, 0.5), (0.02, -0.01));
        assert_eq!(gate.process_stereo(1.0, -1.0, 0.0, 0.0), (0.0, -0.0));
    }

    #[test]
    fn block_and_sample_processing_match() {
        let settings = Parameters::default();
        let mut block_gate = Gate::new(48_000.0, settings);
        let mut sample_gate = block_gate;
        let mut block = [[0.01, -0.01], [0.5, -0.25], [0.02, -0.02], [0.0, 0.0]];
        let sidechain = [[0.0; 2]; 4];
        let mut expected = block;
        for (frame, sidechain) in expected.iter_mut().zip(sidechain) {
            (frame[0], frame[1]) =
                sample_gate.process_stereo(frame[0], frame[1], sidechain[0], sidechain[1]);
        }
        block_gate.process_block(&mut block, &sidechain);
        assert_eq!(block, expected);
    }

    #[test]
    fn invalid_values_are_sanitized_and_samples_remain_finite() {
        let mut gate = Gate::new(
            f32::NAN,
            Parameters {
                threshold_db: f32::NAN,
                hysteresis_db: f32::INFINITY,
                attack_seconds: f32::NEG_INFINITY,
                hold_seconds: f32::NAN,
                release_seconds: f32::INFINITY,
                external_sidechain: true,
            },
        );
        assert_eq!(gate.parameters().threshold_db, -96.0);
        assert_eq!(gate.parameters().hysteresis_db, 48.0);
        let output = gate.process_stereo(f32::NAN, f32::INFINITY, f32::NAN, f32::INFINITY);
        assert!(output.0.is_finite() && output.1.is_finite());
    }

    #[test]
    fn reset_returns_to_the_closed_initial_state() {
        let mut gate = Gate::new(RATE, Parameters::default());
        let _ = gate.process_stereo(1.0, 1.0, 0.0, 0.0);
        gate.reset();
        let fresh = Gate::new(RATE, Parameters::default());
        assert_eq!(gate.is_open(), fresh.is_open());
        assert_eq!(gate.gain(), fresh.gain());
    }
}
