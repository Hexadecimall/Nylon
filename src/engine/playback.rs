//! The renderer that produces a project's audio.
//!
//! [`PlaybackEngine`] owns the mixer and the transport and implements
//! [`Renderer`](crate::audio::Renderer), so the same object feeds a device
//! stream and an offline bounce. It never reads the project model
//! directly: the control thread publishes mixer, timeline, score, and
//! routing revisions through a [`Publisher`], and the engine picks them up
//! at a block boundary. Nothing on this path allocates, locks, or blocks.
//!
//! MIDI tracks drive the built-in polyphonic instrument. Audio regions and
//! instrument output enter the same latency-compensated routing graph.

use crate::audio::{BlockTiming, Renderer, StreamConfig};
use crate::engine::automation::{
    EMPTY_EVENT as EMPTY_AUTOMATION_EVENT, Timeline as AutomationTimeline,
};
use crate::engine::device::{DeviceConfig, DeviceError, MAX_DELAY_STORAGE_FRAMES};
use crate::engine::graph::{GraphRenderError, GraphRenderer, NodeInput};
use crate::engine::rack::{DeviceRack, RackError};
use crate::engine::schedule::{
    MAX_NOTE_EVENTS, NoteAction, NoteEvent, ScheduledNote, Span, schedule_block,
    schedule_looped_block, sort_notes,
};
use crate::engine::timeline::AudioTimeline;
use crate::engine::voice::{Patch, VoiceBank};
use crate::exchange::{AudioSlot, ControlSlot, exchange};
use crate::latest::{Reader, Writer, latest};
use crate::mixer::{
    AutomationEvent, Levels, MASTER, MAX_AUTOMATION_EVENTS, MAX_TRACKS, MixEvent, MixPlan, Mixer,
};
use crate::mixer::{MAX_FRAMES, TrackInput};
use crate::routing::CompiledRouting;
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
    playing: bool,
    // Where to move the playhead, applied once when these settings are
    // taken up. `None` leaves it where it is.
    locate_beats: Option<f64>,
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
            playing: false,
            locate_beats: None,
        }
    }

    /// Whether the transport should run.
    #[inline]
    #[must_use]
    pub const fn is_playing(&self) -> bool {
        self.playing
    }

    /// Starts or stops the transport.
    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }

    /// Beat the playhead should move to when these settings are taken up,
    /// or `None` to leave it alone.
    #[inline]
    #[must_use]
    pub const fn locate_beats(&self) -> Option<f64> {
        self.locate_beats
    }

    /// Moves the playhead. The move happens once, at the block boundary
    /// where the settings are taken up.
    pub fn set_locate_beats(&mut self, beats: Option<f64>) {
        self.locate_beats = beats;
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

/// Loop placement for a launched Session clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SessionLoop {
    /// Transport beat where the launch takes effect.
    pub launch_beats: f64,
    /// First clip beat included in the loop.
    pub loop_start_beats: f64,
    /// Length of the loop in beats.
    pub loop_length_beats: f64,
}

/// The notes an instrument track plays, and the sound it plays them with.
#[derive(Clone, Copy)]
pub struct TrackScore {
    notes: [ScheduledNote; MAX_NOTES_PER_TRACK],
    count: usize,
    patch: Patch,
    session: Option<SessionLoop>,
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
            session: None,
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
        self.session = None;
        sort_notes(&mut self.notes[..count]);
        count
    }

    /// Replaces the notes with a Session clip that repeats after launch.
    /// Returns zero and leaves an empty part when the loop is invalid.
    pub fn set_session_notes(
        &mut self,
        notes: &[ScheduledNote],
        loop_start_beats: f64,
        loop_length_beats: f64,
        launch_beats: f64,
    ) -> usize {
        self.count = 0;
        self.session = None;
        if !loop_start_beats.is_finite()
            || loop_start_beats < 0.0
            || !loop_length_beats.is_finite()
            || loop_length_beats <= 0.0
            || !launch_beats.is_finite()
            || launch_beats < 0.0
        {
            return 0;
        }
        let count = notes.len().min(MAX_NOTES_PER_TRACK);
        self.notes[..count].copy_from_slice(&notes[..count]);
        self.count = count;
        self.session = Some(SessionLoop {
            launch_beats,
            loop_start_beats,
            loop_length_beats,
        });
        sort_notes(&mut self.notes[..count]);
        count
    }

    /// Notes the track holds.
    #[must_use]
    pub fn notes(&self) -> &[ScheduledNote] {
        &self.notes[..self.count]
    }

    /// Active Session loop, or `None` for Arrangement playback.
    #[inline]
    #[must_use]
    pub const fn session_loop(&self) -> Option<SessionLoop> {
        self.session
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

    /// A score with no instrument playing, built on the heap.
    ///
    /// A score is a large value, and a build without optimisation would
    /// pass one through the stack on its way into a box. Filling a vector
    /// a track at a time keeps the largest thing on the stack down to a
    /// single track.
    // off the audio thread
    #[must_use]
    pub fn boxed() -> Box<Self> {
        let mut score = Box::<Self>::new_uninit();
        let pointer = score.as_mut_ptr();
        for index in 0..MAX_INSTRUMENTS {
            // SAFETY: The allocation is the size of a score, so every
            // track slot is within it, and each is written exactly once
            // before the value is treated as initialised.
            unsafe { (&raw mut (*pointer).tracks[index]).write(TrackScore::new()) };
        }
        // SAFETY: Every field of a score is one of the tracks written
        // above, so the whole value is now initialised.
        unsafe { score.assume_init() }
    }
    // back on the audio thread

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
    /// Persistent automation events dropped at the fixed block limit.
    pub automation_dropped: u64,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            position_beats: 0.0,
            position_frames: 0,
            playing: false,
            levels: [Levels::default(); MAX_TRACKS + 1],
            track_count: 0,
            automation_dropped: 0,
        }
    }
}

/// Control-thread end of the link to a running engine.
// off the audio thread
pub struct Publisher {
    settings: Writer<MixSettings>,
    score: ControlSlot<Box<Score>>,
    audio: ControlSlot<Box<AudioTimeline>>,
    automation: ControlSlot<Box<AutomationTimeline>>,
    routing: ControlSlot<Option<Box<PublishedRouting>>>,
    state: Reader<PlaybackState>,
    sample_rate: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutingPublishError {
    Graph(GraphRenderError),
    Device(DeviceError),
}

impl Publisher {
    /// Sends settings to the engine, which picks them up at the next block
    /// boundary.
    ///
    /// Newer settings replace unread settings so transport commands cannot
    /// be stranded behind an initial state publication.
    #[must_use]
    pub fn publish(&mut self, settings: &MixSettings) -> bool {
        self.settings.publish(*settings);
        true
    }

    /// Sends notes and instrument settings to the engine, taken up at the
    /// next block boundary.
    ///
    /// Returns false when a previous publication has not been taken yet.
    #[must_use]
    pub fn publish_score(&mut self, score: &Score) -> bool {
        while self.score.reclaim().is_some() {}
        // The score is boxed here, on the control thread, so that what
        // crosses to the audio thread is a pointer rather than a copy of
        // every note. The engine hands the old box back for release here.
        let mut published = Score::boxed();
        *published = *score;
        self.score.publish(published).is_ok()
    }

    /// Transfers a prepared audio timeline to the engine.
    #[must_use]
    pub fn publish_audio(&mut self, timeline: AudioTimeline) -> bool {
        while self.audio.reclaim().is_some() {}
        self.audio.publish(Box::new(timeline)).is_ok()
    }

    /// Transfers prepared persistent automation to the engine.
    #[must_use]
    pub fn publish_automation(&mut self, timeline: AutomationTimeline) -> bool {
        while self.automation.reclaim().is_some() {}
        self.automation.publish(Box::new(timeline)).is_ok()
    }

    /// Builds and transfers a routing revision to the engine.
    ///
    /// Construction and allocation happen on the control thread. The old
    /// revision returns here for destruction after the callback swaps it.
    pub fn publish_routing(
        &mut self,
        compiled: &CompiledRouting,
        output_node: u16,
    ) -> Result<bool, RoutingPublishError> {
        if compiled.output_latency(output_node).is_none() {
            return Err(RoutingPublishError::Graph(GraphRenderError::InvalidNode));
        }
        while self.routing.reclaim().is_some() {}
        let routing = PublishedRouting {
            renderer: GraphRenderer::new(compiled, MAX_FRAMES)
                .map_err(RoutingPublishError::Graph)?,
            output_node,
            devices: empty_device_racks(compiled.node_count(), self.sample_rate)
                .map_err(RoutingPublishError::Device)?,
        };
        Ok(self.routing.publish(Some(Box::new(routing))).is_ok())
    }

    /// Builds routing and per-node device chains as one callback revision.
    pub fn publish_routing_with_devices(
        &mut self,
        compiled: &CompiledRouting,
        output_node: u16,
        node_devices: &[&[DeviceConfig]],
    ) -> Result<bool, RoutingPublishError> {
        if compiled.output_latency(output_node).is_none() {
            return Err(RoutingPublishError::Graph(GraphRenderError::InvalidNode));
        }
        while self.routing.reclaim().is_some() {}
        let mut devices = Vec::new();
        devices
            .try_reserve_exact(compiled.node_count())
            .map_err(|_| RoutingPublishError::Device(DeviceError::StorageCapacity))?;
        let mut delay_storage = 0_usize;
        for node in 0..compiled.node_count() {
            let rack = DeviceRack::native(
                node_devices.get(node).copied().unwrap_or(&[]),
                self.sample_rate,
                MAX_FRAMES,
            )
            .map_err(|error| RoutingPublishError::Device(device_error(error)))?;
            delay_storage = delay_storage
                .checked_add(rack.delay_storage_frames())
                .filter(|frames| *frames <= MAX_DELAY_STORAGE_FRAMES)
                .ok_or(RoutingPublishError::Device(DeviceError::StorageCapacity))?;
            devices.push(rack);
        }
        let routing = PublishedRouting {
            renderer: GraphRenderer::new(compiled, MAX_FRAMES)
                .map_err(RoutingPublishError::Graph)?,
            output_node,
            devices,
        };
        Ok(self.routing.publish(Some(Box::new(routing))).is_ok())
    }

    /// Transfers a routing revision with already prepared mixed-device racks.
    pub fn publish_prepared_routing(
        &mut self,
        compiled: &CompiledRouting,
        output_node: u16,
        devices: Vec<DeviceRack>,
    ) -> Result<bool, RoutingPublishError> {
        if compiled.output_latency(output_node).is_none() {
            return Err(RoutingPublishError::Graph(GraphRenderError::InvalidNode));
        }
        if devices.len() != compiled.node_count() {
            return Err(RoutingPublishError::Device(DeviceError::Capacity));
        }
        let delay_storage = devices.iter().try_fold(0_usize, |total, rack| {
            total.checked_add(rack.delay_storage_frames())
        });
        if delay_storage.is_none_or(|frames| frames > MAX_DELAY_STORAGE_FRAMES) {
            return Err(RoutingPublishError::Device(DeviceError::StorageCapacity));
        }
        while self.routing.reclaim().is_some() {}
        let routing = PublishedRouting {
            renderer: GraphRenderer::new(compiled, MAX_FRAMES)
                .map_err(RoutingPublishError::Graph)?,
            output_node,
            devices,
        };
        Ok(self.routing.publish(Some(Box::new(routing))).is_ok())
    }
    /// Reads whatever the engine last reported. Repeats the previous
    /// reading when no new one has arrived.
    pub fn state(&mut self) -> PlaybackState {
        *self.state.current()
    }
}

struct PublishedRouting {
    renderer: GraphRenderer,
    output_node: u16,
    devices: Vec<DeviceRack>,
}

fn empty_device_racks(count: usize, sample_rate: f32) -> Result<Vec<DeviceRack>, DeviceError> {
    let mut chains = Vec::new();
    chains
        .try_reserve_exact(count)
        .map_err(|_| DeviceError::StorageCapacity)?;
    for _ in 0..count {
        chains.push(DeviceRack::native(&[], sample_rate, MAX_FRAMES).map_err(device_error)?);
    }
    Ok(chains)
}

fn device_error(error: RackError) -> DeviceError {
    match error {
        RackError::Capacity => DeviceError::Capacity,
        RackError::BufferSize => DeviceError::BufferSize,
        RackError::Device(error) => error,
        RackError::StorageCapacity => DeviceError::StorageCapacity,
    }
}

/// Builds the per-track buffers directly on the heap.
///
/// A vector grows into an allocation of its own, so the whole block of
/// silence never passes through the stack the way `Box::new` of a large
/// array would in a build without optimisation.
fn zeroed_track_audio() -> Box<[[[f32; 2]; MAX_FRAMES]; MAX_TRACKS]> {
    let buffers: Vec<[[f32; 2]; MAX_FRAMES]> = vec![[[0.0; 2]; MAX_FRAMES]; MAX_TRACKS];
    // The vector was built with exactly this many elements, so the
    // conversion cannot fail.
    buffers
        .into_boxed_slice()
        .try_into()
        .unwrap_or_else(|_| unreachable!())
}

/// Builds instrument banks one at a time into heap storage.
fn voice_banks(sample_rate: f32) -> Box<[VoiceBank]> {
    let mut banks = Vec::with_capacity(MAX_INSTRUMENTS);
    for _ in 0..MAX_INSTRUMENTS {
        banks.push(VoiceBank::new(Patch::default(), sample_rate));
    }
    banks.into_boxed_slice()
}

/// The renderer that mixes a project.
pub struct PlaybackEngine {
    mixer: Mixer,
    mix_plan: MixPlan,
    transport: Transport,
    settings: Reader<MixSettings>,
    score: AudioSlot<Box<Score>>,
    audio: AudioSlot<Box<AudioTimeline>>,
    automation: AudioSlot<Box<AutomationTimeline>>,
    routing: AudioSlot<Option<Box<PublishedRouting>>>,
    state: Writer<PlaybackState>,
    // Settings currently in force, kept so they can be read back.
    applied: MixSettings,
    // The score, the voice banks and the per-track buffers are the bulk of
    // an engine, and they sit behind boxes so that moving an engine moves a
    // handful of pointers rather than several hundred kilobytes. A build
    // without optimisation copies a returned value through the stack, and a
    // thread with a small stack cannot afford that.
    applied_score: Box<Score>,
    instruments: Box<[VoiceBank]>,
    // One buffer per instrument track, which the mixer then sums. Owned so
    // the render path borrows it rather than allocating.
    track_audio: Box<[[[f32; 2]; MAX_FRAMES]; MAX_TRACKS]>,
    automation_dropped: u64,
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
        let (settings_control, settings_audio) = latest(MixSettings::new());
        let (score_control, score_audio) = exchange(Score::boxed());
        let (audio_control, audio_audio) = exchange(Box::new(AudioTimeline::new()));
        let (automation_control, automation_audio) = exchange(Box::new(AutomationTimeline::new()));
        let (routing_control, routing_audio) = exchange(None);
        let (state_writer, state_reader) = latest(PlaybackState::default());
        let engine = Self {
            mixer: Mixer::new(0, rate as f32),
            mix_plan: MixPlan::new(),
            transport: Transport::new(rate, 120.0),
            settings: settings_audio,
            score: score_audio,
            audio: audio_audio,
            automation: automation_audio,
            routing: routing_audio,
            state: state_writer,
            applied: MixSettings::new(),
            applied_score: Score::boxed(),
            instruments: voice_banks(rate as f32),
            track_audio: zeroed_track_audio(),
            automation_dropped: 0,
        };
        let publisher = Publisher {
            settings: settings_control,
            score: score_control,
            audio: audio_control,
            automation: automation_control,
            routing: routing_control,
            state: state_reader,
            sample_rate: rate as f32,
        };
        (engine, publisher)
    }
    // back on the audio thread

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
        let previous_sessions: [Option<SessionLoop>; MAX_INSTRUMENTS] =
            core::array::from_fn(|index| {
                self.applied_score
                    .track(index)
                    .and_then(TrackScore::session_loop)
            });
        *self.applied_score = **self.score.current();
        for (index, previous_session) in previous_sessions.iter().enumerate() {
            let Some(track) = self.applied_score.track(index) else {
                continue;
            };
            self.instruments[index].set_patch(track.patch());
            let session_changed = *previous_session != track.session_loop();
            if !track.is_enabled() || session_changed {
                // A disabled or relaunched part stops at once rather than ringing.
                self.instruments[index].reset();
            }
        }
    }

    /// Takes prepared media ownership without releasing the previous value
    /// on this thread.
    fn take_audio(&mut self) {
        self.audio.apply_pending();
    }

    fn take_automation(&mut self) {
        if self.automation.apply_pending() {
            self.mixer.clear_automation();
        }
    }

    /// Takes a prepared routing revision without releasing its predecessor
    /// on this thread.
    fn take_routing(&mut self) {
        self.routing.apply_pending();
    }

    /// Takes any published settings and applies them to the mixer and the
    /// transport. Called at a block boundary, never mid-block, so a change
    /// cannot land halfway through a sum.
    fn take_settings(&mut self) {
        let Some(settings) = self.settings.read().copied() else {
            return;
        };
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
        if let Some(beats) = settings.locate_beats() {
            self.transport.locate_beats(beats);
            self.mixer.clear_automation();
            // Moving the playhead abandons whatever was sounding, so no
            // note hangs on from where playback used to be.
            for bank in self.instruments.iter_mut() {
                bank.reset();
            }
        }
        if settings.is_playing() != self.transport.is_playing() {
            if settings.is_playing() {
                self.transport.play();
            } else {
                self.transport.stop();
                // Stopping releases notes rather than cutting them dead.
                for bank in self.instruments.iter_mut() {
                    bank.all_notes_off();
                }
            }
        }
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
            automation_dropped: self.automation_dropped,
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
            .map_or_else(Default::default, |track| {
                if let Some(session) = track.session_loop() {
                    schedule_looped_block(
                        track.notes(),
                        session.loop_start_beats,
                        session.loop_length_beats,
                        session.launch_beats,
                        span,
                        &mut note_events,
                    )
                } else {
                    schedule_block(track.notes(), span, &mut note_events)
                }
            });

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
    /// each into its own buffer. A published graph routes those buffers;
    /// the direct mixer remains the fallback. `events` land on the sample
    /// they name.
    pub fn render_block(&mut self, output: &mut [[f32; 2]], events: &[MixEvent]) {
        self.take_settings();
        self.take_score();
        self.take_audio();
        self.take_automation();
        self.take_routing();
        let frames = output.len().min(MAX_FRAMES);

        // The span this block covers, taken before the transport moves so
        // the notes land inside it.
        let (start_beats, length_beats) = self.transport.peek(frames);
        let span = Span {
            start_beats,
            length_beats,
            frames,
        };
        let mut automation_events = [EMPTY_AUTOMATION_EVENT; MAX_AUTOMATION_EVENTS];
        let scheduled = self
            .automation
            .current()
            .schedule(span, &mut automation_events);
        self.automation_dropped = self
            .automation_dropped
            .saturating_add(scheduled.dropped as u64);
        let automation_events = &automation_events[..scheduled.count];

        let track_count = self.mixer.track_count();
        for buffer in self.track_audio.iter_mut().take(track_count) {
            buffer[..frames].fill([0.0; 2]);
        }
        let instrument_tracks = MAX_INSTRUMENTS.min(track_count);
        for index in 0..instrument_tracks {
            self.render_instrument(index, frames, span);
        }
        for (index, buffer) in self.track_audio.iter_mut().take(track_count).enumerate() {
            self.audio
                .current()
                .render_track(index, span, &mut buffer[..frames]);
        }

        if self.routing.current().is_some() {
            self.render_routed(output, events, automation_events, frames, track_count);
        } else {
            self.render_direct(output, events, automation_events, frames, track_count);
        }
        output[frames..].fill([0.0; 2]);
        self.transport.advance(frames);
        self.report();
    }

    fn render_direct(
        &mut self,
        output: &mut [[f32; 2]],
        events: &[MixEvent],
        automation: &[AutomationEvent],
        frames: usize,
        track_count: usize,
    ) {
        // Borrowing the buffers and the mixer at once needs the fields
        // apart, since both live on this value.
        let Self {
            mixer, track_audio, ..
        } = self;
        let mut inputs = [TrackInput {
            track: 0,
            samples: &[],
        }; MAX_TRACKS];
        for (index, buffer) in track_audio.iter().take(track_count).enumerate() {
            inputs[index] = TrackInput {
                track: index as u16,
                samples: &buffer[..frames],
            };
        }
        if mixer
            .render_automated(&inputs[..track_count], output, events, automation)
            .is_err()
        {
            // A block the mixer refuses must still be silent rather than
            // whatever the device left in the buffer.
            output.fill([0.0, 0.0]);
        }
    }

    fn render_routed(
        &mut self,
        output: &mut [[f32; 2]],
        events: &[MixEvent],
        automation: &[AutomationEvent],
        frames: usize,
        track_count: usize,
    ) {
        if self.mixer.validate_events(frames, events).is_err()
            || self.mixer.validate_automation(frames, automation).is_err()
        {
            output.fill([0.0; 2]);
            return;
        }
        self.mixer
            .prepare_plan(&mut self.mix_plan, frames, events, automation);
        let mut inputs = [NodeInput {
            node: 0,
            samples: &[],
        }; MAX_TRACKS];
        for (index, buffer) in self.track_audio.iter().take(track_count).enumerate() {
            inputs[index] = NodeInput {
                node: index as u16,
                samples: &buffer[..frames],
            };
        }

        let Self {
            mixer,
            mix_plan,
            routing,
            ..
        } = self;
        let Some(routing) = routing.current_mut().as_mut() else {
            output[..frames].fill([0.0; 2]);
            return;
        };
        let PublishedRouting {
            renderer,
            output_node,
            devices,
        } = routing.as_mut();
        let output_node = *output_node;
        if renderer
            .render(
                &inputs[..track_count],
                output_node,
                &mut output[..frames],
                |node, main, sidechain, pre, post| {
                    if devices[usize::from(node)]
                        .process(main, sidechain, pre)
                        .is_err()
                    {
                        pre.fill([0.0; 2]);
                    }
                    if node == output_node {
                        mixer.process_master_planned(mix_plan, pre, post);
                    } else {
                        mixer.process_track_planned(node, mix_plan, pre, post);
                    }
                },
            )
            .is_err()
        {
            output[..frames].fill([0.0; 2]);
        }
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
    use crate::engine::automation::{
        Lane as AutomationLane, Point as AutomationPoint, Timeline as AutomationTimeline,
    };
    use crate::engine::rack::DeviceRack;
    use crate::mixer::AutomationCurve;
    use crate::mixer::Parameter;
    use crate::plugin::bridge::{BlockProcessor, Bridge};
    use crate::plugin::clap::{NoteEvent as PluginNoteEvent, ParameterEvent};

    const RATE: f64 = 48_000.0;

    struct PluginGain(f32);

    impl BlockProcessor for PluginGain {
        fn process_block(
            &mut self,
            input: Option<(&[f32], &[f32])>,
            output_left: &mut [f32],
            output_right: &mut [f32],
            _: &[ParameterEvent],
            _: &[PluginNoteEvent],
        ) -> bool {
            let Some((left, right)) = input else {
                return false;
            };
            for index in 0..left.len() {
                output_left[index] = left[index] * self.0;
                output_right[index] = right[index] * self.0;
            }
            true
        }
    }

    /// The most an engine may take on the stack when it is moved. A test
    /// thread gets two mebibytes, and a build without optimisation copies a
    /// returned value through several frames, so an engine that carried its
    /// buffers inline would exhaust that stack before the test ran.
    const STACK_BUDGET: usize = 128 * 1024;

    #[test]
    fn an_engine_is_small_enough_to_move() {
        let size = core::mem::size_of::<PlaybackEngine>();
        assert!(
            size <= STACK_BUDGET,
            "an engine is {size} bytes, over the {STACK_BUDGET} byte budget"
        );
    }

    #[test]
    fn an_engine_builds_and_renders_on_a_small_stack() {
        // Half a mebibyte, well under what a test thread is given, so a
        // regression that puts the buffers back on the stack fails here
        // rather than aborting the whole run with a stack overflow.
        let thread = std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
                assert!(publisher.publish(&playing_settings(2)));
                let mut output = [[0.0_f32; 2]; 128];
                engine.render(&mut output, BlockTiming::default());
                engine.transport().position_beats()
            })
            .expect("the thread could not be started");
        let position = thread.join().expect("the engine overflowed the stack");
        assert!(position > 0.0, "the transport did not move");
    }

    /// Settings for `count` tracks with the transport already running.
    fn playing_settings(count: usize) -> MixSettings {
        let mut settings = settings_with(count);
        settings.set_playing(true);
        settings
    }

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
    fn published_persistent_automation_reaches_the_mixer() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&playing_settings(1)));
        let mut automation = AutomationTimeline::new();
        assert!(automation.add_lane(AutomationLane {
            track: 0,
            parameter: Parameter::Volume,
            points: vec![
                AutomationPoint {
                    beat: 0.0,
                    value: -12.0,
                    curve: AutomationCurve::Linear,
                },
                AutomationPoint {
                    beat: 0.002,
                    value: 0.0,
                    curve: AutomationCurve::Step,
                },
            ],
        }));
        assert!(publisher.publish_automation(automation));
        let mut output = [[0.0_f32; 2]; 96];
        engine.render_block(&mut output, &[]);
        assert_eq!(engine.mixer().volume_db(0), 0.0);
        assert_eq!(publisher.state().automation_dropped, 0);
    }

    #[test]
    fn a_published_routing_graph_drives_the_live_mix() {
        use crate::engine::sample::Sample;
        use crate::engine::timeline::{AudioRegion, AudioTimeline};
        use crate::routing::{Edge, EdgeKind, RoutingGraph};

        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        let mut settings = playing_settings(2);
        settings.set_track(
            1,
            TrackSettings {
                volume_db: -6.020_6,
                ..TrackSettings::default()
            },
        );
        assert!(publisher.publish(&settings));

        let mut graph = RoutingGraph::new(3).unwrap();
        graph
            .add_edge(Edge {
                source: 0,
                destination: 1,
                kind: EdgeKind::Main,
                gain: 0.5,
            })
            .unwrap();
        graph
            .add_edge(Edge {
                source: 1,
                destination: 2,
                kind: EdgeKind::Main,
                gain: 1.0,
            })
            .unwrap();
        assert_eq!(
            publisher.publish_routing(&graph.compile().unwrap(), 9),
            Err(RoutingPublishError::Graph(GraphRenderError::InvalidNode))
        );
        let utility = [DeviceConfig {
            enabled: true,
            kind: crate::engine::device::DeviceKind::Utility {
                gain_db: -6.020_6,
                width: 1.0,
                balance: 0.0,
            },
        }];
        let node_devices: [&[DeviceConfig]; 3] = [&utility, &[], &[]];
        assert!(
            publisher
                .publish_routing_with_devices(&graph.compile().unwrap(), 2, &node_devices)
                .unwrap()
        );

        let mut timeline = AudioTimeline::new();
        let media = timeline
            .add_sample(Sample::new(48_000, vec![[0.4, -0.4]; 512]).unwrap())
            .unwrap();
        timeline
            .add_region(AudioRegion::new(media, 0, 0.0, 1.0, 0.0, 512.0).unwrap())
            .unwrap();
        assert!(publisher.publish_audio(timeline));

        let mut output = [[0.0_f32; 2]; 64];
        engine.render_block(&mut output, &[]);
        for frame in output {
            assert!((frame[0] - 0.05).abs() < 1e-4, "{frame:?}");
            assert!((frame[1] + 0.05).abs() < 1e-4, "{frame:?}");
        }
    }

    #[test]
    fn routed_mixer_events_match_the_direct_path() {
        use crate::engine::sample::Sample;
        use crate::engine::timeline::{AudioRegion, AudioTimeline};
        use crate::routing::{Edge, EdgeKind, RoutingGraph};

        let (mut direct, mut direct_publisher) = PlaybackEngine::new(RATE);
        let (mut routed, mut routed_publisher) = PlaybackEngine::new(RATE);
        let settings = playing_settings(1);
        assert!(direct_publisher.publish(&settings));
        assert!(routed_publisher.publish(&settings));

        let mut timeline = AudioTimeline::new();
        let media = timeline
            .add_sample(Sample::new(48_000, vec![[0.25, -0.5]; 512]).unwrap())
            .unwrap();
        timeline
            .add_region(AudioRegion::new(media, 0, 0.0, 1.0, 0.0, 512.0).unwrap())
            .unwrap();
        assert!(direct_publisher.publish_audio(timeline.clone()));
        assert!(routed_publisher.publish_audio(timeline));

        let mut graph = RoutingGraph::new(2).unwrap();
        graph
            .add_edge(Edge {
                source: 0,
                destination: 1,
                kind: EdgeKind::Main,
                gain: 1.0,
            })
            .unwrap();
        assert!(
            routed_publisher
                .publish_routing(&graph.compile().unwrap(), 1)
                .unwrap()
        );

        let events = [
            MixEvent {
                offset: 17,
                track: 0,
                parameter: Parameter::Volume,
                value: -9.0,
            },
            MixEvent {
                offset: 43,
                track: 0,
                parameter: Parameter::Pan,
                value: 0.7,
            },
            MixEvent {
                offset: 64,
                track: MASTER,
                parameter: Parameter::Volume,
                value: -3.0,
            },
        ];
        let mut direct_output = [[0.0_f32; 2]; 96];
        let mut routed_output = [[0.0_f32; 2]; 96];
        direct.render_block(&mut direct_output, &events);
        routed.render_block(&mut routed_output, &events);
        for (routed_frame, direct_frame) in routed_output.iter().zip(direct_output) {
            assert!((routed_frame[0] - direct_frame[0]).abs() < 1e-6);
            assert!((routed_frame[1] - direct_frame[1]).abs() < 1e-6);
        }
    }

    #[test]
    fn a_prepared_plugin_rack_processes_inside_the_live_graph() {
        use crate::engine::sample::Sample;
        use crate::engine::timeline::{AudioRegion, AudioTimeline};
        use crate::routing::{Edge, EdgeKind, RoutingGraph};

        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&playing_settings(1)));
        let mut timeline = AudioTimeline::new();
        let media = timeline
            .add_sample(Sample::new(48_000, vec![[0.2, -0.2]; 256]).unwrap())
            .unwrap();
        let mut region = AudioRegion::new(media, 0, 0.0, 100.0, 0.0, 512.0).unwrap();
        assert!(region.set_loop(Some(0..256)));
        timeline.add_region(region).unwrap();
        assert!(publisher.publish_audio(timeline));

        let utility = DeviceConfig {
            enabled: true,
            kind: crate::engine::device::DeviceKind::Utility {
                gain_db: -6.020_6,
                width: 1.0,
                balance: 0.0,
            },
        };
        let mut track_rack = DeviceRack::new(64).unwrap();
        track_rack.push_native(&[utility], RATE as f32).unwrap();
        track_rack
            .push_plugin(Bridge::new(PluginGain(2.0), 64, 2, 0).unwrap())
            .unwrap();
        track_rack.push_native(&[utility], RATE as f32).unwrap();
        let master_rack = DeviceRack::new(64).unwrap();
        let mut graph = RoutingGraph::new(2).unwrap();
        graph
            .add_edge(Edge {
                source: 0,
                destination: 1,
                kind: EdgeKind::Main,
                gain: 1.0,
            })
            .unwrap();
        assert!(
            publisher
                .publish_prepared_routing(
                    &graph.compile().unwrap(),
                    1,
                    vec![track_rack, master_rack],
                )
                .unwrap()
        );

        let mut output = [[0.0_f32; 2]; 64];
        for _ in 0..10_000 {
            engine.render_block(&mut output, &[]);
            if (output[0][0] - 0.1).abs() < 1e-4 {
                break;
            }
            std::thread::yield_now();
        }
        for frame in output {
            assert!((frame[0] - 0.1).abs() < 1e-4, "{frame:?}");
            assert!((frame[1] + 0.1).abs() < 1e-4, "{frame:?}");
        }
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
            // Publishing repeatedly between blocks keeps the latest value.
            assert!(publisher.publish(&settings_with(count.min(MAX_TRACKS))));
            assert!(publisher.publish(&settings_with(count.min(MAX_TRACKS))));
            engine.render_block(&mut output, &[]);
        }
        assert_eq!(engine.mixer().track_count(), 20);
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

        assert!(publisher.publish(&playing_settings(1)));

        let loudest = play(&mut engine, 40, 256);
        assert!(loudest > 0.001, "the note never sounded: {loudest}");
    }

    #[test]
    fn a_track_that_is_not_enabled_stays_silent() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&playing_settings(1)));
        let mut score = Score::new();
        score.track_mut(0).unwrap().set_notes(&[note(0.0, 4.0, 60)]);
        assert!(publisher.publish_score(&score));
        assert_eq!(play(&mut engine, 20, 256), 0.0);
        assert_eq!(score.sounding_tracks(), 0);
    }

    #[test]
    fn a_note_starts_and_stops_at_its_written_position() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&playing_settings(1)));
        let mut score = Score::new();
        let track = score.track_mut(0).unwrap();
        track.set_enabled(true);
        // One beat of silence, then a note for one beat, at 120 bpm: the
        // note runs from 0.5 s to 1.0 s.
        track.set_notes(&[note(1.0, 1.0, 64)]);
        assert!(publisher.publish_score(&score));

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
        settings.set_playing(true);
        assert!(publisher.publish(&settings));
        let mut score = Score::new();
        let track = score.track_mut(0).unwrap();
        track.set_enabled(true);
        track.set_notes(&[note(0.0, 4.0, 60)]);
        assert!(publisher.publish_score(&score));
        assert_eq!(play(&mut engine, 30, 256), 0.0);
    }

    #[test]
    fn several_instrument_tracks_sound_together() {
        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&playing_settings(3)));
        let mut score = Score::new();
        for (index, pitch) in [60, 64, 67].into_iter().enumerate() {
            let track = score.track_mut(index).unwrap();
            track.set_enabled(true);
            track.set_notes(&[note(0.0, 4.0, pitch)]);
        }
        assert_eq!(score.sounding_tracks(), 3);
        assert!(publisher.publish_score(&score));
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
        assert!(publisher.publish(&playing_settings(1)));
        let mut score = Score::new();
        let track = score.track_mut(0).unwrap();
        track.set_enabled(true);
        track.set_notes(&[note(0.0, 8.0, 60)]);
        assert!(publisher.publish_score(&score));
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
    fn a_published_audio_timeline_reaches_the_mixer() {
        use crate::engine::sample::{Interpolation, Sample};
        use crate::engine::timeline::{AudioRegion, AudioTimeline};

        let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
        assert!(publisher.publish(&playing_settings(1)));
        let sample = Sample::new(48_000, vec![[0.25, -0.5]; 512]).unwrap();
        let mut timeline = AudioTimeline::new();
        let media = timeline.add_sample(sample).unwrap();
        let mut region = AudioRegion::new(media, 0, 0.0, 1.0, 0.0, 24_000.0).unwrap();
        region.set_interpolation(Interpolation::Linear);
        timeline.add_region(region).unwrap();
        assert!(publisher.publish_audio(timeline));

        let mut output = [[0.0; 2]; 256];
        engine.render_block(&mut output, &[]);
        assert!((output[0][0] - 0.25).abs() < 0.000_001);
        assert!((output[0][1] + 0.5).abs() < 0.000_001);
        assert!(output.iter().flatten().any(|sample| *sample != 0.0));
    }

    #[test]
    fn playing_the_same_notes_twice_gives_the_same_audio() {
        let render = || {
            let (mut engine, mut publisher) = PlaybackEngine::new(RATE);
            assert!(publisher.publish(&playing_settings(2)));
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
