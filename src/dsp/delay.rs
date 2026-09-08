//! Delay line over caller-owned storage.
//!
//! The buffer belongs to the caller, which allocates it on the control
//! thread; the line itself only holds a write position. Reads accept a
//! fractional delay and interpolate, so a modulated delay sweeps smoothly
//! instead of stepping between samples.

/// Write cursor into a caller-owned buffer.
#[derive(Clone, Copy, Debug, Default)]
pub struct DelayLine {
    write: usize,
}

impl DelayLine {
    /// A line positioned at the start of its buffer.
    #[must_use]
    pub const fn new() -> Self {
        Self { write: 0 }
    }

    /// Clears the buffer and returns to the start.
    pub fn reset(&mut self, buffer: &mut [f32]) {
        self.write = 0;
        buffer.fill(0.0);
    }

    /// Position the next write will use.
    #[inline]
    #[must_use]
    pub const fn position(&self) -> usize {
        self.write
    }

    /// Writes one sample and advances.
    #[inline]
    pub fn write(&mut self, buffer: &mut [f32], sample: f32) {
        if buffer.is_empty() {
            return;
        }
        buffer[self.write] = sample;
        self.write += 1;
        if self.write == buffer.len() {
            self.write = 0;
        }
    }

    /// Reads the sample written `delay` samples ago. A delay of zero
    /// returns the most recent write. Delays beyond the buffer are clamped
    /// to its length.
    #[inline]
    #[must_use]
    pub fn read(&self, buffer: &[f32], delay: usize) -> f32 {
        if buffer.is_empty() {
            return 0.0;
        }
        let delay = delay.min(buffer.len() - 1);
        // `write` points one past the newest sample.
        let index = (self.write + buffer.len() - 1 - delay) % buffer.len();
        buffer[index]
    }

    /// Reads with a fractional delay, interpolating between neighbours.
    #[inline]
    #[must_use]
    pub fn read_interpolated(&self, buffer: &[f32], delay: f32) -> f32 {
        if buffer.is_empty() {
            return 0.0;
        }
        let maximum = (buffer.len() - 1) as f32;
        let delay = if delay.is_nan() {
            0.0
        } else {
            delay.clamp(0.0, maximum)
        };
        let whole = delay.floor();
        let fraction = delay - whole;
        let first = self.read(buffer, whole as usize);
        let second = self.read(buffer, (whole as usize + 1).min(buffer.len() - 1));
        first + (second - first) * fraction
    }

    /// Writes `input` and returns the sample delayed by `delay`, the usual
    /// pairing for an effect. Reading happens before writing, so a delay of
    /// zero returns the previous sample rather than the one just written.
    #[inline]
    pub fn tick(&mut self, buffer: &mut [f32], input: f32, delay: f32) -> f32 {
        let output = self.read_interpolated(buffer, delay);
        self.write(buffer, input);
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_delay_is_exact() {
        let mut buffer = [0.0_f32; 8];
        let mut line = DelayLine::new();
        for index in 0..8 {
            line.write(&mut buffer, index as f32 + 1.0);
        }
        // The newest sample is 8, one back is 7, and so on.
        assert_eq!(line.read(&buffer, 0), 8.0);
        assert_eq!(line.read(&buffer, 1), 7.0);
        assert_eq!(line.read(&buffer, 7), 1.0);
    }

    #[test]
    fn writes_wrap_around() {
        let mut buffer = [0.0_f32; 4];
        let mut line = DelayLine::new();
        for index in 0..10 {
            line.write(&mut buffer, index as f32);
        }
        assert_eq!(line.read(&buffer, 0), 9.0);
        assert_eq!(line.read(&buffer, 3), 6.0);
        assert_eq!(line.position(), 10 % 4);
    }

    #[test]
    fn fractional_delay_interpolates() {
        let mut buffer = [0.0_f32; 8];
        let mut line = DelayLine::new();
        for index in 0..8 {
            line.write(&mut buffer, index as f32);
        }
        // Newest is 7, one back is 6; halfway is 6.5.
        assert!((line.read_interpolated(&buffer, 0.0) - 7.0).abs() < 1e-6);
        assert!((line.read_interpolated(&buffer, 0.5) - 6.5).abs() < 1e-6);
        assert!((line.read_interpolated(&buffer, 1.0) - 6.0).abs() < 1e-6);
        assert!((line.read_interpolated(&buffer, 2.25) - 4.75).abs() < 1e-6);
    }

    #[test]
    fn an_impulse_comes_back_after_the_delay() {
        let mut buffer = [0.0_f32; 64];
        let mut line = DelayLine::new();
        let mut output = [0.0_f32; 128];
        for (index, sample) in output.iter_mut().enumerate() {
            let input = if index == 0 { 1.0 } else { 0.0 };
            *sample = line.tick(&mut buffer, input, 16.0);
        }
        // Reading before writing puts the impulse one sample later.
        for (index, sample) in output.iter().enumerate() {
            let expected = if index == 17 { 1.0 } else { 0.0 };
            assert!((sample - expected).abs() < 1e-6, "{index}: {sample}");
        }
    }

    #[test]
    fn feedback_decays_and_stays_finite() {
        let mut buffer = [0.0_f32; 32];
        let mut line = DelayLine::new();
        let mut value = 1.0;
        let mut peak = 0.0_f32;
        for index in 0..10_000 {
            let input = if index == 0 { 1.0 } else { 0.0 };
            value = line.tick(&mut buffer, input + value * 0.5, 8.0);
            assert!(value.is_finite());
            peak = peak.max(value.abs());
        }
        assert!(peak <= 2.0, "{peak}");
        assert!(value.abs() < 1e-6, "{value}");
    }

    #[test]
    fn out_of_range_delays_are_clamped() {
        let mut buffer = [0.0_f32; 4];
        let mut line = DelayLine::new();
        for index in 0..4 {
            line.write(&mut buffer, index as f32);
        }
        assert_eq!(line.read(&buffer, 100), line.read(&buffer, 3));
        assert_eq!(
            line.read_interpolated(&buffer, 100.0),
            line.read(&buffer, 3)
        );
        assert_eq!(line.read_interpolated(&buffer, -5.0), line.read(&buffer, 0));
        assert_eq!(
            line.read_interpolated(&buffer, f32::NAN),
            line.read(&buffer, 0)
        );
    }

    #[test]
    fn an_empty_buffer_is_silent_rather_than_a_panic() {
        let mut buffer: [f32; 0] = [];
        let mut line = DelayLine::new();
        line.write(&mut buffer, 1.0);
        assert_eq!(line.read(&buffer, 0), 0.0);
        assert_eq!(line.read_interpolated(&buffer, 1.5), 0.0);
        assert_eq!(line.tick(&mut buffer, 1.0, 1.0), 0.0);
    }

    #[test]
    fn reset_clears_the_buffer_and_position() {
        let mut buffer = [0.0_f32; 8];
        let mut line = DelayLine::new();
        for index in 0..5 {
            line.write(&mut buffer, index as f32 + 1.0);
        }
        line.reset(&mut buffer);
        assert_eq!(line.position(), 0);
        assert!(buffer.iter().all(|value| *value == 0.0));
    }
}
