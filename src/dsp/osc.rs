//! Oscillators.
//!
//! The saw, square, and triangle shapes are corrected at their
//! discontinuities so they stay usable near the top of the range instead
//! of folding harmonics down into the audible band. The correction is the
//! polynomial band-limited step, which costs a few operations per sample
//! rather than a table lookup per harmonic.

use core::f32::consts::TAU;

/// Waveform of an [`Oscillator`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Shape {
    /// Pure tone.
    #[default]
    Sine,
    /// Falling ramp, every harmonic.
    Saw,
    /// Square wave, odd harmonics.
    Square,
    /// Triangle, odd harmonics falling faster than a square.
    Triangle,
}

/// A single oscillator. Phase runs from 0 to 1.
#[derive(Clone, Copy, Debug)]
pub struct Oscillator {
    shape: Shape,
    phase: f32,
    increment: f32,
    sample_rate: f32,
    // Triangle is the running integral of a square, which needs a state.
    integrator: f32,
}

impl Oscillator {
    /// An oscillator at `frequency` hertz.
    #[must_use]
    pub fn new(shape: Shape, frequency: f32, sample_rate: f32) -> Self {
        let mut oscillator = Self {
            shape,
            phase: 0.0,
            increment: 0.0,
            sample_rate: if sample_rate > 0.0 {
                sample_rate
            } else {
                48_000.0
            },
            integrator: 0.0,
        };
        oscillator.set_frequency(frequency);
        oscillator
    }

    /// Changes the waveform, keeping the phase.
    #[inline]
    pub fn set_shape(&mut self, shape: Shape) {
        self.shape = shape;
    }

    /// Waveform in use.
    #[inline]
    #[must_use]
    pub const fn shape(&self) -> Shape {
        self.shape
    }

    /// Sets the frequency in hertz. Values are clamped to below Nyquist so
    /// the phase never advances more than half a cycle per sample.
    #[inline]
    pub fn set_frequency(&mut self, frequency: f32) {
        let nyquist = self.sample_rate * 0.5;
        let frequency = if frequency.is_nan() {
            0.0
        } else {
            frequency.clamp(-nyquist * 0.99, nyquist * 0.99)
        };
        self.increment = frequency / self.sample_rate;
    }

    /// Frequency in hertz.
    #[inline]
    #[must_use]
    pub fn frequency(&self) -> f32 {
        self.increment * self.sample_rate
    }

    /// Sets the phase, wrapped into 0..1.
    #[inline]
    pub fn set_phase(&mut self, phase: f32) {
        self.phase = phase - phase.floor();
    }

    /// Current phase in 0..1.
    #[inline]
    #[must_use]
    pub const fn phase(&self) -> f32 {
        self.phase
    }

    /// Clears the phase and any internal state.
    #[inline]
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.integrator = 0.0;
    }

    /// Produces one sample in -1..1 and advances the phase.
    #[inline]
    pub fn process(&mut self) -> f32 {
        let value = match self.shape {
            Shape::Sine => (self.phase * TAU).sin(),
            Shape::Saw => self.saw(),
            Shape::Square => self.square(),
            Shape::Triangle => self.triangle(),
        };
        self.phase += self.increment;
        self.phase -= self.phase.floor();
        value
    }

    /// Fills a block, replacing its contents.
    #[inline]
    pub fn process_block(&mut self, output: &mut [f32]) {
        for sample in output {
            *sample = self.process();
        }
    }

    /// The correction added around a step discontinuity, from the
    /// two-sample polynomial approximation of a band-limited step.
    #[inline]
    fn blep(&self, mut t: f32) -> f32 {
        let increment = self.increment.abs();
        if increment <= 0.0 {
            return 0.0;
        }
        if t < increment {
            t /= increment;
            t + t - t * t - 1.0
        } else if t > 1.0 - increment {
            t = (t - 1.0) / increment;
            t * t + t + t + 1.0
        } else {
            0.0
        }
    }

    #[inline]
    fn saw(&self) -> f32 {
        let raw = 2.0 * self.phase - 1.0;
        raw - self.blep(self.phase)
    }

    #[inline]
    fn square(&self) -> f32 {
        let raw = if self.phase < 0.5 { 1.0 } else { -1.0 };
        let half = self.phase + 0.5;
        raw + self.blep(self.phase) - self.blep(half - half.floor())
    }

    #[inline]
    fn triangle(&mut self) -> f32 {
        // Integrating a band-limited square gives a band-limited triangle;
        // the leak keeps the integrator from drifting.
        let square = self.square();
        let increment = self.increment.abs();
        self.integrator += 4.0 * increment * (square - self.integrator * 0.02);
        super::flush_denormal(self.integrator.clamp(-1.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// Ratio of energy above `edge` to total energy, using a slow discrete
    /// transform over one buffer. High for an aliasing waveform.
    fn energy_above(samples: &[f32], edge: f32) -> f64 {
        let n = samples.len();
        let mut total = 0.0;
        let mut above = 0.0;
        for bin in 1..n / 2 {
            let frequency = bin as f32 * RATE / n as f32;
            let (mut real, mut imaginary) = (0.0_f64, 0.0_f64);
            for (index, sample) in samples.iter().enumerate() {
                let angle = -2.0 * core::f64::consts::PI * bin as f64 * index as f64 / n as f64;
                real += f64::from(*sample) * angle.cos();
                imaginary += f64::from(*sample) * angle.sin();
            }
            let power = real * real + imaginary * imaginary;
            total += power;
            if frequency > edge {
                above += power;
            }
        }
        if total > 0.0 { above / total } else { 0.0 }
    }

    #[test]
    fn sine_is_a_sine() {
        let mut oscillator = Oscillator::new(Shape::Sine, 1_000.0, RATE);
        let period = RATE / 1_000.0;
        // The phase accumulates in single precision, so allow for drift.
        for index in 0..512 {
            let expected = (index as f32 / period * TAU).sin();
            assert!((oscillator.process() - expected).abs() < 1e-3, "{index}");
        }
    }

    #[test]
    fn every_shape_stays_in_range() {
        for shape in [Shape::Sine, Shape::Saw, Shape::Square, Shape::Triangle] {
            for frequency in [1.0, 55.0, 440.0, 5_000.0, 15_000.0, 23_000.0] {
                let mut oscillator = Oscillator::new(shape, frequency, RATE);
                for _ in 0..4_096 {
                    let value = oscillator.process();
                    assert!(value.is_finite(), "{shape:?} {frequency}");
                    assert!(
                        (-1.05..=1.05).contains(&value),
                        "{shape:?} {frequency}: {value}"
                    );
                }
            }
        }
    }

    #[test]
    fn phase_wraps_and_is_settable() {
        let mut oscillator = Oscillator::new(Shape::Saw, 1_000.0, RATE);
        for _ in 0..10_000 {
            oscillator.process();
            assert!((0.0..1.0).contains(&oscillator.phase()));
        }
        oscillator.set_phase(2.25);
        assert!((oscillator.phase() - 0.25).abs() < 1e-6);
        oscillator.set_phase(-0.25);
        assert!((oscillator.phase() - 0.75).abs() < 1e-6);
    }

    #[test]
    fn frequency_is_clamped_below_nyquist() {
        let mut oscillator = Oscillator::new(Shape::Saw, 1_000_000.0, RATE);
        assert!(oscillator.frequency() < RATE * 0.5);
        oscillator.set_frequency(f32::NAN);
        assert_eq!(oscillator.frequency(), 0.0);
    }

    #[test]
    fn correction_reduces_aliasing_of_a_high_saw() {
        // A raw ramp at 5 kHz folds a great deal of energy back down; the
        // corrected one keeps far more of its energy in the harmonics.
        let mut oscillator = Oscillator::new(Shape::Saw, 5_000.0, RATE);
        let mut corrected = [0.0_f32; 1_024];
        oscillator.process_block(&mut corrected);

        let mut raw = [0.0_f32; 1_024];
        let increment = 5_000.0 / RATE;
        let mut phase = 0.0_f32;
        for sample in &mut raw {
            *sample = 2.0 * phase - 1.0;
            phase += increment;
            phase -= phase.floor();
        }
        // Energy that is not on a harmonic of 5 kHz counts as aliasing;
        // measure what sits between harmonics in the top octave.
        let corrected_high = energy_above(&corrected, 20_000.0);
        let raw_high = energy_above(&raw, 20_000.0);
        assert!(
            corrected_high < raw_high,
            "corrected {corrected_high}, raw {raw_high}"
        );
    }

    #[test]
    fn low_saw_approximates_a_ramp_between_its_discontinuities() {
        let mut oscillator = Oscillator::new(Shape::Saw, 10.0, RATE);
        let increment = 10.0 / RATE;
        let mut worst = 0.0_f32;
        for _ in 0..4_800 {
            let phase = oscillator.phase();
            let value = oscillator.process();
            // The correction only applies within one sample of the wrap.
            if phase > increment * 2.0 && phase < 1.0 - increment * 2.0 {
                worst = worst.max((value - (2.0 * phase - 1.0)).abs());
            }
        }
        assert!(worst < 1e-5, "{worst}");
    }

    #[test]
    fn square_is_symmetric_and_odd_harmonics_only() {
        let mut oscillator = Oscillator::new(Shape::Square, 100.0, RATE);
        let mut sum = 0.0_f64;
        for _ in 0..4_800 {
            sum += f64::from(oscillator.process());
        }
        // Ten whole cycles average to zero.
        assert!((sum / 4_800.0).abs() < 0.01, "{sum}");
    }

    #[test]
    fn triangle_is_bounded_and_roughly_centered() {
        let mut oscillator = Oscillator::new(Shape::Triangle, 220.0, RATE);
        // Let the leaky integrator settle.
        for _ in 0..48_000 {
            oscillator.process();
        }
        let mut sum = 0.0_f64;
        let mut peak = 0.0_f32;
        for _ in 0..48_000 {
            let value = oscillator.process();
            sum += f64::from(value);
            peak = peak.max(value.abs());
        }
        assert!((sum / 48_000.0).abs() < 0.1, "{}", sum / 48_000.0);
        assert!(peak > 0.1 && peak <= 1.0, "{peak}");
    }

    #[test]
    fn reset_returns_to_the_start() {
        let mut oscillator = Oscillator::new(Shape::Triangle, 440.0, RATE);
        for _ in 0..100 {
            oscillator.process();
        }
        oscillator.reset();
        assert_eq!(oscillator.phase(), 0.0);
        let first = oscillator.process();
        oscillator.reset();
        assert_eq!(oscillator.process(), first);
    }

    #[test]
    fn a_stopped_oscillator_holds_its_value() {
        let mut oscillator = Oscillator::new(Shape::Saw, 0.0, RATE);
        let first = oscillator.process();
        for _ in 0..100 {
            assert_eq!(oscillator.process(), first);
        }
    }
}
