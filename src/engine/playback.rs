//! The renderer that produces a project's audio.
//!
//! [`PlaybackEngine`] owns the mixer and the transport and implements
//! [`Renderer`](crate::audio::Renderer), so the same object feeds a device
//! stream and an offline bounce. It never reads the project model
//! directly: the control thread publishes a [`MixSettings`] snapshot
//! through a [`Publisher`], and the engine picks it up at a block
//! boundary. Nothing on this path allocates, locks, or blocks.
//!
//! There are no sound sources yet, so the engine mixes silence: every
//! track contributes nothing and the output is quiet. The transport still
//! runs, the meters still fall, and automation still lands on the sample
//! it was written for, so the plumbing is real even though nothing sounds.

use crate::audio::{BlockTiming, Renderer, StreamConfig};
use crate::engine::schedule::{
    MAX_NOTE_EVENTS, NoteAction, NoteEvent, ScheduledNote, Span, schedule_block, sort_notes,
};
use crate::engine::voice::{Patch, VoiceBank};
use crate::exchange::{AudioSlot, ControlSlot, exchange};
use crate::latest::{Reader, Writer, latest};
use crate::mixer::{Levels, MASTER, MAX_TRACKS, MixEvent, Mixer};
use crate::mixer::{MAX_FRAMES, TrackInput};
use crate::transport::{LoopRange, Transport};

/// Tracks that can carry an instrument. A project may hold more tracks
/// than this; the rest mix audio from elsewhere.
pub const MAX_INSTRUMENTS: usize = 8;
/// Notes one instrument track can hold at a time.
pub const MAX_NOTES_PER_TRACK: usize = 512;

/// Mixer state for one track, as the control thread sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackSettings {
    /// Volume in decibels.
    pub volume_db: f32,
    /// Pan from -1 to 1.
    pub pan: f32,
    /// Whether the track is silenced.
    pub muted: bool,
    /// Whether the track is soloed.
    pub soloed: bool,
}

impl Default for TrackSettings {
    fn default() -> Self {
        Self {
            volume_db: 0.0,
            pan: 0.0,
            muted: false,
            soloed: false,
        }
    }
}

/// Everything the engine needs to mix a project, in a form it can read
/// without touching the project model.
///
/// This is a fixed-size value so publishing one costs no allocation on the
/// audio thread. It is large, so the control thread should build it in
/// place rather than passing it around by value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixSettings {
    tracks: [TrackSettings; MAX_TRACKS],
    track_count: usize,
    master: TrackSettings,
    tempo: f64,
    numerator: u16,
    denominator: u16,
    loop_range: LoopRange,
}

impl Default for MixSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl MixSettings {
    /// Settings for an empty project at 120 beats per minute in four four.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tracks: [TrackSettings::default(); MAX_TRACKS],
            track_count: 0,
            master: TrackSettings::default(),
            tempo: 120.0,
            numerator: 4,
            denominator: 4,
            loop_range: LoopRange {
                start_beats: 0.0,
                length_beats: 0.0,
            },
        }
    }

    /// Number of tracks described.
    #[inline]
    #[must_use]
    pub const fn track_count(&self) -> usize {
        self.track_count
    }

    /// Sets how many tracks are described, capped at [`MAX_TRACKS`].
    pub fn set_track_count(&mut self, count: usize) {
        self.track_count = count.min(MAX_TRACKS);
    }

    /// Settings of one track, or the defaults when the index is past the
    /// track count.
    #[must_use]
    pub fn track(&self, index: usize) -> TrackSettings {
        if index < self.track_count {
            self.tracks[index]
        } else {
            TrackSettings::default()
        }
    }

    /// Replaces one track's settings. An index past [`MAX_TRACKS`] is
    /// ignored.
    pub fn set_track(&mut self, index: usize, settings: TrackSettings) {
        if index < MAX_TRACKS {
            self.tracks[index] = settings;
        }
    }

    /// Master strip settings.
    #[inline]
    #[must_use]
    pub const fn master(&self) -> TrackSettings {
        self.master
    }

    /// Replaces the master strip settings. Solo has no meaning on the
    /// master and is dropped.
    pub fn set_master(&mut self, settings: TrackSettings) {
        self.master = TrackSettings {
            soloed: false,
            ..settings
        };
    }

    /// Tempo in beats per minute.
    #[inline]
    #[must_use]
    pub const fn tempo(&self) -> f64 {
        self.tempo
    }

    /// Sets the tempo. The transport clamps it into its accepted range.
    pub fn set_tempo(&mut self, tempo: f64) {
        self.tempo = tempo;
    }

    /// Beats per bar and the note value that gets the beat.
    #[inline]
    #[must_use]
    pub const fn time_signature(&self) -> (u16, u16) {
        (self.numerator, self.denominator)
    }

    /// Sets the time signature. The transport refuses one it cannot use,
    /// in which case the previous signature stays.
    pub fn set_time_signature(&mut self, numerator: u16, denominator: u16) {
        self.numerator = numerator;
        self.denominator = denominator;
    }

    /// Loop the transport should follow.
    #[inline]
    #[must_use]
    pub const fn loop_range(&self) -> LoopRange {
        self.loop_range
    }

    /// Sets the loop.
    pub fn set_loop(&mut self, range: LoopRange) {
        self.loop_range = range;
    }
}

/// The notes an instrument track plays, and the sound it plays them with.
///
/// Notes are placed at absolute beats on the timeline, which is how an
/// arrangement reads. Session clip looping is not represented here yet.
#[derive(Clone, Copy)]
pub struct TrackScore {
    notes: [ScheduledNote; MAX_NOTES_PER_TRACK],
    count: usize,
    patch: Patch,
    /// Whether this track sounds at all.
    enabled: bool,
}

impl Default for TrackScore {
    fn default() -> Self {
        Self::new()
    }
}

impl TrackScore {
    /// An empty, silent track.
    #[must_use]
    pub fn new() -> Self {
        Self {
            notes: [ScheduledNote {
                start_beats: 0.0,
                length_beats: 0.0,
                pitch: 0,
                velocity: 0,
            }; MAX_NOTES_PER_TRACK],
            count: 0,
            patch: Patch::default(),
            enabled: false,
        }
    }

    /// Replaces the notes, keeping at most [`MAX_NOTES_PER_TRACK`] of
    /// them, and sorts them so the scheduler can walk them in order.
    /// Returns how many were kept.
    pub fn set_notes(&mut self, notes: &[ScheduledNote]) -> usize {
        let count = notes.len().min(MAX_NOTES_PER_TRACK);
        self.notes[..count].copy_from_slice(&notes[..count]);
        self.count = count;
        sort_notes(&mut self.notes[..count]);
        count
    }

    /// Notes the track holds.
    #[must_use]
    pub fn notes(&self) -> &[ScheduledNote] {
        &self.notes[..self.count]
    }

    /// The sound the notes are played with.
    #[inline]
    #[must_use]
    pub const fn patch(&self) -> Patch {
        self.patch
    }

    /// Sets the sound.
    pub fn set_patch(&mut self, patch: Patch) {
        self.patch = patch;
    }

    /// Whether the track sounds.
    #[inline]
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Turns the instrument on or off. A track with no instrument mixes
    /// silence rather than being skipped, so its meters still fall.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

/// What every instrument track plays.
#[derive(Clone, Copy)]
pub struct Score {
    tracks: [TrackScore; MAX_INSTRUMENTS],
}

impl Default for Score {
    fn default() -> Self {
        Self::new()
    }
}

impl Score {
    /// A score with no instrument playing.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tracks: [TrackScore::new(); MAX_INSTRUMENTS],
        }
    }

    /// One track's part, or `None` past [`MAX_INSTRUMENTS`].
    #[must_use]
    pub fn track(&self, index: usize) -> Option<&TrackScore> {
        self.tracks.get(index)
    }

    /// One track's part for editing, or `None` past [`MAX_INSTRUMENTS`].
    pub fn track_mut(&mut self, index: usize) -> Option<&mut TrackScore> {
        self.tracks.get_mut(index)
    }

    /// Instrument tracks that would sound.
    #[must_use]
    pub fn sounding_tracks(&self) -> usize {
        self.tracks.iter().filter(|track| track.enabled).count()
    }
}

/// What the engine reports back after a block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackState {
    /// Position in beats after the block.
    pub position_beats: f64,
    /// Position in frames after the block.
    pub position_frames: u64,
    /// Whether the transport is running.
    pub playing: bool,
    /// Levels of every track, then the master. Only the first
    /// `track_count + 1` entries carry a reading.
    pub levels: [Levels; MAX_TRACKS + 1],
    /// Tracks the levels describe.
    pub track_count: usize,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            position_beats: 0.0,
            position_frames: 0,
            playing: false,
            levels: [Levels::default(); MAX_TRACKS + 1],
            track_count: 0,
        }
    }
}

/// Control-thread end of the link to a running engine.
pub struct Publisher {
    settings: ControlSlot<MixSettings>,
    score: ControlSlot<Score>,
    state: Reader<PlaybackState>,
}

impl Publisher {
    /// Sends settings to the engine, which picks them up at the next block
    /// boundary.
    ///
    /// Returns false when a previous publication has not been taken yet;
    /// the caller keeps its copy and can try again after the next block.
    #[must_use]
    pub fn publish(&mut self, settings: &MixSettings) -> bool {
        // Reclaiming first keeps the exchange from stalling on its own
        // storage after a burst of changes.
        while self.settings.reclaim().is_some() {}
        self.settings.publish(*settings).is_ok()
    }

    /// Sends notes and instrument settings to the engine, taken up at the
    /// next block boundary.
    ///
    /// Returns false when a previous publication has not been taken yet.
    #[must_use]
    pub fn publish_score(&mut self, score: &Score) -> bool {
        while self.score.reclaim().is_some() {}
        self.score.publish(*score).is_ok()
    }

    /// Reads whatever the engine last reported. Repeats the previous
    /// reading when no new one has arrived.
    pub fn state(&mut self) -> PlaybackState {
        *self.state.current()
    }
}

/// The renderer that mixes a project.
pub struct PlaybackEngine {
    mixer: Mixer,
    transport: Transport,
    settings: AudioSlot<MixSettings>,
    score: AudioSlot<Score>,
    state: Writer<PlaybackState>,
    // Settings currently in force, kept so they can be read back.
    applied: MixSettings,
    applied_score: Score,
    instruments: [VoiceBank; MAX_INSTRUMENTS],
    // One buffer per instrument track, which the mixer then sums. Owned so
    // the render path borrows it rather than allocating.
    track_audio: [[[f32; 2]; MAX_FRAMES]; MAX_INSTRUMENTS],
}

impl PlaybackEngine {
    /// Builds an engine and the control-thread end of its link.
    ///
    /// Everything is allocated here, on the control thread, before any
    /// audio is produced.
    #[must_use]
    pub fn new(sample_rate: f64) -> (Self, Publisher) {
        let rate = if sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let (settings_control, settings_audio) = exchange(MixSettings::new());
        let (score_control, score_audio) = exchange(Score::new());
        let (state_writer, state_reader) = latest(PlaybackState::default());
        let engine = Self {
            mixer: Mixer::new(0, rate as f32),
            transport: Transport::new(rate, 120.0),
            settings: settings_audio,
            score: score_audio,
            state: state_writer,
            applied: MixSettings::new(),
            applied_score: Score::new(),
            instruments: core::array::from_fn(|_| VoiceBank::new(Patch::default(), rate as f32)),
            track_audio: [[[0.0; 2]; MAX_FRAMES]; MAX_INSTRUMENTS],
        };
        let publisher = Publisher {
            settings: settings_control,
            score: score_control,
            state: state_reader,
        };
        (engine, publisher)
    }

    /// The transport, for a caller driving the engine directly rather than
    /// through a stream.
    #[inline]
    pub fn transport(&mut self) -> &mut Transport {
        &mut self.transport
    }

    /// The mixer, for the same reason.
    #[inline]
    pub fn mixer(&mut self) -> &mut Mixer {
        &mut self.mixer
    }

    /// Settings currently in force.
    #[inline]
    #[must_use]
    pub const fn settings(&self) -> &MixSettings {
        &self.applied
    }

    /// Takes any published score and applies each track's patch.
    fn take_score(&mut self) {
        if !self.score.apply_pending() {
            return;
        }
        self.applied_score = *self.score.current();
        for index in 0..MAX_INSTRUMENTS {
            let Some(track) = self.applied_score.track(index) else {
                continue;
            };
            self.instruments[index].set_patch(track.patch());
            if !track.is_enabled() {
                // A track switched off stops at once rather than ringing.
                self.instruments[index].reset();
            }
        }
    }

    /// Takes any published settings and applies them to the mixer and the
    /// transport. Called at a block boundary, never mid-block, so a change
    /// cannot land halfway through a sum.
    fn take_settings(&mut self) {
        if !self.settings.apply_pending() {
            return;
        }
        let settings = *self.settings.current();
        self.applied = settings;
        self.mixer.set_track_count(settings.track_count());
        for index in 0..settings.track_count() {
            let track = settings.track(index);
            let strip = index as u16;
            self.mixer.set_volume_db(strip, track.volume_db);
            self.mixer.set_pan(strip, track.pan);
            self.mixer.set_muted(strip, track.muted);
            self.mixer.set_soloed(strip, track.soloed);
        }
        let master = settings.master();
        self.mixer.set_volume_db(MASTER, master.volume_db);
        self.mixer.set_pan(MASTER, master.pan);
        self.mixer.set_muted(MASTER, master.muted);
        self.transport.set_tempo(settings.tempo());
        let (numerator, denominator) = settings.time_signature();
        let _ = self.transport.set_time_signature(numerator, denominator);
        self.transport.set_loop(settings.loop_range());
    }

    /// Reports the position and levels back to the control thread. A
    /// report that cannot be sent is dropped rather than waited on: the
    /// next block carries fresher numbers anyway.
    fn report(&mut self) {
        let mut state = PlaybackState {
            position_beats: self.transport.position_beats(),
            position_frames: self.transport.position_frames(),
            playing: self.transport.is_playing(),
            levels: [Levels::default(); MAX_TRACKS + 1],
            track_count: self.mixer.track_count(),
        };
        self.mixer.copy_levels(&mut state.levels);
        // The buffer always has room; an unread report is replaced rather
        // than queued, so the control thread sees the newest numbers.
        self.state.publish(state);
    }

    /// Plays one instrument track into its own buffer, starting and
    /// releasing notes at the frames the scheduler placed them on.
    fn render_instrument(&mut self, index: usize, frames: usize, span: Span) {
        let enabled = self
            .applied_score
            .track(index)
            .is_some_and(TrackScore::is_enabled);
        let buffer = &mut self.track_audio[index][..frames];
        buffer.fill([0.0, 0.0]);
        let bank = &mut self.instruments[index];
        if !enabled {
            if !bank.is_silent() {
                bank.reset();
            }
            return;
        }

        let mut note_events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; MAX_NOTE_EVENTS];
        let scheduled = self
            .applied_score
            .track(index)
            .map(|track| schedule_block(track.notes(), span, &mut note_events))
            .unwrap_or_default();

        // Render in spans between note events so each note starts and
        // stops on the frame it was written for.
        let mut start = 0;
        for event in &note_events[..scheduled.count] {
            let at = event.offset.min(frames);
            if at > start {
                bank.render_additive(&mut buffer[start..at], 0.0);
                start = at;
            }
            match event.action {
                NoteAction::On => bank.note_on(event.pitch, event.velocity),
                NoteAction::Off => bank.note_off(event.pitch),
            }
        }
        if start < frames {
            bank.render_additive(&mut buffer[start..frames], 0.0);
        }
    }

    /// Mixes one block into `output`, advancing the transport.
    ///
    /// Instrument tracks play the notes the published score gives them,
    /// each into its own buffer, and the mixer sums those through the
    /// strips. `events` land on the sample they name.
    pub fn render_block(&mut self, output: &mut [[f32; 2]], events: &[MixEvent]) {
        self.take_settings();
        self.take_score();
        let frames = output.len().min(MAX_FRAMES);

        // The span this block covers, taken before the transport moves so
        // the notes land inside it.
        let (start_beats, length_beats) = self.transport.peek(frames);
        let span = Span {
            start_beats,
            length_beats,
            frames,
        };

        let instrument_tracks = MAX_INSTRUMENTS.min(self.mixer.track_count());
        for index in 0..instrument_tracks {
            self.render_instrument(index, frames, span);
        }

        // Borrowing the buffers and the mixer at once needs the fields
        // apart, since both live on this value.
        let Self {
            mixer, track_audio, ..
        } = self;
        let mut inputs = [TrackInput {
            track: 0,
            samples: &[],
        }; MAX_INSTRUMENTS];
        for (index, buffer) in track_audio.iter().take(instrument_tracks).enumerate() {
            inputs[index] = TrackInput {
                track: index as u16,
                samples: &buffer[..frames],
            };
        }
        if mixer
            .render(&inputs[..instrument_tracks], output, events)
            .is_err()
        {
            // A block the mixer refuses must still be silent rather than
            // whatever the device left in the buffer.
            output.fill([0.0, 0.0]);
        }
        self.transport.advance(frames);
        self.report();
    }

    /// The instrument on a track, for a caller driving the engine
    /// directly. `None` past [`MAX_INSTRUMENTS`].
    pub fn instrument(&mut self, index: usize) -> Option<&mut VoiceBank> {
        self.instruments.get_mut(index)
    }

    /// The score currently in force.
    #[inline]
    #[must_use]
    pub const fn score(&self) -> &Score {
        &self.applied_score
    }
}

impl Renderer for PlaybackEngine {
    fn render(&mut self, output: &mut [[f32; 2]], _timing: BlockTiming) {
        self.render_block(output, &[]);
    }

    fn prepare(&mut self, config: StreamConfig) {
        let rate = f64::from(config.sample_rate);
        self.transport.set_sample_rate(rate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::offline::{DEVICE, OfflineBackend};
    use crate::audio::{Backend, DeviceId, Stream, StreamConfig};
    use crate::mixer::Parameter;

    const RATE: f64 = 48_000.0;

    fn settings_with(count: usize) -> MixSettings {
        let mut settings = MixSettings::new();
        settings.set_track_count(count);
        for index in 0..count {
            settings.set_track(index, TrackSettings::default());
        }
        settings
    }

    #[test]
    fn a_new_engine_is_stopped_and_silent() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        let mut output = [[9.0_f32, 9.0]; 64];
        engine.render_block(&mut output, &[]);
        assert!(output.iter().all(|frame| *frame == [0.0, 0.0]));
        let state = publisher.state();
        assert!(!state.playing);
        assert_eq!(state.position_frames, 0);
    }

    #[test]
    fn published_settings_reach_the_mixer_at_a_block_boundary() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        let mut settings = settings_with(3);
        settings.set_track(
            1,
            TrackSettings {
                volume_db: -6.0,
                pan: -0.5,
                muted: true,
                soloed: false,
            },
        );
        settings.set_tempo(90.0);
        settings.set_time_signature(3, 4);
        assert!(publisher.publish(&settings));

        // Nothing has changed until a block runs.
        assert_eq!(engine.mixer().track_count(), 0);
        let mut output = [[0.0_f32; 2]; 64];
        engine.render_block(&mut output, &[]);

        assert_eq!(engine.mixer().track_count(), 3);
        assert!((engine.mixer().volume_db(1) + 6.0).abs() < 1e-6);
        assert!((engine.mixer().pan(1) + 0.5).abs() < 1e-6);
        assert!(engine.mixer().is_muted(1));
        assert!((engine.transport().tempo() - 90.0).abs() < 1e-9);
        assert_eq!(engine.transport().time_signature(), (3, 4));
    }

    #[test]
    fn a_refused_time_signature_leaves_the_previous_one() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        let mut settings = settings_with(1);
        settings.set_time_signature(5, 7);
        assert!(publisher.publish(&settings));
        let mut output = [[0.0_f32; 2]; 32];
        engine.render_block(&mut output, &[]);
        assert_eq!(engine.transport().time_signature(), (4, 4));
    }

    #[test]
    fn the_transport_advances_only_while_playing() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        let mut output = [[0.0_f32; 2]; 480];
        engine.render_block(&mut output, &[]);
        assert_eq!(publisher.state().position_frames, 0);

        engine.transport().play();
        engine.render_block(&mut output, &[]);
        let state = publisher.state();
        assert!(state.playing);
        assert_eq!(state.position_frames, 480);
        // At 120 bpm a beat is 24000 frames.
        assert!((state.position_beats - 0.02).abs() < 1e-9);
    }

    #[test]
    fn levels_are_reported_for_every_track_and_the_master() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&settings_with(4)));
        let mut output = [[0.0_f32; 2]; 128];
        engine.render_block(&mut output, &[]);
        let state = publisher.state();
        assert_eq!(state.track_count, 4);
        // Silence everywhere, but the readings are present and finite.
        for index in 0..=state.track_count {
            let levels = state.levels[index];
            assert!(levels.peak_left.is_finite());
            assert_eq!(levels.peak_left, 0.0);
            assert!(!levels.clipped);
        }
    }

    #[test]
    fn automation_events_are_applied_during_the_block() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&settings_with(2)));
        let mut output = [[0.0_f32; 2]; 128];
        engine.render_block(&mut output, &[]);
        let events = [MixEvent {
            offset: 64,
            track: 1,
            parameter: Parameter::Volume,
            value: -12.0,
        }];
        engine.render_block(&mut output, &events);
        assert!((engine.mixer().volume_db(1) + 12.0).abs() < 1e-6);
    }

    #[test]
    fn a_block_the_mixer_refuses_is_silent() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&settings_with(1)));
        let mut output = [[7.0_f32, 7.0]; 64];
        // An event naming a track that does not exist is refused.
        let events = [MixEvent {
            offset: 0,
            track: 200,
            parameter: Parameter::Volume,
            value: 0.0,
        }];
        engine.render_block(&mut output, &events);
        assert!(
            output.iter().all(|frame| *frame == [0.0, 0.0]),
            "a refused block left stale audio in the buffer"
        );
    }

    #[test]
    fn the_control_thread_sees_the_latest_state_after_several_blocks() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        engine.transport().play();
        let mut output = [[0.0_f32; 2]; 256];
        for _ in 0..10 {
            engine.render_block(&mut output, &[]);
        }
        let state = publisher.state();
        assert_eq!(state.position_frames, 2_560);
        // Reading again without a new block repeats the last reading.
        assert_eq!(publisher.state(), state);
    }

    #[test]
    fn settings_published_faster_than_blocks_run_do_not_stall() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        let mut output = [[0.0_f32; 2]; 64];
        for count in 1..=20 {
            // Publishing repeatedly between blocks must keep working; the
            // engine sees the most recent settings it managed to take.
            let _ = publisher.publish(&settings_with(count.min(MAX_TRACKS)));
            let _ = publisher.publish(&settings_with(count.min(MAX_TRACKS)));
            engine.render_block(&mut output, &[]);
        }
        assert!(engine.mixer().track_count() > 0);
    }

    #[test]
    fn the_engine_drives_an_offline_stream() {
        let (engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&settings_with(2)));
        let backend = OfflineBackend::new();
        let config = StreamConfig {
            device: DEVICE,
            sample_rate: 48_000,
            block_frames: 128,
            channels: 2,
        };
        let mut stream = backend.open_output(config, engine).unwrap();
        stream.start().unwrap();
        let mut output = vec![[1.0_f32; 2]; 512];
        assert_eq!(stream.render_frames(512, &mut output), Ok(512));
        assert_eq!(stream.frames_rendered(), 512);
        // Silence, because nothing produces sound yet, but written by the
        // engine rather than left as it was.
        assert!(output.iter().all(|frame| *frame == [0.0, 0.0]));
    }

    #[test]
    fn preparing_a_stream_sets_the_transport_rate() {
        let (mut engine, _publisher) = PlaybackEngine::new(RATE);
        engine.transport().locate_beats(4.0);
        engine.prepare(StreamConfig {
            device: DeviceId(0),
            sample_rate: 96_000,
            block_frames: 256,
            channels: 2,
        });
        assert!((engine.transport().sample_rate() - 96_000.0).abs() < 1e-9);
        // The musical position survives the rate change.
        assert!((engine.transport().position_beats() - 4.0).abs() < 1e-9);
    }

    fn note(start: f64, length: f64, pitch: u8) -> ScheduledNote {
        ScheduledNote {
            start_beats: start,
            length_beats: length,
            pitch,
            velocity: 110,
        }
    }

    fn peak(samples: &[[f32; 2]]) -> f32 {
        samples.iter().fold(0.0_f32, |worst, frame| {
            worst.max(frame[0].abs()).max(frame[1].abs())
        })
    }

    /// Plays `blocks` blocks of `frames` and returns the loudest sample.
    fn play(engine: &mut PlaybackEngine, blocks: usize, frames: usize) -> f32 {
        let mut output = vec![[0.0_f32; 2]; frames];
        let mut loudest = 0.0_f32;
        for _ in 0..blocks {
            engine.render_block(&mut output, &[]);
            loudest = loudest.max(peak(&output));
        }
        loudest
    }

    #[test]
    fn an_instrument_track_sounds_its_notes() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&settings_with(1)));
        let mut score = Score::new();
        let track = score.track_mut(0).unwrap();
        track.set_enabled(true);
        assert_eq!(track.set_notes(&[note(0.0, 2.0, 60)]), 1);
        assert!(publisher.publish_score(&score));

        // Stopped, nothing sounds even though a note sits at beat zero.
        assert_eq!(play(&mut engine, 2, 256), 0.0);

        engine.transport().play();
        let loudest = play(&mut engine, 40, 256);
        assert!(loudest > 0.001, "the note never sounded: {loudest}");
    }

    #[test]
    fn a_track_that_is_not_enabled_stays_silent() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&settings_with(1)));
        let mut score = Score::new();
        score.track_mut(0).unwrap().set_notes(&[note(0.0, 4.0, 60)]);
        assert!(publisher.publish_score(&score));
        engine.transport().play();
        assert_eq!(play(&mut engine, 20, 256), 0.0);
        assert_eq!(score.sounding_tracks(), 0);
    }

    #[test]
    fn a_note_starts_and_stops_at_its_written_position() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&settings_with(1)));
        let mut score = Score::new();
        let track = score.track_mut(0).unwrap();
        track.set_enabled(true);
        // One beat of silence, then a note for one beat, at 120 bpm: the
        // note runs from 0.5 s to 1.0 s.
        track.set_notes(&[note(1.0, 1.0, 64)]);
        assert!(publisher.publish_score(&score));
        engine.transport().play();

        // First half second: nothing.
        let before = play(&mut engine, 24_000 / 256, 256);
        assert!(before < 1e-6, "sound arrived early: {before}");
        // Next half second: the note.
        let during = play(&mut engine, 24_000 / 256, 256);
        assert!(during > 0.001, "the note did not sound: {during}");
        // A second later the release has finished.
        let after = play(&mut engine, 48_000 / 256, 256);
        assert!(after < during, "the note did not stop: {after} {during}");
    }

    #[test]
    fn a_muted_track_makes_no_sound_even_while_playing_notes() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        let mut settings = settings_with(1);
        settings.set_track(
            0,
            TrackSettings {
                muted: true,
                ..TrackSettings::default()
            },
        );
        assert!(publisher.publish(&settings));
        let mut score = Score::new();
        let track = score.track_mut(0).unwrap();
        track.set_enabled(true);
        track.set_notes(&[note(0.0, 4.0, 60)]);
        assert!(publisher.publish_score(&score));
        engine.transport().play();
        assert_eq!(play(&mut engine, 30, 256), 0.0);
    }

    #[test]
    fn several_instrument_tracks_sound_together() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&settings_with(3)));
        let mut score = Score::new();
        for (index, pitch) in [60, 64, 67].into_iter().enumerate() {
            let track = score.track_mut(index).unwrap();
            track.set_enabled(true);
            track.set_notes(&[note(0.0, 4.0, pitch)]);
        }
        assert_eq!(score.sounding_tracks(), 3);
        assert!(publisher.publish_score(&score));
        engine.transport().play();
        let three = play(&mut engine, 40, 256);
        assert!(three > 0.001);

        // Silencing two leaves less sound than all three.
        let mut fewer = score;
        fewer.track_mut(1).unwrap().set_enabled(false);
        fewer.track_mut(2).unwrap().set_enabled(false);
        assert!(publisher.publish_score(&fewer));
        let one = play(&mut engine, 40, 256);
        assert!(one < three, "one track was not quieter: {one} {three}");
    }

    #[test]
    fn a_score_holds_only_what_fits_and_sorts_what_it_keeps() {
        let mut score = Score::new();
        let track = score.track_mut(0).unwrap();
        let many: Vec<ScheduledNote> = (0..MAX_NOTES_PER_TRACK + 50)
            .map(|index| note(f64::from(index as u32) * 0.1, 0.05, 60))
            .collect();
        assert_eq!(track.set_notes(&many), MAX_NOTES_PER_TRACK);
        assert_eq!(track.notes().len(), MAX_NOTES_PER_TRACK);

        // Notes given out of order come back sorted.
        let mut other = Score::new();
        let track = other.track_mut(0).unwrap();
        track.set_notes(&[note(4.0, 1.0, 60), note(1.0, 1.0, 62), note(2.0, 1.0, 64)]);
        let starts: Vec<f64> = track.notes().iter().map(|note| note.start_beats).collect();
        assert_eq!(starts, vec![1.0, 2.0, 4.0]);

        // Tracks past the capacity are absent rather than a panic.
        assert!(score.track(MAX_INSTRUMENTS).is_none());
        assert!(score.track_mut(MAX_INSTRUMENTS).is_none());
    }

    #[test]
    fn switching_an_instrument_off_stops_it_at_once() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&settings_with(1)));
        let mut score = Score::new();
        let track = score.track_mut(0).unwrap();
        track.set_enabled(true);
        track.set_notes(&[note(0.0, 8.0, 60)]);
        assert!(publisher.publish_score(&score));
        engine.transport().play();
        assert!(play(&mut engine, 40, 256) > 0.001);

        let mut off = score;
        off.track_mut(0).unwrap().set_enabled(false);
        assert!(publisher.publish_score(&off));
        // The very next block is already silent: no release tail.
        let mut output = [[0.0_f32; 2]; 256];
        engine.render_block(&mut output, &[]);
        assert_eq!(peak(&output), 0.0);
    }

    #[test]
    fn the_patch_reaches_the_instrument() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&settings_with(1)));
        let mut score = Score::new();
        let track = score.track_mut(0).unwrap();
        track.set_enabled(true);
        track.set_patch(Patch {
            cutoff: 500.0,
            ..Patch::default()
        });
        assert!(publisher.publish_score(&score));
        let mut output = [[0.0_f32; 2]; 64];
        engine.render_block(&mut output, &[]);
        assert_eq!(engine.instrument(0).unwrap().patch().cutoff, 500.0);
        assert!(engine.instrument(MAX_INSTRUMENTS).is_none());
        assert_eq!(engine.score().sounding_tracks(), 1);
    }

    #[test]
    fn playing_the_same_notes_twice_gives_the_same_audio() {
        let render = || {
            let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
            assert!(publisher.publish(&settings_with(2)));
            let mut score = Score::new();
            for index in 0..2 {
                let track = score.track_mut(index).unwrap();
                track.set_enabled(true);
                track.set_notes(&[
                    note(0.0, 0.5, 60 + index as u8 * 7),
                    note(0.5, 0.5, 64),
                    note(1.0, 1.0, 67),
                ]);
            }
            assert!(publisher.publish_score(&score));
            engine.transport().play();
            let mut all = Vec::new();
            let mut output = [[0.0_f32; 2]; 256];
            for _ in 0..60 {
                engine.render_block(&mut output, &[]);
                all.extend_from_slice(&output);
            }
            all
        };
        assert_eq!(render(), render());
    }

    #[test]
    fn settings_describe_tracks_and_reject_out_of_range_indices() {
        let mut settings = MixSettings::new();
        assert_eq!(settings.track_count(), 0);
        settings.set_track_count(MAX_TRACKS + 100);
        assert_eq!(settings.track_count(), MAX_TRACKS);
        settings.set_track(
            0,
            TrackSettings {
                volume_db: -3.0,
                ..TrackSettings::default()
            },
        );
        assert!((settings.track(0).volume_db + 3.0).abs() < 1e-6);
        // Writing past the capacity is ignored rather than panicking.
        settings.set_track(MAX_TRACKS + 5, TrackSettings::default());
        // Reading past the count gives the defaults.
        settings.set_track_count(1);
        assert_eq!(settings.track(50), TrackSettings::default());
    }

    #[test]
    fn the_master_cannot_be_soloed_through_settings() {
        let mut settings = MixSettings::new();
        settings.set_master(TrackSettings {
            volume_db: -2.0,
            soloed: true,
            ..TrackSettings::default()
        });
        assert!(!settings.master().soloed);
        assert!((settings.master().volume_db + 2.0).abs() < 1e-6);
    }
}
