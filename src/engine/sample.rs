//! In-memory sample playback with deterministic rate conversion.

use crate::dsp::db;
use crate::wave::WaveFile;
use std::ops::Range;

/// Why sample storage could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleError {
    /// The rate is zero or the source has no frames.
    Empty,
    /// At least one sample is not finite.
    NonFinite,
}

/// Stereo audio owned by the control side.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    sample_rate: u32,
    frames: Box<[[f32; 2]]>,
}

impl Sample {
    /// Builds storage from stereo frames.
    ///
    /// # Errors
    ///
    /// Returns [`SampleError`] for an empty, zero-rate, or non-finite source.
    pub fn new(sample_rate: u32, frames: Vec<[f32; 2]>) -> Result<Self, SampleError> {
        if sample_rate == 0 || frames.is_empty() {
            return Err(SampleError::Empty);
        }
        if frames.iter().flatten().any(|sample| !sample.is_finite()) {
            return Err(SampleError::NonFinite);
        }
        Ok(Self {
            sample_rate,
            frames: frames.into_boxed_slice(),
        })
    }

    /// Converts a decoded WAVE file to stereo storage.
    ///
    /// # Errors
    ///
    /// Returns [`SampleError`] when the decoded file has no usable audio.
    pub fn from_wave(file: &WaveFile) -> Result<Self, SampleError> {
        let frames = (0..file.frames())
            .map(|index| file.stereo_frame(index))
            .collect();
        Self::new(file.format.sample_rate, frames)
    }

    /// Source rate in frames per second.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Number of stereo frames.
    #[must_use]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether this sample has no frames.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Borrowed stereo frames.
    #[must_use]
    pub fn frames(&self) -> &[[f32; 2]] {
        &self.frames
    }
}

/// Interpolation used when source and output clocks do not align.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Interpolation {
    /// Two-point interpolation for inexpensive preview and scrubbing.
    Linear,
    /// Four-point interpolation for normal playback.
    #[default]
    Cubic,
}

/// Borrowed playback state. Rendering performs no allocation.
pub struct Player<'a> {
    sample: &'a Sample,
    position: f64,
    base_step: f64,
    step: f64,
    speed: f64,
    gain: f32,
    loop_range: Option<Range<usize>>,
    interpolation: Interpolation,
    reverse: bool,
    active: bool,
}

impl<'a> Player<'a> {
    /// Builds stopped playback state for an output clock.
    #[must_use]
    pub fn new(sample: &'a Sample, output_rate: u32) -> Self {
        let output_rate = output_rate.max(1);
        let base_step = f64::from(sample.sample_rate()) / f64::from(output_rate);
        let mut player = Self {
            sample,
            position: 0.0,
            base_step,
            step: base_step,
            speed: 1.0,
            gain: 1.0,
            loop_range: None,
            interpolation: Interpolation::default(),
            reverse: false,
            active: false,
        };
        player.update_step();
        player
    }

    /// Starts from the beginning, or from the final frame in reverse mode.
    pub fn trigger(&mut self) {
        self.position = if self.reverse {
            self.loop_range
                .as_ref()
                .map_or(self.sample.len(), |range| range.end)
                .saturating_sub(1) as f64
        } else {
            self.loop_range.as_ref().map_or(0, |range| range.start) as f64
        };
        self.active = true;
    }

    /// Stops immediately.
    pub fn stop(&mut self) {
        self.active = false;
    }

    /// Whether another render can produce audio.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Current fractional source-frame position.
    #[must_use]
    pub const fn position(&self) -> f64 {
        self.position
    }

    /// Moves to a source frame. Values outside the source are rejected.
    pub fn seek(&mut self, frame: f64) -> bool {
        if !frame.is_finite() || frame < 0.0 || frame >= self.sample.len() as f64 {
            return false;
        }
        self.position = frame;
        true
    }

    /// Sets playback speed. The usable range is one thirty-second to 32x.
    pub fn set_speed(&mut self, speed: f64) {
        self.speed = if speed.is_finite() {
            speed.clamp(1.0 / 32.0, 32.0)
        } else {
            1.0
        };
        self.update_step();
    }

    /// Sets clip gain in decibels.
    pub fn set_gain_db(&mut self, gain_db: f32) {
        self.gain = db::to_linear(gain_db);
    }

    /// Chooses forward or reverse playback.
    pub fn set_reverse(&mut self, reverse: bool) {
        self.reverse = reverse;
        self.update_step();
    }

    /// Chooses interpolation quality.
    pub fn set_interpolation(&mut self, interpolation: Interpolation) {
        self.interpolation = interpolation;
    }

    /// Sets a half-open loop in source frames. `None` disables looping.
    pub fn set_loop(&mut self, range: Option<Range<usize>>) -> bool {
        if let Some(range) = range.as_ref()
            && (range.start >= range.end || range.end > self.sample.len())
        {
            return false;
        }
        self.loop_range = range;
        true
    }

    /// Adds playback into an output block.
    pub fn render_additive(&mut self, output: &mut [[f32; 2]]) {
        for frame in output {
            if !self.active {
                break;
            }
            let source = self.interpolate(self.position);
            frame[0] += source[0] * self.gain;
            frame[1] += source[1] * self.gain;
            self.advance();
        }
    }

    fn update_step(&mut self) {
        let magnitude = self.base_step * self.speed;
        self.step = magnitude.copysign(if self.reverse { -1.0 } else { 1.0 });
    }

    fn interpolate(&self, position: f64) -> [f32; 2] {
        let base = position.floor() as isize;
        let fraction = (position - base as f64) as f32;
        match self.interpolation {
            Interpolation::Linear => {
                let a = self.frame(base);
                let b = self.frame(base + 1);
                [
                    a[0] + (b[0] - a[0]) * fraction,
                    a[1] + (b[1] - a[1]) * fraction,
                ]
            }
            Interpolation::Cubic => {
                let a = self.frame(base - 1);
                let b = self.frame(base);
                let c = self.frame(base + 1);
                let d = self.frame(base + 2);
                [
                    cubic(a[0], b[0], c[0], d[0], fraction),
                    cubic(a[1], b[1], c[1], d[1], fraction),
                ]
            }
        }
    }

    fn frame(&self, index: isize) -> [f32; 2] {
        let index = if let Some(range) = self.loop_range.as_ref() {
            let start = range.start as isize;
            let length = (range.end - range.start) as isize;
            start + (index - start).rem_euclid(length)
        } else {
            index.clamp(0, self.sample.len().saturating_sub(1) as isize)
        } as usize;
        self.sample.frames[index]
    }

    fn advance(&mut self) {
        self.position += self.step;
        if let Some(range) = self.loop_range.as_ref() {
            let start = range.start as f64;
            let end = range.end as f64;
            let length = end - start;
            self.position = start + (self.position - start).rem_euclid(length);
        } else if self.position < 0.0 || self.position >= self.sample.len() as f64 {
            self.active = false;
        }
    }
}

fn cubic(a: f32, b: f32, c: f32, d: f32, position: f32) -> f32 {
    let c0 = b;
    let c1 = 0.5 * (c - a);
    let c2 = a - 2.5 * b + 2.0 * c - 0.5 * d;
    let c3 = 0.5 * (d - a) + 1.5 * (b - c);
    ((c3 * position + c2) * position + c1) * position + c0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wave::{Format, SampleFormat};

    fn sample() -> Sample {
        Sample::new(4, vec![[0.0, 1.0], [1.0, 0.5], [0.0, 0.0], [-1.0, -0.5]]).unwrap()
    }

    #[test]
    fn a_matching_clock_reads_each_frame_once() {
        let sample = sample();
        let mut player = Player::new(&sample, 4);
        player.set_interpolation(Interpolation::Linear);
        player.trigger();
        let mut output = [[0.0; 2]; 4];
        player.render_additive(&mut output);
        assert_eq!(output, sample.frames());
        assert!(!player.is_active());
    }

    #[test]
    fn rate_conversion_interpolates_between_frames() {
        let sample = sample();
        let mut player = Player::new(&sample, 8);
        player.set_interpolation(Interpolation::Linear);
        player.trigger();
        let mut output = [[0.0; 2]; 3];
        player.render_additive(&mut output);
        assert_eq!(output[0], [0.0, 1.0]);
        assert_eq!(output[1], [0.5, 0.75]);
        assert_eq!(output[2], [1.0, 0.5]);
    }

    #[test]
    fn reverse_and_looping_wrap_in_the_selected_range() {
        let sample = sample();
        let mut player = Player::new(&sample, 4);
        assert!(player.set_loop(Some(1..3)));
        player.set_reverse(true);
        player.set_interpolation(Interpolation::Linear);
        player.trigger();
        let mut output = [[0.0; 2]; 5];
        player.render_additive(&mut output);
        assert_eq!(
            output,
            [[0.0, 0.0], [1.0, 0.5], [0.0, 0.0], [1.0, 0.5], [0.0, 0.0]]
        );
        assert!(player.is_active());
    }

    #[test]
    fn gain_seek_and_stop_are_explicit() {
        let sample = sample();
        let mut player = Player::new(&sample, 4);
        player.set_interpolation(Interpolation::Linear);
        player.set_gain_db(-6.020_6);
        assert!(player.seek(1.0));
        assert!(!player.seek(4.0));
        player.trigger();
        assert!(player.seek(1.0));
        let mut output = [[0.0; 2]; 1];
        player.render_additive(&mut output);
        assert!((output[0][0] - 0.5).abs() < 0.000_01);
        player.stop();
        player.render_additive(&mut output);
        assert!(!player.is_active());
    }

    #[test]
    fn a_wave_file_converts_mono_to_stereo() {
        let file = WaveFile {
            format: Format {
                sample_rate: 48_000,
                channels: 1,
                bits: 32,
                sample_format: SampleFormat::Float,
            },
            samples: vec![0.25, -0.5],
        };
        let sample = Sample::from_wave(&file).unwrap();
        assert_eq!(sample.frames(), &[[0.25, 0.25], [-0.5, -0.5]]);
    }

    #[test]
    fn invalid_storage_and_loop_ranges_are_rejected() {
        assert_eq!(Sample::new(0, vec![[0.0; 2]]), Err(SampleError::Empty));
        assert_eq!(Sample::new(1, Vec::new()), Err(SampleError::Empty));
        assert_eq!(
            Sample::new(1, vec![[f32::NAN, 0.0]]),
            Err(SampleError::NonFinite)
        );
        let sample = sample();
        let mut player = Player::new(&sample, 4);
        assert!(!player.set_loop(Some(2..2)));
        assert!(!player.set_loop(Some(0..5)));
    }
}
