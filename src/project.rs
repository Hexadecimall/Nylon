//! Immutable project snapshots and transactional edit history.

use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrackId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    Audio,
    Midi,
    Return,
    Master,
    Group,
    Cue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    id: TrackId,
    name: String,
    kind: TrackKind,
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    tempo: f64,
    tracks: Vec<Track>,
}

impl Snapshot {
    pub fn tempo(&self) -> f64 {
        self.tempo
    }
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }
}

#[derive(Clone, Debug)]
pub enum Command {
    SetTempo(f64),
    CreateTrack { name: String, kind: TrackKind },
    DeleteTrack(TrackId),
    RenameTrack { id: TrackId, name: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectError {
    InvalidTempo,
    InvalidName,
    MissingTrack,
    IdentifierExhausted,
}

/// Control-thread state. Snapshots and history must never be destroyed in a
/// render callback because their storage is reference-counted and allocated.
pub struct Project {
    current: Arc<Snapshot>,
    undo: Vec<Arc<Snapshot>>,
    redo: Vec<Arc<Snapshot>>,
    next_id: u64,
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
            }),
            undo: Vec::new(),
            redo: Vec::new(),
            next_id: 1,
        }
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        Arc::clone(&self.current)
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

fn validate_name(name: &str) -> Result<(), ProjectError> {
    if name.trim().is_empty() || name.chars().any(char::is_control) {
        Err(ProjectError::InvalidName)
    } else {
        Ok(())
    }
}
