//! Immutable project snapshots and transactional edit history.

use std::sync::Arc;

use crate::routing::{CompiledRouting, Edge, EdgeKind, RoutingError, RoutingGraph};

/// Largest number of routes stored in one project snapshot.
pub const MAX_PROJECT_ROUTES: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackId(pub(crate) u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneId(pub(crate) u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipId(pub(crate) u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteId(pub(crate) u64);

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub(crate) id: SceneId,
    pub(crate) name: String,
}

impl Scene {
    pub fn id(&self) -> SceneId {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MidiNote {
    pub pitch: u8,
    pub velocity: u8,
    pub start_beats: f64,
    pub length_beats: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MidiClip {
    pub(crate) id: ClipId,
    pub(crate) name: String,
    pub(crate) color_index: u8,
    pub(crate) loop_start_beats: f64,
    pub(crate) loop_length_beats: f64,
    pub(crate) notes: Vec<MidiNote>,
}

impl MidiClip {
    pub fn id(&self) -> ClipId {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn color_index(&self) -> u8 {
        self.color_index
    }
    pub fn loop_range(&self) -> (f64, f64) {
        (self.loop_start_beats, self.loop_length_beats)
    }
    pub fn notes(&self) -> &[MidiNote] {
        &self.notes
    }
}

/// Audio media referenced by a session slot and arrangement placements.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioClip {
    pub(crate) id: ClipId,
    pub(crate) name: String,
    pub(crate) color_index: u8,
    pub(crate) loop_start_beats: f64,
    pub(crate) loop_length_beats: f64,
    pub(crate) media_path: String,
    pub(crate) gain_db: f64,
    pub(crate) reverse: bool,
    pub(crate) warp: bool,
    pub(crate) source_tempo: f64,
}

impl AudioClip {
    pub fn id(&self) -> ClipId {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn color_index(&self) -> u8 {
        self.color_index
    }
    pub fn loop_range(&self) -> (f64, f64) {
        (self.loop_start_beats, self.loop_length_beats)
    }
    pub fn media_path(&self) -> &str {
        &self.media_path
    }
    pub fn gain_db(&self) -> f64 {
        self.gain_db
    }
    pub fn reversed(&self) -> bool {
        self.reverse
    }
    pub fn warped(&self) -> bool {
        self.warp
    }
    pub fn source_tempo(&self) -> f64 {
        self.source_tempo
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArrangementPlacement {
    pub(crate) clip: ClipId,
    pub(crate) start_beats: f64,
    pub(crate) length_beats: f64,
}

impl ArrangementPlacement {
    pub fn start_beats(&self) -> f64 {
        self.start_beats
    }
    pub fn length_beats(&self) -> f64 {
        self.length_beats
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    Audio,
    Midi,
    Return,
    Master,
    Group,
    Cue,
}

impl TrackKind {
    pub(crate) fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Audio),
            1 => Some(Self::Midi),
            2 => Some(Self::Return),
            3 => Some(Self::Master),
            4 => Some(Self::Group),
            5 => Some(Self::Cue),
            _ => None,
        }
    }
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Audio => 0,
            Self::Midi => 1,
            Self::Return => 2,
            Self::Master => 3,
            Self::Group => 4,
            Self::Cue => 5,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub(crate) id: TrackId,
    pub(crate) name: String,
    pub(crate) kind: TrackKind,
    pub(crate) volume_db: f64,
    pub(crate) pan: f64,
    pub(crate) muted: bool,
    pub(crate) solo: bool,
    pub(crate) armed: bool,
    pub(crate) color_index: u8,
    pub(crate) latency_frames: u32,
    pub(crate) session_slots: Vec<Option<ClipId>>,
    pub(crate) arrangement: Vec<ArrangementPlacement>,
}

impl Track {
    pub fn id(&self) -> TrackId {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn kind(&self) -> TrackKind {
        self.kind
    }
    pub fn volume_db(&self) -> f64 {
        self.volume_db
    }
    pub fn pan(&self) -> f64 {
        self.pan
    }
    pub fn muted(&self) -> bool {
        self.muted
    }
    pub fn solo(&self) -> bool {
        self.solo
    }
    pub fn armed(&self) -> bool {
        self.armed
    }
    pub fn color_index(&self) -> u8 {
        self.color_index
    }
    pub fn latency_frames(&self) -> u32 {
        self.latency_frames
    }
    pub fn arrangement(&self) -> &[ArrangementPlacement] {
        &self.arrangement
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Route {
    pub(crate) id: RouteId,
    pub(crate) source: TrackId,
    pub(crate) destination: TrackId,
    pub(crate) kind: EdgeKind,
    pub(crate) gain: f32,
}

impl Route {
    pub fn id(&self) -> RouteId {
        self.id
    }
    pub fn source(&self) -> TrackId {
        self.source
    }
    pub fn destination(&self) -> TrackId {
        self.destination
    }
    pub fn kind(&self) -> EdgeKind {
        self.kind
    }
    pub fn gain(&self) -> f32 {
        self.gain
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub(crate) tempo: f64,
    pub(crate) tracks: Vec<Track>,
    pub(crate) numerator: u16,
    pub(crate) denominator: u16,
    pub(crate) sample_rate: u32,
    pub(crate) scenes: Vec<Scene>,
    pub(crate) clips: Vec<MidiClip>,
    pub(crate) audio_clips: Vec<AudioClip>,
    pub(crate) routes: Vec<Route>,
}

impl Snapshot {
    pub fn tempo(&self) -> f64 {
        self.tempo
    }
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }
    pub fn time_signature(&self) -> (u16, u16) {
        (self.numerator, self.denominator)
    }
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    pub fn scenes(&self) -> &[Scene] {
        &self.scenes
    }
    pub fn clips(&self) -> &[MidiClip] {
        &self.clips
    }
    pub fn audio_clips(&self) -> &[AudioClip] {
        &self.audio_clips
    }
    pub fn routes(&self) -> &[Route] {
        &self.routes
    }
    pub fn compiled_routing(&self) -> Result<CompiledRouting, RoutingError> {
        let mut graph = RoutingGraph::new(self.tracks.len())?;
        for (index, track) in self.tracks.iter().enumerate() {
            graph.set_node_latency(index as u16, track.latency_frames)?;
        }
        for route in &self.routes {
            let source = self
                .tracks
                .iter()
                .position(|track| track.id == route.source)
                .ok_or(RoutingError::InvalidNode)?;
            let destination = self
                .tracks
                .iter()
                .position(|track| track.id == route.destination)
                .ok_or(RoutingError::InvalidNode)?;
            graph.add_edge(Edge {
                source: source as u16,
                destination: destination as u16,
                kind: route.kind,
                gain: route.gain,
            })?;
        }
        graph.compile()
    }
    pub fn clip_at(&self, track: usize, scene: usize) -> Option<&MidiClip> {
        let id = self.tracks.get(track)?.session_slots.get(scene)?.as_ref()?;
        self.clips.iter().find(|clip| clip.id == *id)
    }
    pub fn audio_clip_at(&self, track: usize, scene: usize) -> Option<&AudioClip> {
        let id = self.tracks.get(track)?.session_slots.get(scene)?.as_ref()?;
        self.audio_clips.iter().find(|clip| clip.id == *id)
    }

    fn track_mut(&mut self, id: TrackId) -> Result<&mut Track, ProjectError> {
        self.tracks
            .iter_mut()
            .find(|track| track.id == id)
            .ok_or(ProjectError::MissingTrack)
    }
}

#[derive(Clone, Debug)]
pub enum Command {
    SetTempo(f64),
    CreateTrack {
        name: String,
        kind: TrackKind,
    },
    DeleteTrack(TrackId),
    RenameTrack {
        id: TrackId,
        name: String,
    },
    SetTrackVolume {
        id: TrackId,
        db: f64,
    },
    SetTrackPan {
        id: TrackId,
        pan: f64,
    },
    SetTrackMute {
        id: TrackId,
        enabled: bool,
    },
    SetTrackSolo {
        id: TrackId,
        enabled: bool,
    },
    SetTrackArm {
        id: TrackId,
        enabled: bool,
    },
    SetTrackColor {
        id: TrackId,
        index: u8,
    },
    SetTrackLatency {
        id: TrackId,
        frames: u32,
    },
    CreateRoute {
        source: TrackId,
        destination: TrackId,
        kind: EdgeKind,
        gain: f32,
    },
    DeleteRoute(RouteId),
    SetTimeSignature {
        numerator: u16,
        denominator: u16,
    },
    SetSampleRate(u32),
    CreateScene {
        name: String,
    },
    DeleteScene(SceneId),
    RenameScene {
        id: SceneId,
        name: String,
    },
    CreateMidiClip {
        track: TrackId,
        scene: SceneId,
        name: String,
        length_beats: f64,
    },
    CreateAudioClip {
        track: TrackId,
        scene: SceneId,
        name: String,
        media_path: String,
        length_beats: f64,
        source_tempo: f64,
    },
    DeleteClip {
        track: TrackId,
        scene: SceneId,
    },
    SetClipName {
        id: ClipId,
        name: String,
    },
    SetClipColor {
        id: ClipId,
        index: u8,
    },
    SetClipLoop {
        id: ClipId,
        start_beats: f64,
        length_beats: f64,
    },
    SetAudioClipGain {
        id: ClipId,
        db: f64,
    },
    SetAudioClipReverse {
        id: ClipId,
        enabled: bool,
    },
    SetAudioClipWarp {
        id: ClipId,
        enabled: bool,
        source_tempo: f64,
    },
    AddNote {
        id: ClipId,
        note: MidiNote,
    },
    RemoveNote {
        id: ClipId,
        index: usize,
    },
    MoveNote {
        id: ClipId,
        index: usize,
        note: MidiNote,
    },
    PlaceClip {
        track: TrackId,
        clip: ClipId,
        start_beats: f64,
        length_beats: f64,
    },
    RemovePlacement {
        track: TrackId,
        index: usize,
    },
    SetPlacementRange {
        track: TrackId,
        index: usize,
        start_beats: f64,
        length_beats: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectError {
    InvalidTempo,
    InvalidName,
    MissingTrack,
    IdentifierExhausted,
    InvalidVolume,
    InvalidPan,
    InvalidColor,
    InvalidTimeSignature,
    InvalidSampleRate,
    MissingScene,
    MissingClip,
    OccupiedClipSlot,
    InvalidClipLength,
    InvalidMediaPath,
    InvalidNote,
    MissingNote,
    MissingPlacement,
    TrackCapacity,
    InvalidRouting,
    MissingRoute,
}

/// Control-thread state. Snapshots and history must never be destroyed in a
/// render callback because their storage is reference-counted and allocated.
pub struct Project {
    pub(crate) current: Arc<Snapshot>,
    pub(crate) undo: Vec<Arc<Snapshot>>,
    pub(crate) redo: Vec<Arc<Snapshot>>,
    pub(crate) next_id: u64,
    pub(crate) bundle_directory: Option<std::path::PathBuf>,
    pub(crate) revision: u64,
    pub(crate) saved_revision: u64,
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

impl Project {
    pub fn new() -> Self {
        let scenes = (1..=8)
            .map(|number| Scene {
                id: SceneId(number),
                name: format!("Scene {number}"),
            })
            .collect();
        Self {
            current: Arc::new(Snapshot {
                tempo: 120.0,
                tracks: Vec::new(),
                numerator: 4,
                denominator: 4,
                sample_rate: 48000,
                scenes,
                clips: Vec::new(),
                audio_clips: Vec::new(),
                routes: Vec::new(),
            }),
            undo: Vec::new(),
            redo: Vec::new(),
            next_id: 9,
            bundle_directory: None,
            revision: 0,
            saved_revision: 0,
        }
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        Arc::clone(&self.current)
    }
    pub fn bundle_directory(&self) -> Option<&std::path::Path> {
        self.bundle_directory.as_deref()
    }
    pub fn is_modified(&self) -> bool {
        self.revision != self.saved_revision
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Apply a group atomically and publish one undo step.
    pub fn apply(&mut self, commands: &[Command]) -> Result<(), ProjectError> {
        if commands.is_empty() {
            return Ok(());
        }
        let mut snapshot = (*self.current).clone();
        let mut next_id = self.next_id;
        for command in commands {
            match command {
                Command::SetTempo(tempo) => {
                    if !tempo.is_finite() || !(20.0..=999.0).contains(tempo) {
                        return Err(ProjectError::InvalidTempo);
                    }
                    snapshot.tempo = *tempo;
                }
                Command::CreateTrack { name, kind } => {
                    validate_name(name)?;
                    if snapshot.tracks.len() == crate::mixer::MAX_TRACKS {
                        return Err(ProjectError::TrackCapacity);
                    }
                    let id = TrackId(next_id);
                    next_id = next_id
                        .checked_add(1)
                        .ok_or(ProjectError::IdentifierExhausted)?;
                    snapshot.tracks.push(Track {
                        id,
                        name: name.clone(),
                        kind: *kind,
                        volume_db: 0.0,
                        pan: 0.0,
                        muted: false,
                        solo: false,
                        armed: false,
                        color_index: (snapshot.tracks.len() % 16) as u8,
                        latency_frames: 0,
                        session_slots: vec![None; snapshot.scenes.len()],
                        arrangement: Vec::new(),
                    });
                }
                Command::DeleteTrack(id) => {
                    let index = snapshot
                        .tracks
                        .iter()
                        .position(|track| track.id == *id)
                        .ok_or(ProjectError::MissingTrack)?;
                    snapshot.tracks.remove(index);
                    snapshot
                        .routes
                        .retain(|route| route.source != *id && route.destination != *id);
                    remove_unreferenced_clips(&mut snapshot);
                }
                Command::RenameTrack { id, name } => {
                    validate_name(name)?;
                    let track = snapshot
                        .tracks
                        .iter_mut()
                        .find(|track| track.id == *id)
                        .ok_or(ProjectError::MissingTrack)?;
                    track.name = name.clone();
                }
                Command::SetTrackVolume { id, db } => {
                    validate_volume(*db)?;
                    snapshot.track_mut(*id)?.volume_db = *db;
                }
                Command::SetTrackPan { id, pan } => {
                    validate_pan(*pan)?;
                    snapshot.track_mut(*id)?.pan = *pan;
                }
                Command::SetTrackMute { id, enabled } => snapshot.track_mut(*id)?.muted = *enabled,
                Command::SetTrackSolo { id, enabled } => snapshot.track_mut(*id)?.solo = *enabled,
                Command::SetTrackArm { id, enabled } => snapshot.track_mut(*id)?.armed = *enabled,
                Command::SetTrackColor { id, index } => {
                    if *index >= 16 {
                        return Err(ProjectError::InvalidColor);
                    }
                    snapshot.track_mut(*id)?.color_index = *index;
                }
                Command::SetTrackLatency { id, frames } => {
                    snapshot.track_mut(*id)?.latency_frames = *frames;
                    snapshot
                        .compiled_routing()
                        .map_err(|_| ProjectError::InvalidRouting)?;
                }
                Command::CreateRoute {
                    source,
                    destination,
                    kind,
                    gain,
                } => {
                    if snapshot.routes.len() == MAX_PROJECT_ROUTES
                        || !gain.is_finite()
                        || !(0.0..=4.0).contains(gain)
                        || snapshot.routes.iter().any(|route| {
                            route.source == *source
                                && route.destination == *destination
                                && route.kind == *kind
                        })
                    {
                        return Err(ProjectError::InvalidRouting);
                    }
                    snapshot.track_mut(*source)?;
                    snapshot.track_mut(*destination)?;
                    let id = RouteId(next_id);
                    next_id = next_identifier(next_id)?;
                    snapshot.routes.push(Route {
                        id,
                        source: *source,
                        destination: *destination,
                        kind: *kind,
                        gain: *gain,
                    });
                    snapshot
                        .compiled_routing()
                        .map_err(|_| ProjectError::InvalidRouting)?;
                }
                Command::DeleteRoute(id) => {
                    let index = snapshot
                        .routes
                        .iter()
                        .position(|route| route.id == *id)
                        .ok_or(ProjectError::MissingRoute)?;
                    snapshot.routes.remove(index);
                }
                Command::SetTimeSignature {
                    numerator,
                    denominator,
                } => {
                    validate_signature(*numerator, *denominator)?;
                    snapshot.numerator = *numerator;
                    snapshot.denominator = *denominator;
                }
                Command::SetSampleRate(rate) => {
                    validate_rate(*rate)?;
                    snapshot.sample_rate = *rate;
                }
                Command::CreateScene { name } => {
                    validate_name(name)?;
                    let id = SceneId(next_id);
                    next_id = next_identifier(next_id)?;
                    snapshot.scenes.push(Scene {
                        id,
                        name: name.clone(),
                    });
                    for track in &mut snapshot.tracks {
                        track.session_slots.push(None);
                    }
                }
                Command::DeleteScene(id) => {
                    let index = scene_index(&snapshot, *id)?;
                    snapshot.scenes.remove(index);
                    for track in &mut snapshot.tracks {
                        track.session_slots.remove(index);
                    }
                    remove_unreferenced_clips(&mut snapshot);
                }
                Command::RenameScene { id, name } => {
                    validate_name(name)?;
                    let index = scene_index(&snapshot, *id)?;
                    snapshot.scenes[index].name = name.clone();
                }
                Command::CreateMidiClip {
                    track,
                    scene,
                    name,
                    length_beats,
                } => {
                    validate_name(name)?;
                    validate_positive_beats(*length_beats)?;
                    let scene = scene_index(&snapshot, *scene)?;
                    let id = ClipId(next_id);
                    next_id = next_identifier(next_id)?;
                    let color_index = {
                        let track = snapshot.track_mut(*track)?;
                        if track.kind != TrackKind::Midi || track.session_slots[scene].is_some() {
                            return Err(ProjectError::OccupiedClipSlot);
                        }
                        track.session_slots[scene] = Some(id);
                        track.color_index
                    };
                    snapshot.clips.push(MidiClip {
                        id,
                        name: name.clone(),
                        color_index,
                        loop_start_beats: 0.0,
                        loop_length_beats: *length_beats,
                        notes: Vec::new(),
                    });
                }
                Command::CreateAudioClip {
                    track,
                    scene,
                    name,
                    media_path,
                    length_beats,
                    source_tempo,
                } => {
                    validate_name(name)?;
                    validate_media_path(media_path)?;
                    validate_positive_beats(*length_beats)?;
                    validate_tempo(*source_tempo)?;
                    let scene = scene_index(&snapshot, *scene)?;
                    let id = ClipId(next_id);
                    next_id = next_identifier(next_id)?;
                    let color_index = {
                        let track = snapshot.track_mut(*track)?;
                        if track.kind != TrackKind::Audio || track.session_slots[scene].is_some() {
                            return Err(ProjectError::OccupiedClipSlot);
                        }
                        track.session_slots[scene] = Some(id);
                        track.color_index
                    };
                    snapshot.audio_clips.push(AudioClip {
                        id,
                        name: name.clone(),
                        color_index,
                        loop_start_beats: 0.0,
                        loop_length_beats: *length_beats,
                        media_path: media_path.clone(),
                        gain_db: 0.0,
                        reverse: false,
                        warp: false,
                        source_tempo: *source_tempo,
                    });
                }
                Command::DeleteClip { track, scene } => {
                    let scene = scene_index(&snapshot, *scene)?;
                    let track = snapshot.track_mut(*track)?;
                    if track.session_slots[scene].take().is_none() {
                        return Err(ProjectError::MissingClip);
                    }
                    remove_unreferenced_clips(&mut snapshot);
                }
                Command::SetClipName { id, name } => {
                    validate_name(name)?;
                    set_clip_name(&mut snapshot, *id, name.clone())?;
                }
                Command::SetClipColor { id, index } => {
                    if *index >= 16 {
                        return Err(ProjectError::InvalidColor);
                    }
                    set_clip_color(&mut snapshot, *id, *index)?;
                }
                Command::SetClipLoop {
                    id,
                    start_beats,
                    length_beats,
                } => {
                    validate_nonnegative_beats(*start_beats)?;
                    validate_positive_beats(*length_beats)?;
                    set_clip_loop(&mut snapshot, *id, *start_beats, *length_beats)?;
                }
                Command::SetAudioClipGain { id, db } => {
                    validate_volume(*db)?;
                    audio_clip_mut(&mut snapshot, *id)?.gain_db = *db;
                }
                Command::SetAudioClipReverse { id, enabled } => {
                    audio_clip_mut(&mut snapshot, *id)?.reverse = *enabled;
                }
                Command::SetAudioClipWarp {
                    id,
                    enabled,
                    source_tempo,
                } => {
                    validate_tempo(*source_tempo)?;
                    let clip = audio_clip_mut(&mut snapshot, *id)?;
                    clip.warp = *enabled;
                    clip.source_tempo = *source_tempo;
                }
                Command::AddNote { id, note } => {
                    validate_note(*note)?;
                    let clip = midi_clip_mut(&mut snapshot, *id)?;
                    clip.notes.push(*note);
                    clip.notes.sort_by(|a, b| {
                        a.start_beats
                            .total_cmp(&b.start_beats)
                            .then(a.pitch.cmp(&b.pitch))
                    });
                }
                Command::RemoveNote { id, index } => {
                    let clip = midi_clip_mut(&mut snapshot, *id)?;
                    if *index >= clip.notes.len() {
                        return Err(ProjectError::MissingNote);
                    }
                    clip.notes.remove(*index);
                }
                Command::MoveNote { id, index, note } => {
                    validate_note(*note)?;
                    let clip = midi_clip_mut(&mut snapshot, *id)?;
                    if *index >= clip.notes.len() {
                        return Err(ProjectError::MissingNote);
                    }
                    clip.notes[*index] = *note;
                    clip.notes.sort_by(|a, b| {
                        a.start_beats
                            .total_cmp(&b.start_beats)
                            .then(a.pitch.cmp(&b.pitch))
                    });
                }
                Command::PlaceClip {
                    track,
                    clip,
                    start_beats,
                    length_beats,
                } => {
                    validate_nonnegative_beats(*start_beats)?;
                    validate_positive_beats(*length_beats)?;
                    let kind = snapshot.track_mut(*track)?.kind;
                    validate_clip_track(&snapshot, *clip, kind)?;
                    snapshot
                        .track_mut(*track)?
                        .arrangement
                        .push(ArrangementPlacement {
                            clip: *clip,
                            start_beats: *start_beats,
                            length_beats: *length_beats,
                        });
                }
                Command::RemovePlacement { track, index } => {
                    let track = snapshot.track_mut(*track)?;
                    if *index >= track.arrangement.len() {
                        return Err(ProjectError::MissingPlacement);
                    }
                    track.arrangement.remove(*index);
                    remove_unreferenced_clips(&mut snapshot);
                }
                Command::SetPlacementRange {
                    track,
                    index,
                    start_beats,
                    length_beats,
                } => {
                    validate_nonnegative_beats(*start_beats)?;
                    validate_positive_beats(*length_beats)?;
                    let track = snapshot.track_mut(*track)?;
                    let placement = track
                        .arrangement
                        .get_mut(*index)
                        .ok_or(ProjectError::MissingPlacement)?;
                    placement.start_beats = *start_beats;
                    placement.length_beats = *length_beats;
                }
            }
        }
        self.undo
            .push(std::mem::replace(&mut self.current, Arc::new(snapshot)));
        self.redo.clear();
        self.next_id = next_id;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    pub fn undo(&mut self) -> bool {
        if let Some(snapshot) = self.undo.pop() {
            self.redo
                .push(std::mem::replace(&mut self.current, snapshot));
            self.revision = self.revision.wrapping_add(1);
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(snapshot) = self.redo.pop() {
            self.undo
                .push(std::mem::replace(&mut self.current, snapshot));
            self.revision = self.revision.wrapping_add(1);
            true
        } else {
            false
        }
    }
}

fn validate_tempo(tempo: f64) -> Result<(), ProjectError> {
    if tempo.is_finite() && (20.0..=999.0).contains(&tempo) {
        Ok(())
    } else {
        Err(ProjectError::InvalidTempo)
    }
}

pub(crate) fn validate_name(name: &str) -> Result<(), ProjectError> {
    if name.trim().is_empty() || name.len() > 1024 || name.chars().any(char::is_control) {
        Err(ProjectError::InvalidName)
    } else {
        Ok(())
    }
}

pub(crate) fn validate_media_path(path: &str) -> Result<(), ProjectError> {
    if path.is_empty()
        || path.len() > 4096
        || path.starts_with('/')
        || path.starts_with("~/")
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        Err(ProjectError::InvalidMediaPath)
    } else {
        Ok(())
    }
}

pub(crate) fn validate_volume(db: f64) -> Result<(), ProjectError> {
    if db == f64::NEG_INFINITY || (db.is_finite() && (-120.0..=6.0).contains(&db)) {
        Ok(())
    } else {
        Err(ProjectError::InvalidVolume)
    }
}

pub(crate) fn validate_pan(pan: f64) -> Result<(), ProjectError> {
    if pan.is_finite() && (-1.0..=1.0).contains(&pan) {
        Ok(())
    } else {
        Err(ProjectError::InvalidPan)
    }
}

pub(crate) fn validate_signature(numerator: u16, denominator: u16) -> Result<(), ProjectError> {
    if (1..=64).contains(&numerator) && denominator.is_power_of_two() && denominator <= 64 {
        Ok(())
    } else {
        Err(ProjectError::InvalidTimeSignature)
    }
}

pub(crate) fn validate_rate(rate: u32) -> Result<(), ProjectError> {
    if (44100..=192000).contains(&rate) {
        Ok(())
    } else {
        Err(ProjectError::InvalidSampleRate)
    }
}

fn next_identifier(value: u64) -> Result<u64, ProjectError> {
    value
        .checked_add(1)
        .ok_or(ProjectError::IdentifierExhausted)
}

fn scene_index(snapshot: &Snapshot, id: SceneId) -> Result<usize, ProjectError> {
    snapshot
        .scenes
        .iter()
        .position(|scene| scene.id == id)
        .ok_or(ProjectError::MissingScene)
}

fn midi_clip_mut(snapshot: &mut Snapshot, id: ClipId) -> Result<&mut MidiClip, ProjectError> {
    snapshot
        .clips
        .iter_mut()
        .find(|clip| clip.id == id)
        .ok_or(ProjectError::MissingClip)
}

fn audio_clip_mut(snapshot: &mut Snapshot, id: ClipId) -> Result<&mut AudioClip, ProjectError> {
    snapshot
        .audio_clips
        .iter_mut()
        .find(|clip| clip.id == id)
        .ok_or(ProjectError::MissingClip)
}

fn validate_clip_track(
    snapshot: &Snapshot,
    id: ClipId,
    track: TrackKind,
) -> Result<(), ProjectError> {
    if snapshot.clips.iter().any(|clip| clip.id == id) {
        return (track == TrackKind::Midi)
            .then_some(())
            .ok_or(ProjectError::MissingClip);
    }
    if snapshot.audio_clips.iter().any(|clip| clip.id == id) {
        return (track == TrackKind::Audio)
            .then_some(())
            .ok_or(ProjectError::MissingClip);
    }
    Err(ProjectError::MissingClip)
}

fn set_clip_name(snapshot: &mut Snapshot, id: ClipId, name: String) -> Result<(), ProjectError> {
    if let Some(clip) = snapshot.clips.iter_mut().find(|clip| clip.id == id) {
        clip.name = name;
        return Ok(());
    }
    audio_clip_mut(snapshot, id)?.name = name;
    Ok(())
}

fn set_clip_color(snapshot: &mut Snapshot, id: ClipId, color: u8) -> Result<(), ProjectError> {
    if let Some(clip) = snapshot.clips.iter_mut().find(|clip| clip.id == id) {
        clip.color_index = color;
        return Ok(());
    }
    audio_clip_mut(snapshot, id)?.color_index = color;
    Ok(())
}

fn set_clip_loop(
    snapshot: &mut Snapshot,
    id: ClipId,
    start: f64,
    length: f64,
) -> Result<(), ProjectError> {
    if let Some(clip) = snapshot.clips.iter_mut().find(|clip| clip.id == id) {
        clip.loop_start_beats = start;
        clip.loop_length_beats = length;
        return Ok(());
    }
    let clip = audio_clip_mut(snapshot, id)?;
    clip.loop_start_beats = start;
    clip.loop_length_beats = length;
    Ok(())
}

fn validate_nonnegative_beats(value: f64) -> Result<(), ProjectError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(ProjectError::InvalidClipLength)
    }
}

fn validate_positive_beats(value: f64) -> Result<(), ProjectError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(ProjectError::InvalidClipLength)
    }
}

fn validate_note(note: MidiNote) -> Result<(), ProjectError> {
    if note.pitch > 127 || note.velocity == 0 || note.velocity > 127 {
        return Err(ProjectError::InvalidNote);
    }
    validate_nonnegative_beats(note.start_beats).map_err(|_| ProjectError::InvalidNote)?;
    validate_positive_beats(note.length_beats).map_err(|_| ProjectError::InvalidNote)
}

fn remove_unreferenced_clips(snapshot: &mut Snapshot) {
    snapshot.clips.retain(|clip| {
        snapshot.tracks.iter().any(|track| {
            track.session_slots.contains(&Some(clip.id))
                || track
                    .arrangement
                    .iter()
                    .any(|placement| placement.clip == clip.id)
        })
    });
    snapshot.audio_clips.retain(|clip| {
        snapshot.tracks.iter().any(|track| {
            track.session_slots.contains(&Some(clip.id))
                || track
                    .arrangement
                    .iter()
                    .any(|placement| placement.clip == clip.id)
        })
    });
}
