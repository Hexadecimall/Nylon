//! Track mixing.
//!
//! The mixer sums a set of stereo track signals into one stereo bus,
//! applying each track's volume, pan, mute, and solo, then the master
//! volume and pan. Parameter changes arrive as events carrying the frame
//! they take effect on, so automation lands on the sample it was written
//! for; the values themselves are smoothed, so a change never steps the
//! signal.
//!
//! Everything is fixed-size. A mixer is built on the control thread and
//! rendered from the callback; no method here allocates, locks, or blocks.

use crate::dsp::db;
use crate::dsp::meter::Meter;
use crate::dsp::pan;
use crate::dsp::smooth::OnePole;

/// Largest number of tracks one mixer can carry.
pub const MAX_TRACKS: usize = 256;
/// Largest block the mixer renders in one call.
pub const MAX_FRAMES: usize = 2048;
/// Largest number of parameter changes in one block.
pub const MAX_EVENTS: usize = 512;
/// Index that addresses the master strip rather than a track.
pub const MASTER: u16 = u16::MAX;

/// Time a smoothed parameter takes to arrive, in seconds. Short enough to
/// feel immediate, long enough that a jump does not click.
const SMOOTHING_SECONDS: f32 = 0.008;

/// Quietest volume that still produces sound. Below this a strip is off.
pub const MIN_VOLUME_DB: f32 = -120.0;
/// Loudest volume a strip accepts.
pub const MAX_VOLUME_DB: f32 = 6.0;

/// A parameter a [`MixEvent`] can change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parameter {
    /// Volume in decibels.
    Volume,
    /// Pan position from -1 to 1.
    Pan,
    /// Mute: any non-zero value silences the strip.
    Mute,
    /// Solo: any non-zero value restricts playback to soloed tracks.
    Solo,
}

/// A parameter change taking effect at a frame offset within the block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixEvent {
    /// Frame the change takes effect on. An offset equal to the block
    /// length applies to the next block.
    pub offset: usize,
    /// Track the change applies to, or [`MASTER`].
    pub track: u16,
    /// Parameter being changed.
    pub parameter: Parameter,
    /// New value, interpreted by the parameter.
    pub value: f32,
}

/// Why a render was refused. A refused render changes nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixError {
    /// The block is longer than [`MAX_FRAMES`], or an input length differs
    /// from the output length.
    BufferSize,
    /// More events than [`MAX_EVENTS`].
    EventCapacity,
    /// An event offset lies past the end of the block.
    EventOffset,
    /// Events are not in ascending offset order.
    EventOrder,
    /// An event names a track that does not exist.
    EventTrack,
    /// An event carries a value the parameter cannot take.
    EventValue,
    /// More inputs than the mixer has tracks, or a repeated track index.
    TrackIndex,
}

/// One track's stereo signal for this block.
#[derive(Clone, Copy, Debug)]
pub struct TrackInput<'a> {
    /// Track the samples belong to.
    pub track: u16,
    /// Interleaved stereo frames, the same length as the output.
    pub samples: &'a [[f32; 2]],
}

/// Levels most recently measured on a strip.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Levels {
    /// Peak amplitude of the left channel.
    pub peak_left: f32,
    /// Peak amplitude of the right channel.
    pub peak_right: f32,
    /// Root-mean-square amplitude of the left channel.
    pub rms_left: f32,
    /// Root-mean-square amplitude of the right channel.
    pub rms_right: f32,
    /// Whether either channel has exceeded full scale.
    pub clipped: bool,
}

/// One channel strip.
#[derive(Clone, Copy, Debug)]
struct Strip {
    volume_db: f32,
    pan_position: f32,
    muted: bool,
    soloed: bool,
    gain: OnePole,
    pan: OnePole,
    // Pan gains for the smoothed position, recomputed only while moving.
    cached_pan: f32,
    cached_gains: pan::Gains,
    meter_left: Meter,
    meter_right: Meter,
}

impl Strip {
    fn new(sample_rate: f32) -> Self {
        let centered = pan::constant_power(0.0);
        let gains = pan::Gains {
            left: centered.left * core::f32::consts::SQRT_2,
            right: centered.right * core::f32::consts::SQRT_2,
        };
        Self {
            volume_db: 0.0,
            pan_position: 0.0,
            muted: false,
            soloed: false,
            gain: OnePole::new(1.0, SMOOTHING_SECONDS, sample_rate),
            pan: OnePole::new(0.0, SMOOTHING_SECONDS, sample_rate),
            cached_pan: 0.0,
            cached_gains: gains,
            meter_left: Meter::with_defaults(sample_rate),
            meter_right: Meter::with_defaults(sample_rate),
        }
    }

    /// Target gain once mute and solo are taken into account.
    fn target_gain(&self, any_solo: bool) -> f32 {
        let audible = !self.muted && (!any_solo || self.soloed);
        if audible {
            db::to_linear(self.volume_db)
        } else {
            0.0
        }
    }

    /// Pan gains for the smoothed position, normalized so a centered
    /// strip is unity and a hard-panned one is 3 dB up on that side. The
    /// law itself still preserves power across the sweep.
    #[inline]
    fn pan_gains(&mut self) -> pan::Gains {
        let position = self.pan.process();
        if position != self.cached_pan {
            self.cached_pan = position;
            let gains = pan::constant_power(position);
            self.cached_gains = pan::Gains {
                left: gains.left * core::f32::consts::SQRT_2,
                right: gains.right * core::f32::consts::SQRT_2,
            };
        }
        self.cached_gains
    }

    fn levels(&self) -> Levels {
        Levels {
            peak_left: self.meter_left.peak(),
            peak_right: self.meter_right.peak(),
            rms_left: self.meter_left.rms(),
            rms_right: self.meter_right.rms(),
            clipped: self.meter_left.is_clipped() || self.meter_right.is_clipped(),
        }
    }
}

/// A fixed-capacity stereo mixer.
pub struct Mixer {
    strips: [Strip; MAX_TRACKS],
    master: Strip,
    track_count: usize,
    any_solo: bool,
    sample_rate: f32,
}

impl Mixer {
    /// A mixer with `track_count` tracks, every strip at unity and center.
    ///
    /// # Panics
    ///
    /// Panics if `track_count` exceeds [`MAX_TRACKS`].
    #[must_use]
    pub fn new(track_count: usize, sample_rate: f32) -> Self {
        assert!(
            track_count <= MAX_TRACKS,
            "track count exceeds the mixer capacity"
        );
        let sample_rate = if sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        Self {
            strips: [Strip::new(sample_rate); MAX_TRACKS],
            master: Strip::new(sample_rate),
            track_count,
            any_solo: false,
            sample_rate,
        }
    }

    /// Number of tracks in use.
    #[inline]
    #[must_use]
    pub const fn track_count(&self) -> usize {
        self.track_count
    }

    /// Sample rate the strips were built for.
    #[inline]
    #[must_use]
    pub const fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Changes the number of tracks. Strips brought into use start from
    /// their defaults; strips taken out of use keep their settings.
    ///
    /// # Panics
    ///
    /// Panics if `track_count` exceeds [`MAX_TRACKS`].
    pub fn set_track_count(&mut self, track_count: usize) {
        assert!(
            track_count <= MAX_TRACKS,
            "track count exceeds the mixer capacity"
        );
        for index in self.track_count..track_count {
            self.strips[index] = Strip::new(self.sample_rate);
        }
        self.track_count = track_count;
        self.refresh_solo();
    }

    /// Sets a strip's volume in decibels without smoothing, as when a
    /// project is loaded. [`MASTER`] addresses the master strip.
    pub fn set_volume_db(&mut self, track: u16, volume_db: f32) {
        let value = clamp_volume(volume_db);
        if let Some(strip) = self.strip_mut(track) {
            strip.volume_db = value;
        }
        let any_solo = self.any_solo;
        if let Some(strip) = self.strip_mut(track) {
            let target = strip.target_gain(any_solo);
            strip.gain.reset(target);
        }
    }

    /// Sets a strip's pan without smoothing.
    pub fn set_pan(&mut self, track: u16, position: f32) {
        let value = crate::dsp::clamp(position, -1.0, 1.0);
        if let Some(strip) = self.strip_mut(track) {
            strip.pan_position = value;
            strip.pan.reset(value);
            // Force a recompute on the next sample.
            strip.cached_pan = f32::NAN;
        }
    }

    /// Mutes or unmutes a strip without smoothing.
    pub fn set_muted(&mut self, track: u16, muted: bool) {
        if let Some(strip) = self.strip_mut(track) {
            strip.muted = muted;
        }
        self.reset_gains();
    }

    /// Soloes or unsoloes a track. The master strip cannot be soloed.
    pub fn set_soloed(&mut self, track: u16, soloed: bool) {
        if track != MASTER
            && let Some(strip) = self.strip_mut(track)
        {
            strip.soloed = soloed;
        }
        self.refresh_solo();
        self.reset_gains();
    }

    /// Volume of a strip in decibels.
    #[must_use]
    pub fn volume_db(&self, track: u16) -> f32 {
        self.strip(track).map_or(0.0, |strip| strip.volume_db)
    }

    /// Pan position of a strip.
    #[must_use]
    pub fn pan(&self, track: u16) -> f32 {
        self.strip(track).map_or(0.0, |strip| strip.pan_position)
    }

    /// Whether a strip is muted.
    #[must_use]
    pub fn is_muted(&self, track: u16) -> bool {
        self.strip(track).is_some_and(|strip| strip.muted)
    }

    /// Whether a track is soloed.
    #[must_use]
    pub fn is_soloed(&self, track: u16) -> bool {
        self.strip(track).is_some_and(|strip| strip.soloed)
    }

    /// Whether any track is soloed, which silences the rest.
    #[inline]
    #[must_use]
    pub const fn any_soloed(&self) -> bool {
        self.any_solo
    }

    /// Levels measured on a strip during the last render.
    #[must_use]
    pub fn levels(&self, track: u16) -> Levels {
        self.strip(track).map_or(Levels::default(), Strip::levels)
    }

    /// Copies every strip's levels into `out`, master last. Returns how
    /// many entries were written, which is the smaller of `out.len()` and
    /// the track count plus one.
    pub fn copy_levels(&self, out: &mut [Levels]) -> usize {
        let mut written = 0;
        for index in 0..self.track_count.min(out.len()) {
            out[index] = self.strips[index].levels();
            written += 1;
        }
        if written < out.len() {
            out[written] = self.master.levels();
            written += 1;
        }
        written
    }

    /// Clears the clip indicator on every strip.
    pub fn clear_clipping(&mut self) {
        for strip in &mut self.strips[..self.track_count] {
            strip.meter_left.clear_clip();
            strip.meter_right.clear_clip();
        }
        self.master.meter_left.clear_clip();
        self.master.meter_right.clear_clip();
    }

    /// Clears every meter and settles every smoother, as when playback
    /// stops.
    pub fn reset(&mut self) {
        for index in 0..self.track_count {
            self.strips[index].meter_left.reset();
            self.strips[index].meter_right.reset();
        }
        self.master.meter_left.reset();
        self.master.meter_right.reset();
        self.reset_gains();
    }

    /// Mixes `inputs` into `output`, applying `events` at their offsets.
    ///
    /// The whole request is validated before anything changes, so a
    /// refused render leaves the mixer and the output untouched. Tracks
    /// with no input contribute silence but still advance their smoothing
    /// and metering.
    ///
    /// # Errors
    ///
    /// Returns [`MixError`] when the buffers, events, or track indices are
    /// not usable.
    pub fn render(
        &mut self,
        inputs: &[TrackInput<'_>],
        output: &mut [[f32; 2]],
        events: &[MixEvent],
    ) -> Result<(), MixError> {
        let frames = output.len();
        if frames > MAX_FRAMES {
            return Err(MixError::BufferSize);
        }
        if events.len() > MAX_EVENTS {
            return Err(MixError::EventCapacity);
        }
        if inputs.len() > self.track_count {
            return Err(MixError::TrackIndex);
        }
        let mut seen = [false; MAX_TRACKS];
        for input in inputs {
            let index = usize::from(input.track);
            if index >= self.track_count {
                return Err(MixError::TrackIndex);
            }
            if seen[index] {
                return Err(MixError::TrackIndex);
            }
            seen[index] = true;
            if input.samples.len() != frames {
                return Err(MixError::BufferSize);
            }
        }
        let mut previous = 0;
        for event in events {
            if event.offset > frames {
                return Err(MixError::EventOffset);
            }
            if event.offset < previous {
                return Err(MixError::EventOrder);
            }
            previous = event.offset;
            if event.track != MASTER && usize::from(event.track) >= self.track_count {
                return Err(MixError::EventTrack);
            }
            if !event.value.is_finite() {
                return Err(MixError::EventValue);
            }
            match event.parameter {
                Parameter::Volume => {
                    if event.value > MAX_VOLUME_DB {
                        return Err(MixError::EventValue);
                    }
                }
                Parameter::Pan => {
                    if !(-1.0..=1.0).contains(&event.value) {
                        return Err(MixError::EventValue);
                    }
                }
                Parameter::Mute | Parameter::Solo => {
                    if event.track == MASTER && event.parameter == Parameter::Solo {
                        return Err(MixError::EventTrack);
                    }
                }
            }
        }

        output.fill([0.0, 0.0]);
        let mut start = 0;
        let mut cursor = 0;
        loop {
            // An event takes effect before the sample at its offset, so
            // everything due at or before the cursor applies first. Events
            // at the block end apply here too, for the next block.
            while cursor < events.len() && events[cursor].offset <= start {
                let event = events[cursor];
                self.apply(event);
                cursor += 1;
            }
            if start >= frames {
                break;
            }
            // The span runs to the next event, or to the end of the block.
            let end = events.get(cursor).map_or(frames, |event| event.offset);
            self.render_span(inputs, output, start, end);
            start = end;
        }
        Ok(())
    }

    /// Mixes one span of the block, exclusive of `end`.
    fn render_span(
        &mut self,
        inputs: &[TrackInput<'_>],
        output: &mut [[f32; 2]],
        start: usize,
        end: usize,
    ) {
        if end <= start {
            return;
        }
        let any_solo = self.any_solo;
        // Tracks with input contribute; every strip advances so a muted or
        // silent track's meters fall the same way a sounding one does.
        for index in 0..self.track_count {
            let strip = &mut self.strips[index];
            strip.gain.set_target(strip.target_gain(any_solo));
            strip.pan.set_target(strip.pan_position);
            let samples = inputs
                .iter()
                .find(|input| usize::from(input.track) == index)
                .map(|input| &input.samples[start..end]);
            match samples {
                Some(samples) => {
                    for (frame, destination) in samples.iter().zip(&mut output[start..end]) {
                        let gain = strip.gain.process();
                        let gains = strip.pan_gains();
                        let left = frame[0] * gain * gains.left;
                        let right = frame[1] * gain * gains.right;
                        strip.meter_left.push(left);
                        strip.meter_right.push(right);
                        destination[0] += left;
                        destination[1] += right;
                    }
                }
                None => {
                    for _ in start..end {
                        strip.gain.process();
                        strip.pan_gains();
                        strip.meter_left.push(0.0);
                        strip.meter_right.push(0.0);
                    }
                }
            }
        }
        let master = &mut self.master;
        master.gain.set_target(master.target_gain(false));
        master.pan.set_target(master.pan_position);
        for frame in &mut output[start..end] {
            let gain = master.gain.process();
            let gains = master.pan_gains();
            frame[0] *= gain * gains.left;
            frame[1] *= gain * gains.right;
            master.meter_left.push(frame[0]);
            master.meter_right.push(frame[1]);
        }
    }

    fn apply(&mut self, event: MixEvent) {
        match event.parameter {
            Parameter::Volume => {
                let value = clamp_volume(event.value);
                if let Some(strip) = self.strip_mut(event.track) {
                    strip.volume_db = value;
                }
            }
            Parameter::Pan => {
                let value = crate::dsp::clamp(event.value, -1.0, 1.0);
                if let Some(strip) = self.strip_mut(event.track) {
                    strip.pan_position = value;
                }
            }
            Parameter::Mute => {
                let muted = event.value != 0.0;
                if let Some(strip) = self.strip_mut(event.track) {
                    strip.muted = muted;
                }
            }
            Parameter::Solo => {
                let soloed = event.value != 0.0;
                if let Some(strip) = self.strip_mut(event.track) {
                    strip.soloed = soloed;
                }
                self.refresh_solo();
            }
        }
    }

    fn refresh_solo(&mut self) {
        self.any_solo = self.strips[..self.track_count]
            .iter()
            .any(|strip| strip.soloed);
    }

    fn reset_gains(&mut self) {
        let any_solo = self.any_solo;
        for index in 0..self.track_count {
            let target = self.strips[index].target_gain(any_solo);
            self.strips[index].gain.reset(target);
        }
        let target = self.master.target_gain(false);
        self.master.gain.reset(target);
    }

    fn strip(&self, track: u16) -> Option<&Strip> {
        if track == MASTER {
            Some(&self.master)
        } else {
            self.strips
                .get(usize::from(track))
                .filter(|_| usize::from(track) < self.track_count)
        }
    }

    fn strip_mut(&mut self, track: u16) -> Option<&mut Strip> {
        if track == MASTER {
            Some(&mut self.master)
        } else if usize::from(track) < self.track_count {
            self.strips.get_mut(usize::from(track))
        } else {
            None
        }
    }
}

fn clamp_volume(volume_db: f32) -> f32 {
    if volume_db.is_nan() || volume_db <= MIN_VOLUME_DB {
        f32::NEG_INFINITY
    } else {
        volume_db.min(MAX_VOLUME_DB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn silence(frames: usize) -> Vec<[f32; 2]> {
        vec![[0.0, 0.0]; frames]
    }

    fn constant(frames: usize, left: f32, right: f32) -> Vec<[f32; 2]> {
        vec![[left, right]; frames]
    }

    /// Renders a block and returns the output, settling the smoothers
    /// first so the result reflects the target values rather than a ramp.
    fn render_settled(
        mixer: &mut Mixer,
        inputs: &[TrackInput<'_>],
        frames: usize,
    ) -> Vec<[f32; 2]> {
        let mut output = silence(frames);
        for _ in 0..8 {
            mixer.render(inputs, &mut output, &[]).unwrap();
        }
        output
    }

    #[test]
    fn a_new_mixer_passes_one_track_at_unity() {
        let mut mixer = Mixer::new(1, RATE);
        let input = constant(64, 0.5, -0.25);
        let inputs = [TrackInput {
            track: 0,
            samples: &input,
        }];
        let output = render_settled(&mut mixer, &inputs, 64);
        // Center pan on both the track and the master costs 3 dB twice;
        // the master compensates so a centered chain is unity.
        for frame in &output {
            assert!((frame[0] - 0.5).abs() < 1e-5, "{frame:?}");
            assert!((frame[1] + 0.25).abs() < 1e-5, "{frame:?}");
        }
    }

    #[test]
    fn tracks_sum() {
        let mut mixer = Mixer::new(3, RATE);
        let a = constant(32, 0.1, 0.1);
        let b = constant(32, 0.2, 0.2);
        let c = constant(32, -0.05, -0.05);
        let inputs = [
            TrackInput {
                track: 0,
                samples: &a,
            },
            TrackInput {
                track: 1,
                samples: &b,
            },
            TrackInput {
                track: 2,
                samples: &c,
            },
        ];
        let output = render_settled(&mut mixer, &inputs, 32);
        for frame in &output {
            assert!((frame[0] - 0.25).abs() < 1e-5, "{frame:?}");
        }
    }

    #[test]
    fn volume_scales_and_silence_is_exact() {
        let mut mixer = Mixer::new(1, RATE);
        let input = constant(64, 1.0, 1.0);
        let inputs = [TrackInput {
            track: 0,
            samples: &input,
        }];

        mixer.set_volume_db(0, -6.020_6);
        let output = render_settled(&mut mixer, &inputs, 64);
        assert!((output[63][0] - 0.5).abs() < 1e-3, "{:?}", output[63]);

        mixer.set_volume_db(0, MIN_VOLUME_DB);
        let output = render_settled(&mut mixer, &inputs, 64);
        assert_eq!(output[63], [0.0, 0.0]);
        assert_eq!(mixer.volume_db(0), f32::NEG_INFINITY);
    }

    #[test]
    fn volume_is_clamped_at_the_top_and_nan_is_silence() {
        let mut mixer = Mixer::new(1, RATE);
        mixer.set_volume_db(0, 100.0);
        assert_eq!(mixer.volume_db(0), MAX_VOLUME_DB);
        mixer.set_volume_db(0, f32::NAN);
        assert_eq!(mixer.volume_db(0), f32::NEG_INFINITY);
    }

    #[test]
    fn panning_moves_the_signal_and_holds_power() {
        let mut mixer = Mixer::new(1, RATE);
        let input = constant(64, 1.0, 1.0);
        let inputs = [TrackInput {
            track: 0,
            samples: &input,
        }];

        mixer.set_pan(0, -1.0);
        let left = render_settled(&mut mixer, &inputs, 64);
        assert!(left[63][0] > 1.3, "{:?}", left[63]);
        assert!(left[63][1].abs() < 1e-5, "{:?}", left[63]);

        mixer.set_pan(0, 1.0);
        let right = render_settled(&mut mixer, &inputs, 64);
        assert!(right[63][0].abs() < 1e-5, "{:?}", right[63]);
        assert!(right[63][1] > 1.3, "{:?}", right[63]);

        mixer.set_pan(0, 0.0);
        let center = render_settled(&mut mixer, &inputs, 64);
        assert!((center[63][0] - 1.0).abs() < 1e-4);
        assert!((center[63][1] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn mute_silences_only_the_muted_track() {
        let mut mixer = Mixer::new(2, RATE);
        let a = constant(32, 0.4, 0.4);
        let b = constant(32, 0.6, 0.6);
        let inputs = [
            TrackInput {
                track: 0,
                samples: &a,
            },
            TrackInput {
                track: 1,
                samples: &b,
            },
        ];
        mixer.set_muted(0, true);
        let output = render_settled(&mut mixer, &inputs, 32);
        assert!((output[31][0] - 0.6).abs() < 1e-4, "{:?}", output[31]);
        assert!(mixer.is_muted(0));
        mixer.set_muted(0, false);
        let output = render_settled(&mut mixer, &inputs, 32);
        assert!((output[31][0] - 1.0).abs() < 1e-4, "{:?}", output[31]);
    }

    #[test]
    fn solo_silences_every_track_that_is_not_soloed() {
        let mut mixer = Mixer::new(3, RATE);
        let a = constant(32, 0.1, 0.1);
        let b = constant(32, 0.2, 0.2);
        let c = constant(32, 0.4, 0.4);
        let inputs = [
            TrackInput {
                track: 0,
                samples: &a,
            },
            TrackInput {
                track: 1,
                samples: &b,
            },
            TrackInput {
                track: 2,
                samples: &c,
            },
        ];
        assert!(!mixer.any_soloed());
        mixer.set_soloed(1, true);
        assert!(mixer.any_soloed());
        let output = render_settled(&mut mixer, &inputs, 32);
        assert!((output[31][0] - 0.2).abs() < 1e-4, "{:?}", output[31]);

        // A second solo adds to what is heard.
        mixer.set_soloed(2, true);
        let output = render_settled(&mut mixer, &inputs, 32);
        assert!((output[31][0] - 0.6).abs() < 1e-4, "{:?}", output[31]);

        // Clearing every solo restores the full mix.
        mixer.set_soloed(1, false);
        mixer.set_soloed(2, false);
        assert!(!mixer.any_soloed());
        let output = render_settled(&mut mixer, &inputs, 32);
        assert!((output[31][0] - 0.7).abs() < 1e-4, "{:?}", output[31]);
    }

    #[test]
    fn mute_wins_over_solo_on_the_same_track() {
        let mut mixer = Mixer::new(2, RATE);
        let a = constant(32, 0.5, 0.5);
        let b = constant(32, 0.25, 0.25);
        let inputs = [
            TrackInput {
                track: 0,
                samples: &a,
            },
            TrackInput {
                track: 1,
                samples: &b,
            },
        ];
        mixer.set_soloed(0, true);
        mixer.set_muted(0, true);
        let output = render_settled(&mut mixer, &inputs, 32);
        assert_eq!(output[31], [0.0, 0.0]);
    }

    #[test]
    fn the_master_strip_scales_the_sum() {
        let mut mixer = Mixer::new(1, RATE);
        let input = constant(64, 1.0, 1.0);
        let inputs = [TrackInput {
            track: 0,
            samples: &input,
        }];
        mixer.set_volume_db(MASTER, -6.020_6);
        let output = render_settled(&mut mixer, &inputs, 64);
        assert!((output[63][0] - 0.5).abs() < 1e-3, "{:?}", output[63]);
        assert_eq!(mixer.volume_db(MASTER), -6.020_6);
        mixer.set_muted(MASTER, true);
        let output = render_settled(&mut mixer, &inputs, 64);
        assert_eq!(output[63], [0.0, 0.0]);
    }

    #[test]
    fn the_master_cannot_be_soloed() {
        let mut mixer = Mixer::new(1, RATE);
        mixer.set_soloed(MASTER, true);
        assert!(!mixer.any_soloed());
        assert!(!mixer.is_soloed(MASTER));
    }

    #[test]
    fn an_event_takes_effect_at_its_offset() {
        let mut mixer = Mixer::new(1, RATE);
        let input = constant(256, 1.0, 1.0);
        let inputs = [TrackInput {
            track: 0,
            samples: &input,
        }];
        let mut output = silence(256);
        // Settle at silence, then rise to unity partway through the block.
        mixer.set_volume_db(0, MIN_VOLUME_DB);
        mixer.render(&inputs, &mut output, &[]).unwrap();
        let events = [MixEvent {
            offset: 128,
            track: 0,
            parameter: Parameter::Volume,
            value: 0.0,
        }];
        mixer.render(&inputs, &mut output, &events).unwrap();
        // Nothing before the offset.
        for (index, frame) in output[..128].iter().enumerate() {
            assert_eq!(*frame, [0.0, 0.0], "{index}");
        }
        // The smoother starts moving at the offset and keeps rising.
        assert!(output[128][0] > 0.0);
        assert!(output[255][0] > output[128][0]);
    }

    #[test]
    fn parameter_changes_are_smoothed_rather_than_stepped() {
        let mut mixer = Mixer::new(1, RATE);
        let input = constant(512, 1.0, 1.0);
        let inputs = [TrackInput {
            track: 0,
            samples: &input,
        }];
        let mut output = silence(512);
        mixer.render(&inputs, &mut output, &[]).unwrap();
        let events = [MixEvent {
            offset: 0,
            track: 0,
            parameter: Parameter::Volume,
            value: MIN_VOLUME_DB,
        }];
        mixer.render(&inputs, &mut output, &events).unwrap();
        // No sample-to-sample jump larger than a small fraction.
        let mut previous = output[0][0];
        let mut largest = 0.0_f32;
        for frame in &output[1..] {
            largest = largest.max((frame[0] - previous).abs());
            previous = frame[0];
        }
        assert!(largest < 0.02, "largest step {largest}");
        // A one-pole is still on its way down after one time constant;
        // several blocks later it is inaudible.
        assert!(output[511][0] < 0.3, "{}", output[511][0]);
        for _ in 0..8 {
            mixer.render(&inputs, &mut output, &[]).unwrap();
        }
        assert!(output[511][0].abs() < 1e-3, "{}", output[511][0]);
    }

    #[test]
    fn several_events_in_one_block_are_applied_in_order() {
        let mut mixer = Mixer::new(2, RATE);
        let a = constant(128, 1.0, 1.0);
        let b = constant(128, 1.0, 1.0);
        let inputs = [
            TrackInput {
                track: 0,
                samples: &a,
            },
            TrackInput {
                track: 1,
                samples: &b,
            },
        ];
        let mut output = silence(128);
        let events = [
            MixEvent {
                offset: 0,
                track: 0,
                parameter: Parameter::Mute,
                value: 1.0,
            },
            MixEvent {
                offset: 32,
                track: 1,
                parameter: Parameter::Pan,
                value: -1.0,
            },
            MixEvent {
                offset: 64,
                track: 0,
                parameter: Parameter::Mute,
                value: 0.0,
            },
            MixEvent {
                offset: 64,
                track: 0,
                parameter: Parameter::Volume,
                value: -6.0,
            },
        ];
        mixer.render(&inputs, &mut output, &events).unwrap();
        assert!(!mixer.is_muted(0));
        assert!((mixer.volume_db(0) + 6.0).abs() < 1e-6);
        assert!((mixer.pan(1) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn an_event_at_the_block_end_applies_to_the_next_block() {
        let mut mixer = Mixer::new(1, RATE);
        let input = constant(64, 1.0, 1.0);
        let inputs = [TrackInput {
            track: 0,
            samples: &input,
        }];
        let mut output = silence(64);
        let events = [MixEvent {
            offset: 64,
            track: 0,
            parameter: Parameter::Mute,
            value: 1.0,
        }];
        mixer.render(&inputs, &mut output, &events).unwrap();
        // The block still played, and the mute is in force afterwards.
        assert!(output[63][0] > 0.5);
        assert!(mixer.is_muted(0));
    }

    #[test]
    fn a_refused_render_changes_nothing() {
        let mut mixer = Mixer::new(2, RATE);
        let input = constant(64, 1.0, 1.0);
        let inputs = [TrackInput {
            track: 0,
            samples: &input,
        }];
        let mut output = constant(64, 9.0, 9.0);
        let before = mixer.volume_db(0);

        let cases: [(&[MixEvent], MixError); 6] = [
            (
                &[MixEvent {
                    offset: 65,
                    track: 0,
                    parameter: Parameter::Volume,
                    value: 0.0,
                }],
                MixError::EventOffset,
            ),
            (
                &[
                    MixEvent {
                        offset: 10,
                        track: 0,
                        parameter: Parameter::Volume,
                        value: 0.0,
                    },
                    MixEvent {
                        offset: 5,
                        track: 0,
                        parameter: Parameter::Volume,
                        value: 0.0,
                    },
                ],
                MixError::EventOrder,
            ),
            (
                &[MixEvent {
                    offset: 0,
                    track: 7,
                    parameter: Parameter::Volume,
                    value: 0.0,
                }],
                MixError::EventTrack,
            ),
            (
                &[MixEvent {
                    offset: 0,
                    track: 0,
                    parameter: Parameter::Volume,
                    value: f32::NAN,
                }],
                MixError::EventValue,
            ),
            (
                &[MixEvent {
                    offset: 0,
                    track: 0,
                    parameter: Parameter::Volume,
                    value: 24.0,
                }],
                MixError::EventValue,
            ),
            (
                &[MixEvent {
                    offset: 0,
                    track: 0,
                    parameter: Parameter::Pan,
                    value: 2.0,
                }],
                MixError::EventValue,
            ),
        ];
        for (events, expected) in cases {
            assert_eq!(mixer.render(&inputs, &mut output, events), Err(expected));
            assert_eq!(output[0], [9.0, 9.0], "output was written for {expected:?}");
            assert_eq!(mixer.volume_db(0), before);
        }

        // Soloing the master is refused as well.
        let solo_master = [MixEvent {
            offset: 0,
            track: MASTER,
            parameter: Parameter::Solo,
            value: 1.0,
        }];
        assert_eq!(
            mixer.render(&inputs, &mut output, &solo_master),
            Err(MixError::EventTrack)
        );
    }

    #[test]
    fn buffer_and_index_problems_are_refused() {
        let mut mixer = Mixer::new(2, RATE);
        let short = constant(32, 1.0, 1.0);
        let mut output = silence(64);
        let mismatched = [TrackInput {
            track: 0,
            samples: &short,
        }];
        assert_eq!(
            mixer.render(&mismatched, &mut output, &[]),
            Err(MixError::BufferSize)
        );

        let long = constant(MAX_FRAMES + 1, 1.0, 1.0);
        let mut big = silence(MAX_FRAMES + 1);
        let inputs = [TrackInput {
            track: 0,
            samples: &long,
        }];
        assert_eq!(
            mixer.render(&inputs, &mut big, &[]),
            Err(MixError::BufferSize)
        );

        let a = constant(64, 1.0, 1.0);
        let duplicated = [
            TrackInput {
                track: 0,
                samples: &a,
            },
            TrackInput {
                track: 0,
                samples: &a,
            },
        ];
        assert_eq!(
            mixer.render(&duplicated, &mut output, &[]),
            Err(MixError::TrackIndex)
        );

        let out_of_range = [TrackInput {
            track: 5,
            samples: &a,
        }];
        assert_eq!(
            mixer.render(&out_of_range, &mut output, &[]),
            Err(MixError::TrackIndex)
        );

        let too_many = vec![
            MixEvent {
                offset: 0,
                track: 0,
                parameter: Parameter::Volume,
                value: 0.0
            };
            MAX_EVENTS + 1
        ];
        assert_eq!(
            mixer.render(&inputs, &mut output, &too_many),
            Err(MixError::EventCapacity)
        );
    }

    #[test]
    fn a_track_with_no_input_is_silent_but_still_advances() {
        let mut mixer = Mixer::new(2, RATE);
        let a = constant(64, 1.0, 1.0);
        let inputs = [TrackInput {
            track: 0,
            samples: &a,
        }];
        let output = render_settled(&mut mixer, &inputs, 64);
        assert!((output[63][0] - 1.0).abs() < 1e-4);
        // The absent track reads as silence rather than stale levels.
        let levels = mixer.levels(1);
        assert_eq!(levels.peak_left, 0.0);
        assert_eq!(levels.rms_left, 0.0);
    }

    #[test]
    fn meters_follow_the_signal() {
        let mut mixer = Mixer::new(1, RATE);
        let input = constant(2_048, 0.5, 0.25);
        let inputs = [TrackInput {
            track: 0,
            samples: &input,
        }];
        for _ in 0..8 {
            let mut output = silence(2_048);
            mixer.render(&inputs, &mut output, &[]).unwrap();
        }
        let track = mixer.levels(0);
        assert!((track.peak_left - 0.5).abs() < 1e-3, "{track:?}");
        assert!((track.peak_right - 0.25).abs() < 1e-3, "{track:?}");
        assert!(track.rms_left > 0.4 && track.rms_left <= 0.5, "{track:?}");
        assert!(!track.clipped);
        let master = mixer.levels(MASTER);
        assert!(master.peak_left > 0.4, "{master:?}");
    }

    #[test]
    fn clipping_is_reported_and_can_be_cleared() {
        let mut mixer = Mixer::new(1, RATE);
        let input = constant(256, 2.0, 2.0);
        let inputs = [TrackInput {
            track: 0,
            samples: &input,
        }];
        let mut output = silence(256);
        mixer.render(&inputs, &mut output, &[]).unwrap();
        assert!(mixer.levels(0).clipped);
        assert!(mixer.levels(MASTER).clipped);
        mixer.clear_clipping();
        assert!(!mixer.levels(0).clipped);
        assert!(!mixer.levels(MASTER).clipped);
    }

    #[test]
    fn levels_can_be_copied_out_in_one_call() {
        let mut mixer = Mixer::new(3, RATE);
        let a = constant(64, 1.0, 1.0);
        let inputs = [TrackInput {
            track: 0,
            samples: &a,
        }];
        let _ = render_settled(&mut mixer, &inputs, 64);
        let mut levels = [Levels::default(); 8];
        assert_eq!(mixer.copy_levels(&mut levels), 4);
        assert!(levels[0].peak_left > 0.5);
        assert_eq!(levels[1].peak_left, 0.0);
        assert!(levels[3].peak_left > 0.5, "master is last");
        // A short destination takes what fits.
        let mut two = [Levels::default(); 2];
        assert_eq!(mixer.copy_levels(&mut two), 2);
    }

    #[test]
    fn reset_clears_meters_and_settles_gains() {
        let mut mixer = Mixer::new(1, RATE);
        let input = constant(256, 1.0, 1.0);
        let inputs = [TrackInput {
            track: 0,
            samples: &input,
        }];
        let mut output = silence(256);
        mixer.render(&inputs, &mut output, &[]).unwrap();
        mixer.reset();
        assert_eq!(mixer.levels(0).peak_left, 0.0);
        // After a reset the very first sample is already at the target.
        mixer.set_volume_db(0, -6.020_6);
        mixer.render(&inputs, &mut output, &[]).unwrap();
        assert!((output[0][0] - 0.5).abs() < 1e-3, "{:?}", output[0]);
    }

    #[test]
    fn the_track_count_can_grow_and_shrink() {
        let mut mixer = Mixer::new(1, RATE);
        mixer.set_volume_db(0, -3.0);
        mixer.set_track_count(4);
        assert_eq!(mixer.track_count(), 4);
        // The existing strip keeps its setting; new ones start at unity.
        assert!((mixer.volume_db(0) + 3.0).abs() < 1e-6);
        assert_eq!(mixer.volume_db(3), 0.0);
        mixer.set_track_count(1);
        assert_eq!(mixer.track_count(), 1);
        // Out of range indices read as defaults rather than panicking.
        assert_eq!(mixer.volume_db(3), 0.0);
        assert!(!mixer.is_muted(3));
    }

    #[test]
    fn a_solo_cleared_by_shrinking_the_track_count_is_forgotten() {
        let mut mixer = Mixer::new(2, RATE);
        mixer.set_soloed(1, true);
        assert!(mixer.any_soloed());
        mixer.set_track_count(1);
        assert!(!mixer.any_soloed());
    }

    #[test]
    fn an_empty_block_is_accepted() {
        let mut mixer = Mixer::new(1, RATE);
        let empty: [[f32; 2]; 0] = [];
        let inputs = [TrackInput {
            track: 0,
            samples: &empty,
        }];
        let mut output: [[f32; 2]; 0] = [];
        assert_eq!(mixer.render(&inputs, &mut output, &[]), Ok(()));
        // An event at offset zero of an empty block still applies.
        let events = [MixEvent {
            offset: 0,
            track: 0,
            parameter: Parameter::Mute,
            value: 1.0,
        }];
        assert_eq!(mixer.render(&inputs, &mut output, &events), Ok(()));
        assert!(mixer.is_muted(0));
    }

    #[test]
    fn rendering_is_deterministic() {
        let build = || {
            let mut mixer = Mixer::new(4, RATE);
            mixer.set_volume_db(1, -4.0);
            mixer.set_pan(2, 0.6);
            mixer.set_muted(3, true);
            mixer
        };
        let signal: Vec<[f32; 2]> = (0..512)
            .map(|index| {
                let phase = index as f32 * 0.01;
                [phase.sin(), (phase * 1.5).cos()]
            })
            .collect();
        let inputs: Vec<TrackInput<'_>> = (0..4)
            .map(|track| TrackInput {
                track,
                samples: &signal,
            })
            .collect();
        let events = [
            MixEvent {
                offset: 100,
                track: 0,
                parameter: Parameter::Volume,
                value: -12.0,
            },
            MixEvent {
                offset: 300,
                track: 2,
                parameter: Parameter::Pan,
                value: -0.5,
            },
        ];
        let mut first = silence(512);
        let mut second = silence(512);
        let mut a = build();
        let mut b = build();
        for _ in 0..4 {
            a.render(&inputs, &mut first, &events).unwrap();
            b.render(&inputs, &mut second, &events).unwrap();
        }
        assert_eq!(first, second);
    }

    #[test]
    fn output_stays_finite_for_extreme_input() {
        let mut mixer = Mixer::new(2, RATE);
        let wild: Vec<[f32; 2]> = (0..256)
            .map(|index| match index % 4 {
                0 => [1e30, -1e30],
                1 => [f32::MIN_POSITIVE, -f32::MIN_POSITIVE],
                2 => [0.0, 0.0],
                _ => [1.0, -1.0],
            })
            .collect();
        let inputs = [
            TrackInput {
                track: 0,
                samples: &wild,
            },
            TrackInput {
                track: 1,
                samples: &wild,
            },
        ];
        let mut output = silence(256);
        mixer.render(&inputs, &mut output, &[]).unwrap();
        assert!(
            output
                .iter()
                .all(|frame| frame[0].is_finite() && frame[1].is_finite())
        );
    }
}
