//! Second-order sections.
//!
//! Coefficients follow the audio filter cookbook forms, normalized so
//! `a0` is one. Filtering runs in transposed direct form II, which keeps
//! the state in the same range as the signal and behaves well when
//! coefficients change between samples.

use core::f64::consts::PI;

/// Filter shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Passes below the cutoff.
    LowPass,
    /// Passes above the cutoff.
    HighPass,
    /// Passes a band around the center, unity at the peak.
    BandPass,
    /// Rejects a band around the center.
    Notch,
    /// Flat magnitude, frequency-dependent phase.
    AllPass,
    /// Boosts or cuts a band, flat elsewhere.
    Peaking,
    /// Boosts or cuts below the corner.
    LowShelf,
    /// Boosts or cuts above the corner.
    HighShelf,
}

/// Normalized coefficients of one section.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coefficients {
    /// Feed-forward coefficients.
    pub b0: f64,
    /// Feed-forward coefficient for the first delayed input.
    pub b1: f64,
    /// Feed-forward coefficient for the second delayed input.
    pub b2: f64,
    /// Feedback coefficient for the first delayed output.
    pub a1: f64,
    /// Feedback coefficient for the second delayed output.
    pub a2: f64,
}

impl Default for Coefficients {
    fn default() -> Self {
        Self::identity()
    }
}

impl Coefficients {
    /// Passes the signal through unchanged.
    #[must_use]
    pub const fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }

    /// Designs a section. `frequency` is in hertz, `q` is the quality
    /// factor, and `gain_db` applies to the peaking and shelf shapes only.
    ///
    /// The frequency is clamped below Nyquist and `q` to a positive value,
    /// so no combination of inputs produces an unstable section.
    #[must_use]
    pub fn design(kind: Kind, frequency: f32, q: f32, gain_db: f32, sample_rate: f32) -> Self {
        if sample_rate <= 0.0 || sample_rate.is_nan() {
            return Self::identity();
        }
        let nyquist = f64::from(sample_rate) * 0.5;
        let frequency = if frequency.is_nan() {
            1.0
        } else {
            f64::from(frequency).clamp(1.0, nyquist * 0.995)
        };
        let q = if q.is_nan() {
            0.707
        } else {
            f64::from(q).clamp(0.001, 100.0)
        };
        let gain_db = if gain_db.is_nan() {
            0.0
        } else {
            f64::from(gain_db).clamp(-96.0, 96.0)
        };

        let omega = 2.0 * PI * frequency / f64::from(sample_rate);
        let (sin, cos) = omega.sin_cos();
        let alpha = sin / (2.0 * q);
        let amplitude = 10.0_f64.powf(gain_db / 40.0);

        let (b0, b1, b2, a0, a1, a2) = match kind {
            Kind::LowPass => {
                let b1 = 1.0 - cos;
                (b1 * 0.5, b1, b1 * 0.5, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
            }
            Kind::HighPass => {
                let b1 = -(1.0 + cos);
                (
                    (1.0 + cos) * 0.5,
                    b1,
                    (1.0 + cos) * 0.5,
                    1.0 + alpha,
                    -2.0 * cos,
                    1.0 - alpha,
                )
            }
            Kind::BandPass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            Kind::Notch => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            Kind::AllPass => (
                1.0 - alpha,
                -2.0 * cos,
                1.0 + alpha,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            Kind::Peaking => (
                1.0 + alpha * amplitude,
                -2.0 * cos,
                1.0 - alpha * amplitude,
                1.0 + alpha / amplitude,
                -2.0 * cos,
                1.0 - alpha / amplitude,
            ),
            Kind::LowShelf => {
                let root = 2.0 * amplitude.sqrt() * alpha;
                (
                    amplitude * ((amplitude + 1.0) - (amplitude - 1.0) * cos + root),
                    2.0 * amplitude * ((amplitude - 1.0) - (amplitude + 1.0) * cos),
                    amplitude * ((amplitude + 1.0) - (amplitude - 1.0) * cos - root),
                    (amplitude + 1.0) + (amplitude - 1.0) * cos + root,
                    -2.0 * ((amplitude - 1.0) + (amplitude + 1.0) * cos),
                    (amplitude + 1.0) + (amplitude - 1.0) * cos - root,
                )
            }
            Kind::HighShelf => {
                let root = 2.0 * amplitude.sqrt() * alpha;
                (
                    amplitude * ((amplitude + 1.0) + (amplitude - 1.0) * cos + root),
                    -2.0 * amplitude * ((amplitude - 1.0) + (amplitude + 1.0) * cos),
                    amplitude * ((amplitude + 1.0) + (amplitude - 1.0) * cos - root),
                    (amplitude + 1.0) - (amplitude - 1.0) * cos + root,
                    2.0 * ((amplitude - 1.0) - (amplitude + 1.0) * cos),
                    (amplitude + 1.0) - (amplitude - 1.0) * cos - root,
                )
            }
        };
        if a0.abs() < f64::EPSILON {
            return Self::identity();
        }
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Magnitude of the transfer function at `frequency`, as a linear
    /// ratio. Used by the tests and by an analyzer drawing a response
    /// curve; not called from the callback.
    #[must_use]
    pub fn magnitude_at(&self, frequency: f32, sample_rate: f32) -> f64 {
        if sample_rate <= 0.0 || sample_rate.is_nan() {
            return 1.0;
        }
        let omega = 2.0 * PI * f64::from(frequency) / f64::from(sample_rate);
        let (sin1, cos1) = omega.sin_cos();
        let (sin2, cos2) = (2.0 * omega).sin_cos();
        // Evaluate B(z)/A(z) on the unit circle with z = e^{-j omega}.
        let numerator_real = self.b0 + self.b1 * cos1 + self.b2 * cos2;
        let numerator_imaginary = -(self.b1 * sin1 + self.b2 * sin2);
        let denominator_real = 1.0 + self.a1 * cos1 + self.a2 * cos2;
        let denominator_imaginary = -(self.a1 * sin1 + self.a2 * sin2);
        let numerator =
            (numerator_real * numerator_real + numerator_imaginary * numerator_imaginary).sqrt();
        let denominator = (denominator_real * denominator_real
            + denominator_imaginary * denominator_imaginary)
            .sqrt();
        if denominator < f64::EPSILON {
            f64::INFINITY
        } else {
            numerator / denominator
        }
    }

    /// True when both poles lie inside the unit circle.
    #[must_use]
    pub fn is_stable(&self) -> bool {
        self.a2.abs() < 1.0 && self.a1.abs() < 1.0 + self.a2
    }
}

/// One second-order section with its own state.
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    coefficients: Coefficients,
    z1: f64,
    z2: f64,
}

impl Biquad {
    /// A section with the given coefficients and cleared state.
    #[must_use]
    pub const fn new(coefficients: Coefficients) -> Self {
        Self {
            coefficients,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// Replaces the coefficients, keeping the state so the change does not
    /// click.
    #[inline]
    pub fn set_coefficients(&mut self, coefficients: Coefficients) {
        self.coefficients = coefficients;
    }

    /// Current coefficients.
    #[inline]
    #[must_use]
    pub const fn coefficients(&self) -> Coefficients {
        self.coefficients
    }

    /// Clears the state, as when playback restarts.
    #[inline]
    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    /// Filters one sample.
    #[inline]
    pub fn process(&mut self, input: f32) -> f32 {
        let c = &self.coefficients;
        let x = f64::from(input);
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        super::flush_denormal(y as f32)
    }

    /// Filters a block in place.
    #[inline]
    pub fn process_block(&mut self, samples: &mut [f32]) {
        for sample in samples {
            *sample = self.process(*sample);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// Measured magnitude response, for checking the analytic one.
    fn measure(coefficients: Coefficients, frequency: f32) -> f64 {
        let mut filter = Biquad::new(coefficients);
        let period = f64::from(RATE) / f64::from(frequency);
        let settle = (period * 200.0) as usize;
        for index in 0..settle {
            let phase = 2.0 * PI * index as f64 / period;
            filter.process(phase.sin() as f32);
        }
        let mut peak = 0.0_f64;
        for index in settle..settle + (period * 20.0) as usize {
            let phase = 2.0 * PI * index as f64 / period;
            peak = peak.max(f64::from(filter.process(phase.sin() as f32)).abs());
        }
        peak
    }

    #[test]
    fn identity_passes_the_signal() {
        let mut filter = Biquad::new(Coefficients::identity());
        for value in [-1.0, -0.25, 0.0, 0.5, 1.0] {
            assert_eq!(filter.process(value), value);
        }
    }

    #[test]
    fn low_pass_response_matches_theory() {
        let c = Coefficients::design(
            Kind::LowPass,
            1_000.0,
            core::f32::consts::FRAC_1_SQRT_2,
            0.0,
            RATE,
        );
        assert!(c.is_stable());
        // Unity at DC, -3 dB at the cutoff, and far down at Nyquist.
        assert!((c.magnitude_at(0.0, RATE) - 1.0).abs() < 1e-9);
        let at_cutoff = c.magnitude_at(1_000.0, RATE);
        assert!(
            (at_cutoff - core::f64::consts::FRAC_1_SQRT_2).abs() < 1e-3,
            "{at_cutoff}"
        );
        assert!(c.magnitude_at(24_000.0, RATE) < 1e-3);
        // Two poles give 12 dB per octave: an octave up is about a quarter.
        let one_octave = c.magnitude_at(2_000.0, RATE);
        let two_octaves = c.magnitude_at(4_000.0, RATE);
        assert!(
            (one_octave / two_octaves - 4.0).abs() < 0.5,
            "{one_octave} {two_octaves}"
        );
    }

    #[test]
    fn analytic_response_matches_a_measured_sine() {
        let c = Coefficients::design(Kind::LowPass, 1_000.0, 0.707, 0.0, RATE);
        for frequency in [200.0, 1_000.0, 4_000.0] {
            let predicted = c.magnitude_at(frequency, RATE);
            let measured = measure(c, frequency);
            assert!(
                (predicted - measured).abs() < 0.02,
                "{frequency}: predicted {predicted}, measured {measured}"
            );
        }
    }

    #[test]
    fn high_pass_mirrors_the_low_pass() {
        let c = Coefficients::design(
            Kind::HighPass,
            1_000.0,
            core::f32::consts::FRAC_1_SQRT_2,
            0.0,
            RATE,
        );
        assert!(c.is_stable());
        assert!(c.magnitude_at(0.0, RATE) < 1e-9);
        assert!((c.magnitude_at(1_000.0, RATE) - core::f64::consts::FRAC_1_SQRT_2).abs() < 1e-3);
        assert!((c.magnitude_at(24_000.0, RATE) - 1.0).abs() < 1e-2);
    }

    #[test]
    fn band_pass_and_notch_are_complementary_at_the_center() {
        let band = Coefficients::design(Kind::BandPass, 2_000.0, 2.0, 0.0, RATE);
        let notch = Coefficients::design(Kind::Notch, 2_000.0, 2.0, 0.0, RATE);
        assert!((band.magnitude_at(2_000.0, RATE) - 1.0).abs() < 1e-6);
        assert!(band.magnitude_at(0.0, RATE) < 1e-6);
        assert!(notch.magnitude_at(2_000.0, RATE) < 1e-6);
        assert!((notch.magnitude_at(0.0, RATE) - 1.0).abs() < 1e-6);
        assert!((notch.magnitude_at(24_000.0, RATE) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn all_pass_is_flat() {
        let c = Coefficients::design(Kind::AllPass, 1_000.0, 0.707, 0.0, RATE);
        for frequency in [10.0, 100.0, 1_000.0, 10_000.0, 20_000.0] {
            assert!(
                (c.magnitude_at(frequency, RATE) - 1.0).abs() < 1e-6,
                "{frequency}"
            );
        }
    }

    #[test]
    fn peaking_reaches_its_gain_and_is_flat_away_from_center() {
        for gain_db in [-12.0, -6.0, 6.0, 12.0] {
            let c = Coefficients::design(Kind::Peaking, 1_000.0, 1.0, gain_db, RATE);
            assert!(c.is_stable());
            let peak = 20.0 * c.magnitude_at(1_000.0, RATE).log10();
            assert!(
                (peak - f64::from(gain_db)).abs() < 1e-6,
                "{gain_db}: {peak}"
            );
            assert!((c.magnitude_at(10.0, RATE) - 1.0).abs() < 0.01);
            assert!((c.magnitude_at(23_000.0, RATE) - 1.0).abs() < 0.01);
        }
    }

    #[test]
    fn shelves_reach_their_gain_on_the_correct_side() {
        let low = Coefficients::design(Kind::LowShelf, 1_000.0, 0.707, 12.0, RATE);
        assert!((20.0 * low.magnitude_at(10.0, RATE).log10() - 12.0).abs() < 0.1);
        assert!((low.magnitude_at(23_000.0, RATE) - 1.0).abs() < 0.05);
        let high = Coefficients::design(Kind::HighShelf, 1_000.0, 0.707, -12.0, RATE);
        assert!((20.0 * high.magnitude_at(23_000.0, RATE).log10() + 12.0).abs() < 0.1);
        assert!((high.magnitude_at(10.0, RATE) - 1.0).abs() < 0.05);
    }

    #[test]
    fn every_shape_is_stable_across_the_parameter_range() {
        let kinds = [
            Kind::LowPass,
            Kind::HighPass,
            Kind::BandPass,
            Kind::Notch,
            Kind::AllPass,
            Kind::Peaking,
            Kind::LowShelf,
            Kind::HighShelf,
        ];
        for kind in kinds {
            for frequency in [1.0, 20.0, 1_000.0, 20_000.0, 23_999.0, 48_000.0, -5.0] {
                for q in [0.1, 0.707, 10.0, 100.0, 0.0, -1.0] {
                    for gain in [-24.0, 0.0, 24.0] {
                        let c = Coefficients::design(kind, frequency, q, gain, RATE);
                        assert!(c.is_stable(), "{kind:?} {frequency} {q} {gain}");
                        let mut filter = Biquad::new(c);
                        for index in 0..2_000 {
                            let value = filter.process(if index == 0 { 1.0 } else { 0.0 });
                            assert!(value.is_finite(), "{kind:?} {frequency} {q} {gain}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn nonsense_inputs_fall_back_to_identity() {
        assert_eq!(
            Coefficients::design(Kind::LowPass, 1_000.0, 1.0, 0.0, 0.0),
            Coefficients::identity()
        );
        assert_eq!(
            Coefficients::design(Kind::LowPass, 1_000.0, 1.0, 0.0, -1.0),
            Coefficients::identity()
        );
        let c = Coefficients::design(Kind::LowPass, f32::NAN, f32::NAN, f32::NAN, RATE);
        assert!(c.is_stable());
        assert_eq!(Coefficients::identity().magnitude_at(1_000.0, 0.0), 1.0);
    }

    #[test]
    fn reset_clears_the_tail() {
        let c = Coefficients::design(Kind::LowPass, 500.0, 0.707, 0.0, RATE);
        let mut filter = Biquad::new(c);
        filter.process(1.0);
        assert!(filter.process(0.0) != 0.0);
        filter.reset();
        assert_eq!(filter.process(0.0), 0.0);
    }

    #[test]
    fn block_processing_matches_sample_processing() {
        let c = Coefficients::design(Kind::HighPass, 800.0, 1.2, 0.0, RATE);
        let input: [f32; 64] = core::array::from_fn(|i| (i as f32 * 0.1).sin());
        let mut one = Biquad::new(c);
        let mut expected = input;
        for sample in &mut expected {
            *sample = one.process(*sample);
        }
        let mut many = Biquad::new(c);
        let mut actual = input;
        many.process_block(&mut actual);
        assert_eq!(actual, expected);
    }
}
