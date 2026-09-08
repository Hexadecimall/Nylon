//! Level metering.
//!
//! Reports peak and root-mean-square level with the ballistics a mixer
//! needs: the peak rises at once and falls at a fixed rate, holding its
//! highest recent value, while the RMS follows a sliding average. Both are
//! reported as linear amplitude; the display converts to decibels.

/// Peak and RMS of one channel.
#[derive(Clone, Copy, Debug)]
pub struct Meter {
    peak: f32,
    hold: f32,
    hold_remaining: u32,
    hold_samples: u32,
    decay: f32,
    mean_square: f32,
    average_coefficient: f32,
    clipped: bool,
}

impl Meter {
    /// A meter with common ballistics: hold the peak for `hold_seconds`,
    /// then fall by `decay_db_per_second`, and average the RMS over
    /// `average_seconds`.
    #[must_use]
    pub fn new(
        sample_rate: f32,
        hold_seconds: f32,
        decay_db_per_second: f32,
        average_seconds: f32,
    ) -> Self {
        let sample_rate = if sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let decay_per_sample = -decay_db_per_second.abs() / sample_rate;
        Self {
            peak: 0.0,
            hold: 0.0,
            hold_remaining: 0,
            hold_samples: (hold_seconds.max(0.0) * sample_rate) as u32,
            decay: 10.0_f32.powf(decay_per_sample * 0.05),
            mean_square: 0.0,
            average_coefficient: if average_seconds > 0.0 {
                (-1.0 / (average_seconds * sample_rate)).exp()
            } else {
                0.0
            },
            clipped: false,
        }
    }

    /// A meter with the defaults a channel strip uses: a 1.5 second hold,
    /// 20 dB per second decay, and a 300 millisecond RMS window.
    #[must_use]
    pub fn with_defaults(sample_rate: f32) -> Self {
        Self::new(sample_rate, 1.5, 20.0, 0.3)
    }

    /// Clears every reading.
    pub fn reset(&mut self) {
        self.peak = 0.0;
        self.hold = 0.0;
        self.hold_remaining = 0;
        self.mean_square = 0.0;
        self.clipped = false;
    }

    /// Feeds one sample.
    #[inline]
    pub fn push(&mut self, sample: f32) {
        let magnitude = if sample.is_nan() { 0.0 } else { sample.abs() };
        if magnitude > 1.0 {
            self.clipped = true;
        }
        if magnitude >= self.hold {
            self.hold = magnitude;
            self.hold_remaining = self.hold_samples;
        } else if self.hold_remaining > 0 {
            self.hold_remaining -= 1;
        } else {
            self.hold *= self.decay;
        }
        self.peak = self.hold;
        let square = magnitude * magnitude;
        self.mean_square = square + (self.mean_square - square) * self.average_coefficient;
        self.mean_square = super::flush_denormal(self.mean_square);
    }

    /// Feeds a block.
    #[inline]
    pub fn push_block(&mut self, samples: &[f32]) {
        for sample in samples {
            self.push(*sample);
        }
    }

    /// Current peak as a linear amplitude.
    #[inline]
    #[must_use]
    pub const fn peak(&self) -> f32 {
        self.peak
    }

    /// Current RMS as a linear amplitude.
    #[inline]
    #[must_use]
    pub fn rms(&self) -> f32 {
        self.mean_square.max(0.0).sqrt()
    }

    /// True once a sample has exceeded full scale. Stays set until
    /// [`clear_clip`](Self::clear_clip).
    #[inline]
    #[must_use]
    pub const fn is_clipped(&self) -> bool {
        self.clipped
    }

    /// Clears the clip indicator, keeping the levels.
    #[inline]
    pub fn clear_clip(&mut self) {
        self.clipped = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    #[test]
    fn peak_rises_immediately() {
        let mut meter = Meter::with_defaults(RATE);
        assert_eq!(meter.peak(), 0.0);
        meter.push(0.5);
        assert_eq!(meter.peak(), 0.5);
        meter.push(-0.8);
        assert_eq!(meter.peak(), 0.8);
    }

    #[test]
    fn peak_holds_then_decays() {
        let mut meter = Meter::new(RATE, 0.01, 20.0, 0.3);
        meter.push(1.0);
        // Held for 10 ms.
        for _ in 0..400 {
            meter.push(0.0);
        }
        assert!((meter.peak() - 1.0).abs() < 1e-6);
        for _ in 0..200 {
            meter.push(0.0);
        }
        assert!(meter.peak() < 1.0);
        // 20 dB per second means a tenth after a second.
        for _ in 0..48_000 {
            meter.push(0.0);
        }
        assert!(meter.peak() < 0.2, "{}", meter.peak());
        assert!(meter.peak() > 0.0);
    }

    #[test]
    fn rms_of_a_sine_is_the_amplitude_over_root_two() {
        let mut meter = Meter::new(RATE, 0.0, 20.0, 0.05);
        let amplitude = 0.5_f32;
        for index in 0..48_000 {
            let phase = index as f32 / RATE * 1_000.0 * core::f32::consts::TAU;
            meter.push(amplitude * phase.sin());
        }
        let expected = amplitude * core::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (meter.rms() - expected).abs() < 0.02,
            "{} {expected}",
            meter.rms()
        );
    }

    #[test]
    fn rms_of_a_constant_is_the_constant() {
        let mut meter = Meter::new(RATE, 0.0, 20.0, 0.01);
        for _ in 0..48_000 {
            meter.push(0.25);
        }
        assert!((meter.rms() - 0.25).abs() < 1e-3, "{}", meter.rms());
    }

    #[test]
    fn rms_falls_back_toward_silence() {
        let mut meter = Meter::new(RATE, 0.0, 20.0, 0.01);
        meter.push_block(&[1.0; 4_800]);
        assert!(meter.rms() > 0.9);
        meter.push_block(&[0.0; 48_000]);
        assert!(meter.rms() < 1e-3, "{}", meter.rms());
    }

    #[test]
    fn clipping_latches_until_cleared() {
        let mut meter = Meter::with_defaults(RATE);
        meter.push(0.99);
        assert!(!meter.is_clipped());
        meter.push(1.5);
        assert!(meter.is_clipped());
        meter.push_block(&[0.0; 1_000]);
        assert!(meter.is_clipped());
        meter.clear_clip();
        assert!(!meter.is_clipped());
        meter.push(-2.0);
        assert!(meter.is_clipped());
    }

    #[test]
    fn reset_clears_everything() {
        let mut meter = Meter::with_defaults(RATE);
        meter.push_block(&[1.2; 100]);
        meter.reset();
        assert_eq!(meter.peak(), 0.0);
        assert_eq!(meter.rms(), 0.0);
        assert!(!meter.is_clipped());
    }

    #[test]
    fn readings_stay_finite_on_odd_input() {
        let mut meter = Meter::with_defaults(RATE);
        for sample in [f32::NAN, 0.0, -0.0, 1e-30, -1e-30] {
            meter.push(sample);
            assert!(meter.peak().is_finite());
            assert!(meter.rms().is_finite());
        }
    }

    #[test]
    fn block_and_sample_paths_agree() {
        let samples: [f32; 256] = core::array::from_fn(|i| (i as f32 * 0.05).sin() * 0.7);
        let mut one = Meter::with_defaults(RATE);
        for sample in &samples {
            one.push(*sample);
        }
        let mut many = Meter::with_defaults(RATE);
        many.push_block(&samples);
        assert_eq!(one.peak(), many.peak());
        assert_eq!(one.rms(), many.rms());
    }
}
