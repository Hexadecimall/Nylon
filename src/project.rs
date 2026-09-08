//! Immutable project snapshots and transactional edit history.

use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackId(pub(crate) u64);

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
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub(crate) tempo: f64,
    pub(crate) tracks: Vec<Track>,
    pub(crate) numerator: u16,
    pub(crate) denominator: u16,
    pub(crate) sample_rate: u32,
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
    CreateTrack { name: String, kind: TrackKind },
    DeleteTrack(TrackId),
    RenameTrack { id: TrackId, name: String },
    SetTrackVolume { id: TrackId, db: f64 },
    SetTrackPan { id: TrackId, pan: f64 },
    SetTrackMute { id: TrackId, enabled: bool },
    SetTrackSolo { id: TrackId, enabled: bool },
    SetTrackArm { id: TrackId, enabled: bool },
    SetTrackColor { id: TrackId, index: u8 },
    SetTimeSignature { numerator: u16, denominator: u16 },
    SetSampleRate(u32),
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
}

/// Control-thread state. Snapshots and history must never be destroyed in a
/// render callback because their storage is reference-counted and allocated.
pub struct Project {
    pub(crate) current: Arc<Snapshot>,
    pub(crate) undo: Vec<Arc<Snapshot>>,
    pub(crate) redo: Vec<Arc<Snapshot>>,
    pub(crate) next_id: u64,
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

impl Project {
    pub fn new() -> Self {
        Self {
            current: Arc::new(Snapshot {
                tempo: 120.0,
                tracks: Vec::new(),
                numerator: 4,
                denominator: 4,
                sample_rate: 48000,
            }),
            undo: Vec::new(),
            redo: Vec::new(),
            next_id: 1,
        }
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        Arc::clone(&self.current)
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
                        color_index: ((id.0 - 1) % 16) as u8,
                    });
                }
                Command::DeleteTrack(id) => {
                    let index = snapshot
                        .tracks
                        .iter()
                        .position(|track| track.id == *id)
                        .ok_or(ProjectError::MissingTrack)?;
                    snapshot.tracks.remove(index);
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
            }
        }
        self.undo
            .push(std::mem::replace(&mut self.current, Arc::new(snapshot)));
        self.redo.clear();
        self.next_id = next_id;
        Ok(())
    }

    pub fn undo(&mut self) -> bool {
        if let Some(snapshot) = self.undo.pop() {
            self.redo
                .push(std::mem::replace(&mut self.current, snapshot));
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(snapshot) = self.redo.pop() {
            self.undo
                .push(std::mem::replace(&mut self.current, snapshot));
            true
        } else {
            false
        }
    }
}

pub(crate) fn validate_name(name: &str) -> Result<(), ProjectError> {
    if name.trim().is_empty() || name.len() > 1024 || name.chars().any(char::is_control) {
        Err(ProjectError::InvalidName)
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
