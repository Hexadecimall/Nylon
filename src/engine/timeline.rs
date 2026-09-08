//! Immutable audio regions rendered against musical time.

use crate::engine::sample::{Interpolation, Sample};
use crate::engine::schedule::Span;
use crate::mixer::MAX_TRACKS;
use std::ops::Range;

/// Maximum decoded media objects in one published timeline.
pub const MAX_MEDIA: usize = 1_024;
/// Maximum audio regions in one published timeline.
pub const MAX_REGIONS: usize = 4_096;

/// One placement of decoded media on a track.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioRegion {
    sample: usize,
    track: usize,
    start_beats: f64,
    length_beats: f64,
    source_start: f64,
    source_frames_per_beat: f64,
    gain: f32,
    reverse: bool,
    loop_frames: Option<Range<usize>>,
    interpolation: Interpolation,
}

impl AudioRegion {
    /// Builds a forward region at unity gain.
    #[must_use]
    pub fn new(
        sample: usize,
        track: usize,
        start_beats: f64,
        length_beats: f64,
        source_start: f64,
        source_frames_per_beat: f64,
    ) -> Option<Self> {
        if track >= MAX_TRACKS
            || !start_beats.is_finite()
            || start_beats < 0.0
            || !length_beats.is_finite()
            || length_beats <= 0.0
            || !source_start.is_finite()
            || source_start < 0.0
            || !source_frames_per_beat.is_finite()
            || source_frames_per_beat <= 0.0
        {
            return None;
        }
        Some(Self {
            sample,
            track,
            start_beats,
            length_beats,
            source_start,
            source_frames_per_beat,
            gain: 1.0,
            reverse: false,
            loop_frames: None,
            interpolation: Interpolation::Cubic,
        })
    }

    /// Sets linear clip gain.
    pub fn set_gain(&mut self, gain: f32) {
        self.gain = if gain.is_finite() { gain.max(0.0) } else { 0.0 };
    }

    /// Chooses reverse playback.
    pub fn set_reverse(&mut self, reverse: bool) {
        self.reverse = reverse;
    }

    /// Chooses interpolation quality.
    pub fn set_interpolation(&mut self, interpolation: Interpolation) {
        self.interpolation = interpolation;
    }

    /// Sets a half-open source loop. The media bounds are checked when the
    /// region enters a timeline.
    pub fn set_loop(&mut self, range: Option<Range<usize>>) -> bool {
        if range.as_ref().is_some_and(|range| range.start >= range.end) {
            return false;
        }
        self.loop_frames = range;
        true
    }
}

/// Media and placements transferred to the audio thread as one value.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioTimeline {
    media: Vec<Sample>,
    regions: Vec<AudioRegion>,
    track_ranges: [(usize, usize); MAX_TRACKS],
}

impl Default for AudioTimeline {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioTimeline {
    /// Empty timeline.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            media: Vec::new(),
            regions: Vec::new(),
            track_ranges: [(0, 0); MAX_TRACKS],
        }
    }

    /// Adds decoded media and returns its index. A full timeline returns the
    /// sample to the caller.
    pub fn add_sample(&mut self, sample: Sample) -> Result<usize, Sample> {
        if self.media.len() >= MAX_MEDIA {
            return Err(sample);
        }
        let index = self.media.len();
        self.media.push(sample);
        Ok(index)
    }

    /// Adds a placement after checking its media and loop range.
    pub fn add_region(&mut self, region: AudioRegion) -> Result<usize, AudioRegion> {
        let valid_sample = self.media.get(region.sample).is_some_and(|sample| {
            region.source_start < sample.len() as f64
                && region
                    .loop_frames
                    .as_ref()
                    .is_none_or(|range| range.end <= sample.len())
        });
        if !valid_sample || self.regions.len() >= MAX_REGIONS {
            return Err(region);
        }
        let index = self
            .regions
            .partition_point(|placed| placed.track <= region.track);
        self.regions.insert(index, region);
        self.rebuild_ranges();
        Ok(index)
    }

    /// Number of decoded media objects.
    #[must_use]
    pub fn media_count(&self) -> usize {
        self.media.len()
    }

    /// Number of placements.
    #[must_use]
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// Adds the regions on one track into a block.
    pub fn render_track(&self, track: usize, span: Span, output: &mut [[f32; 2]]) {
        if output.is_empty() || span.frames == 0 {
            return;
        }
        let Some(&(start, end)) = self.track_ranges.get(track) else {
            return;
        };
        let beat_step = span.length_beats / span.frames as f64;
        for region in &self.regions[start..end] {
            let Some(sample) = self.media.get(region.sample) else {
                continue;
            };
            let region_end = region.start_beats + region.length_beats;
            for (index, frame) in output.iter_mut().enumerate() {
                let beat = span.start_beats + index as f64 * beat_step;
                if beat < region.start_beats || beat >= region_end {
                    continue;
                }
                let elapsed = beat - region.start_beats;
                let position = source_position(region, sample, elapsed);
                let source = interpolate(
                    sample,
                    position,
                    region.interpolation,
                    region.loop_frames.as_ref(),
                );
                frame[0] += source[0] * region.gain;
                frame[1] += source[1] * region.gain;
            }
        }
    }

    fn rebuild_ranges(&mut self) {
        self.track_ranges.fill((0, 0));
        for track in 0..MAX_TRACKS {
            let start = self.regions.partition_point(|region| region.track < track);
            let end = self.regions.partition_point(|region| region.track <= track);
            self.track_ranges[track] = (start, end);
        }
    }
}

fn source_position(region: &AudioRegion, sample: &Sample, elapsed_beats: f64) -> f64 {
    let travelled = elapsed_beats * region.source_frames_per_beat;
    if let Some(range) = region.loop_frames.as_ref() {
        let start = range.start as f64;
        let length = (range.end - range.start) as f64;
        let initial = if region.reverse {
            range.end.saturating_sub(1) as f64 - travelled
        } else {
            region.source_start + travelled
        };
        start + (initial - start).rem_euclid(length)
    } else if region.reverse {
        let available_end = (region.source_start
            + region.length_beats * region.source_frames_per_beat)
            .min(sample.len() as f64);
        available_end - 1.0 - travelled
    } else {
        region.source_start + travelled
    }
}

fn interpolate(
    sample: &Sample,
    position: f64,
    interpolation: Interpolation,
    loop_frames: Option<&Range<usize>>,
) -> [f32; 2] {
    if !position.is_finite() || position < 0.0 || position >= sample.len() as f64 {
        return [0.0; 2];
    }
    let base = position.floor() as isize;
    let fraction = (position - base as f64) as f32;
    let frame = |index: isize| {
        let index = if let Some(range) = loop_frames {
            let start = range.start as isize;
            start + (index - start).rem_euclid((range.end - range.start) as isize)
        } else {
            index.clamp(0, sample.len().saturating_sub(1) as isize)
        } as usize;
        sample.frames()[index]
    };
    let b = frame(base);
    let c = frame(base + 1);
    if interpolation == Interpolation::Linear {
        return [
            b[0] + (c[0] - b[0]) * fraction,
            b[1] + (c[1] - b[1]) * fraction,
        ];
    }
    let a = frame(base - 1);
    let d = frame(base + 2);
    [
        cubic(a[0], b[0], c[0], d[0], fraction),
        cubic(a[1], b[1], c[1], d[1], fraction),
    ]
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

    fn timeline() -> AudioTimeline {
        let sample = Sample::new(4, vec![[0.0; 2], [1.0; 2], [0.0; 2], [-1.0; 2]]).unwrap();
        let mut timeline = AudioTimeline::new();
        let media = timeline.add_sample(sample).unwrap();
        let mut region = AudioRegion::new(media, 2, 1.0, 2.0, 0.0, 2.0).unwrap();
        region.set_interpolation(Interpolation::Linear);
        timeline.add_region(region).unwrap();
        timeline
    }

    #[test]
    fn a_region_lands_on_its_track_and_beat() {
        let timeline = timeline();
        let span = Span {
            start_beats: 0.0,
            length_beats: 2.0,
            frames: 8,
        };
        let mut wrong = [[0.0; 2]; 8];
        timeline.render_track(1, span, &mut wrong);
        assert!(wrong.iter().flatten().all(|sample| *sample == 0.0));
        let mut output = [[0.0; 2]; 8];
        timeline.render_track(2, span, &mut output);
        assert_eq!(output[4], [0.0; 2]);
        assert_eq!(output[5], [0.5; 2]);
        assert_eq!(output[6], [1.0; 2]);
    }

    #[test]
    fn invalid_regions_and_capacity_are_rejected() {
        assert!(AudioRegion::new(0, MAX_TRACKS, 0.0, 1.0, 0.0, 1.0).is_none());
        let mut timeline = AudioTimeline::new();
        let region = AudioRegion::new(0, 0, 0.0, 1.0, 0.0, 1.0).unwrap();
        assert_eq!(timeline.add_region(region.clone()), Err(region));
        let media = timeline
            .add_sample(Sample::new(1, vec![[0.0; 2]]).unwrap())
            .unwrap();
        let mut region = AudioRegion::new(media, 0, 0.0, 1.0, 0.0, 1.0).unwrap();
        assert!(region.set_loop(Some(0..2)));
        assert!(timeline.add_region(region).is_err());
    }

    #[test]
    fn looping_reverse_and_gain_are_applied() {
        let sample = Sample::new(4, vec![[0.0; 2], [1.0; 2], [0.5; 2], [-1.0; 2]]).unwrap();
        let mut timeline = AudioTimeline::new();
        let media = timeline.add_sample(sample).unwrap();
        let mut region = AudioRegion::new(media, 0, 0.0, 4.0, 1.0, 1.0).unwrap();
        region.set_reverse(true);
        region.set_gain(0.5);
        region.set_interpolation(Interpolation::Linear);
        assert!(region.set_loop(Some(1..3)));
        timeline.add_region(region).unwrap();
        let mut output = [[0.0; 2]; 4];
        timeline.render_track(
            0,
            Span {
                start_beats: 0.0,
                length_beats: 4.0,
                frames: 4,
            },
            &mut output,
        );
        assert_eq!(output, [[0.25; 2], [0.5; 2], [0.25; 2], [0.5; 2]]);
    }
}
