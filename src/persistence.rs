//! Versioned binary project documents and directory bundles.

use crate::project::{
    self, ArrangementPlacement, ClipId, MidiClip, MidiNote, Project, Scene, SceneId, Snapshot,
    Track, TrackId, TrackKind,
};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

const MAX_BYTES: usize = 256 * 1024 * 1024;
const VERSION: u32 = 2;
static SAVE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersistenceError {
    InvalidFormat,
    UnsupportedVersion,
    SizeLimit,
    Io,
}

impl From<std::io::Error> for PersistenceError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}

struct Encoder(Vec<u8>);
impl Encoder {
    fn bytes(&mut self, bytes: &[u8]) -> Result<(), PersistenceError> {
        if self.0.len().saturating_add(bytes.len()) > MAX_BYTES - 4 {
            return Err(PersistenceError::SizeLimit);
        }
        self.0.extend_from_slice(bytes);
        Ok(())
    }
    fn count(&mut self, count: usize) -> Result<(), PersistenceError> {
        self.bytes(
            &u32::try_from(count)
                .map_err(|_| PersistenceError::SizeLimit)?
                .to_le_bytes(),
        )
    }
    fn snapshot(&mut self, snapshot: &Snapshot) -> Result<(), PersistenceError> {
        self.bytes(&snapshot.tempo.to_le_bytes())?;
        self.bytes(&snapshot.numerator.to_le_bytes())?;
        self.bytes(&snapshot.denominator.to_le_bytes())?;
        self.bytes(&snapshot.sample_rate.to_le_bytes())?;
        self.count(snapshot.tracks.len())?;
        for track in &snapshot.tracks {
            self.bytes(&track.id.0.to_le_bytes())?;
            let flags =
                u8::from(track.muted) | (u8::from(track.solo) << 1) | (u8::from(track.armed) << 2);
            self.bytes(&[track.kind.code(), flags, track.color_index])?;
            self.bytes(&track.volume_db.to_le_bytes())?;
            self.bytes(&track.pan.to_le_bytes())?;
            self.count(track.name.len())?;
            self.bytes(track.name.as_bytes())?;
        }
        self.count(snapshot.scenes.len())?;
        for scene in &snapshot.scenes {
            self.bytes(&scene.id.0.to_le_bytes())?;
            self.count(scene.name.len())?;
            self.bytes(scene.name.as_bytes())?;
        }
        self.count(snapshot.clips.len())?;
        for clip in &snapshot.clips {
            self.bytes(&clip.id.0.to_le_bytes())?;
            self.bytes(&[clip.color_index])?;
            self.bytes(&clip.loop_start_beats.to_le_bytes())?;
            self.bytes(&clip.loop_length_beats.to_le_bytes())?;
            self.count(clip.name.len())?;
            self.bytes(clip.name.as_bytes())?;
            self.count(clip.notes.len())?;
            for note in &clip.notes {
                self.bytes(&[note.pitch, note.velocity])?;
                self.bytes(&note.start_beats.to_le_bytes())?;
                self.bytes(&note.length_beats.to_le_bytes())?;
            }
        }
        for track in &snapshot.tracks {
            self.count(track.session_slots.len())?;
            for slot in &track.session_slots {
                self.bytes(&slot.map_or(0, |id| id.0).to_le_bytes())?;
            }
            self.count(track.arrangement.len())?;
            for placement in &track.arrangement {
                self.bytes(&placement.clip.0.to_le_bytes())?;
                self.bytes(&placement.start_beats.to_le_bytes())?;
                self.bytes(&placement.length_beats.to_le_bytes())?;
            }
        }
        Ok(())
    }
}

struct Decoder<'a> {
    remaining: &'a [u8],
}
impl<'a> Decoder<'a> {
    fn bytes(&mut self, count: usize) -> Result<&'a [u8], PersistenceError> {
        let Some(result) = self.remaining.get(..count) else {
            return Err(PersistenceError::InvalidFormat);
        };
        self.remaining = &self.remaining[count..];
        Ok(result)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], PersistenceError> {
        self.bytes(N)?
            .try_into()
            .map_err(|_| PersistenceError::InvalidFormat)
    }
    fn count(&mut self) -> Result<usize, PersistenceError> {
        Ok(u32::from_le_bytes(self.array()?) as usize)
    }
    fn snapshot(&mut self, next_id: u64, version: u32) -> Result<Arc<Snapshot>, PersistenceError> {
        let tempo = f64::from_le_bytes(self.array()?);
        let numerator = u16::from_le_bytes(self.array()?);
        let denominator = u16::from_le_bytes(self.array()?);
        let sample_rate = u32::from_le_bytes(self.array()?);
        if !tempo.is_finite()
            || !(20.0..=999.0).contains(&tempo)
            || project::validate_signature(numerator, denominator).is_err()
            || project::validate_rate(sample_rate).is_err()
        {
            return Err(PersistenceError::InvalidFormat);
        }
        let count = self.count()?;
        if count > self.remaining.len() / 32 {
            return Err(PersistenceError::InvalidFormat);
        }
        let mut tracks = Vec::new();
        let mut ids = HashSet::new();
        for _ in 0..count {
            let id = u64::from_le_bytes(self.array()?);
            if id == 0 || id >= next_id || !ids.insert(id) {
                return Err(PersistenceError::InvalidFormat);
            }
            let [kind, flags, color_index] = self.array()?;
            let kind = TrackKind::from_code(kind).ok_or(PersistenceError::InvalidFormat)?;
            let volume_db = f64::from_le_bytes(self.array()?);
            let pan = f64::from_le_bytes(self.array()?);
            if flags & !7 != 0
                || color_index >= 16
                || project::validate_volume(volume_db).is_err()
                || project::validate_pan(pan).is_err()
            {
                return Err(PersistenceError::InvalidFormat);
            }
            let length = self.count()?;
            if length > 1024 {
                return Err(PersistenceError::InvalidFormat);
            }
            let name = std::str::from_utf8(self.bytes(length)?)
                .map_err(|_| PersistenceError::InvalidFormat)?;
            project::validate_name(name).map_err(|_| PersistenceError::InvalidFormat)?;
            tracks.push(Track {
                id: TrackId(id),
                name: name.into(),
                kind,
                volume_db,
                pan,
                muted: flags & 1 != 0,
                solo: flags & 2 != 0,
                armed: flags & 4 != 0,
                color_index,
                session_slots: Vec::new(),
                arrangement: Vec::new(),
            });
        }
        if version == 1 {
            return Ok(Arc::new(Snapshot {
                tempo,
                numerator,
                denominator,
                sample_rate,
                tracks,
                scenes: Vec::new(),
                clips: Vec::new(),
            }));
        }

        let scene_count = self.count()?;
        if scene_count > self.remaining.len() / 12 {
            return Err(PersistenceError::InvalidFormat);
        }
        let mut scenes = Vec::with_capacity(scene_count);
        for _ in 0..scene_count {
            let id = u64::from_le_bytes(self.array()?);
            if id == 0 || id >= next_id || !ids.insert(id) {
                return Err(PersistenceError::InvalidFormat);
            }
            let length = self.count()?;
            if length > 1024 {
                return Err(PersistenceError::InvalidFormat);
            }
            let name = std::str::from_utf8(self.bytes(length)?)
                .map_err(|_| PersistenceError::InvalidFormat)?;
            project::validate_name(name).map_err(|_| PersistenceError::InvalidFormat)?;
            scenes.push(Scene {
                id: SceneId(id),
                name: name.into(),
            });
        }

        let clip_count = self.count()?;
        if clip_count > self.remaining.len() / 33 {
            return Err(PersistenceError::InvalidFormat);
        }
        let mut clips = Vec::with_capacity(clip_count);
        for _ in 0..clip_count {
            let id = u64::from_le_bytes(self.array()?);
            if id == 0 || id >= next_id || !ids.insert(id) {
                return Err(PersistenceError::InvalidFormat);
            }
            let color_index = self.array::<1>()?[0];
            let loop_start_beats = f64::from_le_bytes(self.array()?);
            let loop_length_beats = f64::from_le_bytes(self.array()?);
            if color_index >= 16
                || !loop_start_beats.is_finite()
                || loop_start_beats < 0.0
                || !loop_length_beats.is_finite()
                || loop_length_beats <= 0.0
            {
                return Err(PersistenceError::InvalidFormat);
            }
            let length = self.count()?;
            if length > 1024 {
                return Err(PersistenceError::InvalidFormat);
            }
            let name = std::str::from_utf8(self.bytes(length)?)
                .map_err(|_| PersistenceError::InvalidFormat)?;
            project::validate_name(name).map_err(|_| PersistenceError::InvalidFormat)?;
            let note_count = self.count()?;
            if note_count > self.remaining.len() / 18 {
                return Err(PersistenceError::InvalidFormat);
            }
            let mut notes = Vec::with_capacity(note_count);
            for _ in 0..note_count {
                let [pitch, velocity] = self.array()?;
                let start_beats = f64::from_le_bytes(self.array()?);
                let length_beats = f64::from_le_bytes(self.array()?);
                if pitch > 127
                    || velocity == 0
                    || velocity > 127
                    || !start_beats.is_finite()
                    || start_beats < 0.0
                    || !length_beats.is_finite()
                    || length_beats <= 0.0
                {
                    return Err(PersistenceError::InvalidFormat);
                }
                notes.push(MidiNote {
                    pitch,
                    velocity,
                    start_beats,
                    length_beats,
                });
            }
            clips.push(MidiClip {
                id: ClipId(id),
                name: name.into(),
                color_index,
                loop_start_beats,
                loop_length_beats,
                notes,
            });
        }

        for track in &mut tracks {
            let slot_count = self.count()?;
            if slot_count != scenes.len() {
                return Err(PersistenceError::InvalidFormat);
            }
            for _ in 0..slot_count {
                let id = u64::from_le_bytes(self.array()?);
                if id != 0 && !clips.iter().any(|clip| clip.id.0 == id) {
                    return Err(PersistenceError::InvalidFormat);
                }
                track.session_slots.push((id != 0).then_some(ClipId(id)));
            }
            let placement_count = self.count()?;
            for _ in 0..placement_count {
                let clip = u64::from_le_bytes(self.array()?);
                let start_beats = f64::from_le_bytes(self.array()?);
                let length_beats = f64::from_le_bytes(self.array()?);
                if !clips.iter().any(|item| item.id.0 == clip)
                    || !start_beats.is_finite()
                    || start_beats < 0.0
                    || !length_beats.is_finite()
                    || length_beats <= 0.0
                {
                    return Err(PersistenceError::InvalidFormat);
                }
                track.arrangement.push(ArrangementPlacement {
                    clip: ClipId(clip),
                    start_beats,
                    length_beats,
                });
            }
        }
        if clips.iter().any(|clip| {
            !tracks.iter().any(|track| {
                track.session_slots.contains(&Some(clip.id))
                    || track
                        .arrangement
                        .iter()
                        .any(|placement| placement.clip == clip.id)
            })
        }) {
            return Err(PersistenceError::InvalidFormat);
        }
        Ok(Arc::new(Snapshot {
            tempo,
            numerator,
            denominator,
            sample_rate,
            tracks,
            scenes,
            clips,
        }))
    }
}

impl Project {
    pub fn to_bytes(&self) -> Result<Vec<u8>, PersistenceError> {
        let mut output = Encoder(Vec::new());
        output.bytes(b"NYLN")?;
        output.bytes(&VERSION.to_le_bytes())?;
        output.bytes(&self.next_id.to_le_bytes())?;
        output.count(self.undo.len())?;
        output.count(self.redo.len())?;
        output.snapshot(&self.current)?;
        for snapshot in &self.undo {
            output.snapshot(snapshot)?;
        }
        for snapshot in &self.redo {
            output.snapshot(snapshot)?;
        }
        let checksum = crc32(&output.0);
        output.0.extend_from_slice(&checksum.to_le_bytes());
        Ok(output.0)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PersistenceError> {
        if bytes.len() > MAX_BYTES {
            return Err(PersistenceError::SizeLimit);
        }
        if bytes.len() < 28 {
            return Err(PersistenceError::InvalidFormat);
        }
        let mut input = Decoder {
            remaining: &bytes[..bytes.len() - 4],
        };
        if input.bytes(4)? != b"NYLN" {
            return Err(PersistenceError::InvalidFormat);
        }
        let version = u32::from_le_bytes(input.array()?);
        if version == 0 || version > VERSION {
            return Err(PersistenceError::UnsupportedVersion);
        }
        let checksum = u32::from_le_bytes(
            bytes[bytes.len() - 4..]
                .try_into()
                .map_err(|_| PersistenceError::InvalidFormat)?,
        );
        if crc32(&bytes[..bytes.len() - 4]) != checksum {
            return Err(PersistenceError::InvalidFormat);
        }
        let next_id = u64::from_le_bytes(input.array()?);
        let undo_count = input.count()?;
        let redo_count = input.count()?;
        if next_id == 0
            || undo_count.saturating_add(redo_count).saturating_add(1) > input.remaining.len() / 20
        {
            return Err(PersistenceError::InvalidFormat);
        }
        let current = input.snapshot(next_id, version)?;
        let mut undo = Vec::new();
        let mut redo = Vec::new();
        for _ in 0..undo_count {
            undo.push(input.snapshot(next_id, version)?);
        }
        for _ in 0..redo_count {
            redo.push(input.snapshot(next_id, version)?);
        }
        if !input.remaining.is_empty() {
            return Err(PersistenceError::InvalidFormat);
        }
        Ok(Self {
            current,
            undo,
            redo,
            next_id,
        })
    }

    /// Write a complete temporary document, synchronize it, then replace the
    /// bundle document. A failed write leaves the previous document intact.
    pub fn save_bundle(&self, directory: &Path) -> Result<(), PersistenceError> {
        let bytes = self.to_bytes()?;
        fs::create_dir_all(directory)?;
        let sequence = SAVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = directory.join(format!(".project-{}-{sequence}.tmp", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, directory.join("project.nylon"))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    pub fn load_bundle(directory: &Path) -> Result<Self, PersistenceError> {
        let file = File::open(directory.join("project.nylon"))?;
        if file.metadata()?.len() > MAX_BYTES as u64 {
            return Err(PersistenceError::SizeLimit);
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
        Self::from_bytes(&bytes)
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut value = !0_u32;
    for byte in bytes {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = (value >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(value & 1));
        }
    }
    !value
}

#[cfg(test)]
mod tests {
    #[test]
    fn checksum_matches_the_standard_vector() {
        assert_eq!(super::crc32(b"123456789"), 0xcbf4_3926);
    }
}
