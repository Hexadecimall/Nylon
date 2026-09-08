//! Parameter smoothing.
//!
//! A control change applied straight to a gain or a filter coefficient
//! steps the signal and clicks. These types move a value toward its target
//! over a fixed time so the change is inaudible, and they report when the
//! target has been reached so a block can take a plain path once the ramp
//! ends.

/// One-pole smoother. Reaches within 1/e of the target in the configured
/// time and settles fully in about five times that.
#[derive(Clone, Copy, Debug)]
pub struct OnePole {
    target: f32,
    // Distance still to travel. Decaying this multiplicatively keeps full
    // relative precision; recomputing the value from the target each
    // sample instead reaches a floating-point fixed point short of it.
    difference: f32,
    coefficient: f32,
}

impl OnePole {
    /// A smoother resting at `value` with a time constant of
    /// `seconds` at `sample_rate`. A non-positive time makes every change
    /// immediate.
    #[must_use]
    pub fn new(value: f32, seconds: f32, sample_rate: f32) -> Self {
        let mut smoother = Self {
            target: value,
            difference: 0.0,
            coefficient: 0.0,
        };
        smoother.set_time(seconds, sample_rate);
        smoother
    }

    /// Changes the time constant, keeping the current value and target.
    pub fn set_time(&mut self, seconds: f32, sample_rate: f32) {
        self.coefficient = if seconds <= 0.0 || sample_rate <= 0.0 {
            0.0
        } else {
            (-1.0 / (seconds * sample_rate)).exp()
        };
    }

    /// Sets the value the smoother moves toward.
    #[inline]
    pub fn set_target(&mut self, target: f32) {
        self.difference = self.current() - target;
        self.target = target;
    }

    /// Jumps to `value` with no ramp.
    #[inline]
    pub fn reset(&mut self, value: f32) {
        self.target = value;
        self.difference = 0.0;
    }

    /// Value reached so far.
    #[inline]
    #[must_use]
    pub fn current(&self) -> f32 {
        self.target + self.difference
    }

    /// Value being approached.
    #[inline]
    #[must_use]
    pub const fn target(&self) -> f32 {
        self.target
    }

    /// True once the value has settled on the target.
    #[inline]
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.difference == 0.0
    }

    /// Advances one sample and returns the new value.
    #[inline]
    pub fn process(&mut self) -> f32 {
        self.difference *= self.coefficient;
        if self.difference.abs() <= super::SILENCE {
            self.difference = 0.0;
        }
        self.current()
    }
}

/// Linear ramp over a fixed number of samples. Unlike [`OnePole`] this
/// arrives exactly, which matters when a value must land on a boundary,
/// such as a crossfade reaching full level at the end of a block.
#[derive(Clone, Copy, Debug)]
pub struct Ramp {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
}

impl Ramp {
    /// A ramp resting at `value`.
    #[must_use]
    pub const fn new(value: f32) -> Self {
        Self {
            current: value,
            target: value,
            step: 0.0,
            remaining: 0,
        }
    }

    /// Ramps to `target` over `samples`. Zero samples jumps immediately.
    pub fn ramp_to(&mut self, target: f32, samples: u32) {
        self.target = target;
        if samples == 0 {
            self.current = target;
            self.remaining = 0;
            self.step = 0.0;
        } else {
            self.remaining = samples;
            self.step = (target - self.current) / samples as f32;
        }
    }

    /// Jumps to `value` with no ramp.
    #[inline]
    pub fn reset(&mut self, value: f32) {
        self.current = value;
        self.target = value;
        self.remaining = 0;
        self.step = 0.0;
    }

    /// Value reached so far.
    #[inline]
    #[must_use]
    pub const fn current(&self) -> f32 {
        self.current
    }

    /// Samples left before the target is reached.
    #[inline]
    #[must_use]
    pub const fn remaining(&self) -> u32 {
        self.remaining
    }

    /// True once the value has arrived.
    #[inline]
    #[must_use]
    pub const fn is_settled(&self) -> bool {
        self.remaining == 0
    }

    /// Advances one sample and returns the new value.
    #[inline]
    pub fn process(&mut self) -> f32 {
        if self.remaining == 0 {
            return self.current;
        }
        self.remaining -= 1;
        if self.remaining == 0 {
            self.current = self.target;
        } else {
            self.current += self.step;
        }
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_pole_approaches_without_overshoot() {
        let mut smoother = OnePole::new(0.0, 0.01, 48_000.0);
        smoother.set_target(1.0);
        let mut previous = 0.0;
        for _ in 0..48_000 {
            let value = smoother.process();
            assert!(value >= previous, "monotonic");
            assert!(value <= 1.0, "no overshoot: {value}");
            previous = value;
        }
        assert!(smoother.is_settled());
        assert_eq!(smoother.current(), 1.0);
    }

    #[test]
    fn one_pole_time_constant_is_honored() {
        let mut smoother = OnePole::new(0.0, 0.1, 48_000.0);
        smoother.set_target(1.0);
        for _ in 0..4_800 {
            smoother.process();
        }
        // One time constant reaches 1 - 1/e.
        let expected = 1.0 - core::f32::consts::E.recip();
        assert!(
            (smoother.current() - expected).abs() < 0.01,
            "{}",
            smoother.current()
        );
    }

    #[test]
    fn one_pole_zero_time_is_immediate() {
        let mut smoother = OnePole::new(0.0, 0.0, 48_000.0);
        smoother.set_target(0.75);
        assert_eq!(smoother.process(), 0.75);
        assert!(smoother.is_settled());
    }

    #[test]
    fn one_pole_reset_clears_the_ramp() {
        let mut smoother = OnePole::new(0.0, 0.05, 48_000.0);
        smoother.set_target(1.0);
        smoother.process();
        smoother.reset(0.25);
        assert_eq!(smoother.current(), 0.25);
        assert_eq!(smoother.target(), 0.25);
        assert!(smoother.is_settled());
        assert_eq!(smoother.process(), 0.25);
    }

    #[test]
    fn ramp_arrives_exactly_on_the_last_sample() {
        let mut ramp = Ramp::new(0.0);
        ramp.ramp_to(1.0, 64);
        for index in 0..63 {
            let value = ramp.process();
            assert!(value > 0.0 && value < 1.0, "{index}: {value}");
            assert!(!ramp.is_settled());
        }
        assert_eq!(ramp.process(), 1.0);
        assert!(ramp.is_settled());
        assert_eq!(ramp.process(), 1.0);
    }

    #[test]
    fn ramp_is_linear() {
        let mut ramp = Ramp::new(0.0);
        ramp.ramp_to(8.0, 8);
        for expected in 1..=8 {
            assert!((ramp.process() - expected as f32).abs() < 1e-5);
        }
    }

    #[test]
    fn ramp_retarget_starts_from_where_it_is() {
        let mut ramp = Ramp::new(0.0);
        ramp.ramp_to(1.0, 100);
        for _ in 0..50 {
            ramp.process();
        }
        let midpoint = ramp.current();
        assert!((midpoint - 0.5).abs() < 0.02);
        ramp.ramp_to(0.0, 50);
        for _ in 0..50 {
            ramp.process();
        }
        assert_eq!(ramp.current(), 0.0);
    }

    #[test]
    fn ramp_zero_samples_jumps() {
        let mut ramp = Ramp::new(2.0);
        ramp.ramp_to(-1.0, 0);
        assert_eq!(ramp.current(), -1.0);
        assert!(ramp.is_settled());
    }
}
