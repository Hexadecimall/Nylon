//! Versioned binary project documents and directory bundles.

use crate::dsp::biquad::Kind as FilterKind;
use crate::dsp::compressor::Parameters as CompressorParameters;
use crate::dsp::limiter::Parameters as LimiterParameters;
use crate::dsp::saturator::{
    Curve as SaturatorCurve, Oversampling as SaturatorOversampling,
    Parameters as SaturatorParameters,
};
use crate::engine::device::{DeviceConfig, DeviceKind, MAX_DEVICES};
use crate::project::{
    self, ArrangementPlacement, AudioClip, ClipId, Device, DeviceId, MidiClip, MidiNote, Project,
    Route, RouteId, Scene, SceneId, Snapshot, Track, TrackId, TrackKind,
};
use crate::routing::EdgeKind;
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

const MAX_BYTES: usize = 256 * 1024 * 1024;
const VERSION: u32 = 7;
const DOCUMENT_NAME: &str = "project.nylon";
const RECOVERY_NAME: &str = ".autosave.nylon";
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
            self.bytes(&track.latency_frames.to_le_bytes())?;
            self.count(track.name.len())?;
            self.bytes(track.name.as_bytes())?;
            self.count(track.devices.len())?;
            for device in &track.devices {
                self.bytes(&device.id.0.to_le_bytes())?;
                self.bytes(&[u8::from(device.config.enabled)])?;
                match device.config.kind {
                    DeviceKind::Utility {
                        gain_db,
                        width,
                        balance,
                    } => {
                        self.bytes(&[0])?;
                        for value in [gain_db, width, balance] {
                            self.bytes(&value.to_le_bytes())?;
                        }
                    }
                    DeviceKind::Equalizer {
                        kind,
                        frequency,
                        q,
                        gain_db,
                    } => {
                        self.bytes(&[1, filter_code(kind)])?;
                        for value in [frequency, q, gain_db] {
                            self.bytes(&value.to_le_bytes())?;
                        }
                    }
                    DeviceKind::Compressor {
                        parameters,
                        external_sidechain,
                    } => {
                        self.bytes(&[2, u8::from(external_sidechain)])?;
                        for value in [
                            parameters.threshold_db,
                            parameters.ratio,
                            parameters.knee_db,
                            parameters.attack_seconds,
                            parameters.release_seconds,
                            parameters.makeup_db,
                        ] {
                            self.bytes(&value.to_le_bytes())?;
                        }
                    }
                    DeviceKind::StereoDelay {
                        delay_seconds,
                        feedback,
                        mix,
                    } => {
                        self.bytes(&[3])?;
                        for value in [delay_seconds, feedback, mix] {
                            self.bytes(&value.to_le_bytes())?;
                        }
                    }
                    DeviceKind::Limiter { parameters } => {
                        self.bytes(&[4])?;
                        for value in [
                            parameters.ceiling_db,
                            parameters.release_seconds,
                            parameters.lookahead_seconds,
                        ] {
                            self.bytes(&value.to_le_bytes())?;
                        }
                    }
                    DeviceKind::Saturator { parameters } => {
                        self.bytes(&[
                            5,
                            match parameters.curve {
                                SaturatorCurve::SoftClip => 0,
                                SaturatorCurve::Tanh => 1,
                                SaturatorCurve::HardClip => 2,
                                SaturatorCurve::Diode => 3,
                            },
                            match parameters.oversampling {
                                SaturatorOversampling::One => 0,
                                SaturatorOversampling::Two => 1,
                                SaturatorOversampling::Four => 2,
                            },
                            u8::from(parameters.dc_filter),
                        ])?;
                        for value in [parameters.drive_db, parameters.output_db, parameters.mix] {
                            self.bytes(&value.to_le_bytes())?;
                        }
                    }
                }
            }
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
        self.count(snapshot.audio_clips.len())?;
        for clip in &snapshot.audio_clips {
            self.bytes(&clip.id.0.to_le_bytes())?;
            let flags = u8::from(clip.reverse) | (u8::from(clip.warp) << 1);
            self.bytes(&[clip.color_index, flags])?;
            self.bytes(&clip.loop_start_beats.to_le_bytes())?;
            self.bytes(&clip.loop_length_beats.to_le_bytes())?;
            self.bytes(&clip.gain_db.to_le_bytes())?;
            self.bytes(&clip.source_tempo.to_le_bytes())?;
            self.count(clip.name.len())?;
            self.bytes(clip.name.as_bytes())?;
            self.count(clip.media_path.len())?;
            self.bytes(clip.media_path.as_bytes())?;
        }
        self.count(snapshot.routes.len())?;
        for route in &snapshot.routes {
            self.bytes(&route.id.0.to_le_bytes())?;
            self.bytes(&route.source.0.to_le_bytes())?;
            self.bytes(&route.destination.0.to_le_bytes())?;
            self.bytes(&[match route.kind {
                EdgeKind::Main => 0,
                EdgeKind::SendPreFader => 1,
                EdgeKind::SendPostFader => 2,
                EdgeKind::Sidechain => 3,
            }])?;
            self.bytes(&route.gain.to_le_bytes())?;
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
            let latency_frames = if version >= 4 {
                u32::from_le_bytes(self.array()?)
            } else {
                0
            };
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
            let mut devices = Vec::new();
            if version >= 5 {
                let device_count = self.count()?;
                if device_count > MAX_DEVICES {
                    return Err(PersistenceError::InvalidFormat);
                }
                devices.reserve(device_count);
                for _ in 0..device_count {
                    let device_id = u64::from_le_bytes(self.array()?);
                    let enabled = match self.array::<1>()?[0] {
                        0 => false,
                        1 => true,
                        _ => return Err(PersistenceError::InvalidFormat),
                    };
                    if device_id == 0 || device_id >= next_id || !ids.insert(device_id) {
                        return Err(PersistenceError::InvalidFormat);
                    }
                    let kind = match self.array::<1>()?[0] {
                        0 => DeviceKind::Utility {
                            gain_db: f32::from_le_bytes(self.array()?),
                            width: f32::from_le_bytes(self.array()?),
                            balance: f32::from_le_bytes(self.array()?),
                        },
                        1 => DeviceKind::Equalizer {
                            kind: filter_from_code(self.array::<1>()?[0])
                                .ok_or(PersistenceError::InvalidFormat)?,
                            frequency: f32::from_le_bytes(self.array()?),
                            q: f32::from_le_bytes(self.array()?),
                            gain_db: f32::from_le_bytes(self.array()?),
                        },
                        2 => {
                            let external_sidechain = match self.array::<1>()?[0] {
                                0 => false,
                                1 => true,
                                _ => return Err(PersistenceError::InvalidFormat),
                            };
                            DeviceKind::Compressor {
                                parameters: CompressorParameters {
                                    threshold_db: f32::from_le_bytes(self.array()?),
                                    ratio: f32::from_le_bytes(self.array()?),
                                    knee_db: f32::from_le_bytes(self.array()?),
                                    attack_seconds: f32::from_le_bytes(self.array()?),
                                    release_seconds: f32::from_le_bytes(self.array()?),
                                    makeup_db: f32::from_le_bytes(self.array()?),
                                },
                                external_sidechain,
                            }
                        }
                        3 => DeviceKind::StereoDelay {
                            delay_seconds: f32::from_le_bytes(self.array()?),
                            feedback: f32::from_le_bytes(self.array()?),
                            mix: f32::from_le_bytes(self.array()?),
                        },
                        4 => DeviceKind::Limiter {
                            parameters: LimiterParameters {
                                ceiling_db: f32::from_le_bytes(self.array()?),
                                release_seconds: f32::from_le_bytes(self.array()?),
                                lookahead_seconds: f32::from_le_bytes(self.array()?),
                            },
                        },
                        5 if version >= 7 => {
                            let curve = match self.array::<1>()?[0] {
                                0 => SaturatorCurve::SoftClip,
                                1 => SaturatorCurve::Tanh,
                                2 => SaturatorCurve::HardClip,
                                3 => SaturatorCurve::Diode,
                                _ => return Err(PersistenceError::InvalidFormat),
                            };
                            let oversampling = match self.array::<1>()?[0] {
                                0 => SaturatorOversampling::One,
                                1 => SaturatorOversampling::Two,
                                2 => SaturatorOversampling::Four,
                                _ => return Err(PersistenceError::InvalidFormat),
                            };
                            let dc_filter = match self.array::<1>()?[0] {
                                0 => false,
                                1 => true,
                                _ => return Err(PersistenceError::InvalidFormat),
                            };
                            DeviceKind::Saturator {
                                parameters: SaturatorParameters {
                                    drive_db: f32::from_le_bytes(self.array()?),
                                    output_db: f32::from_le_bytes(self.array()?),
                                    mix: f32::from_le_bytes(self.array()?),
                                    curve,
                                    oversampling,
                                    dc_filter,
                                },
                            }
                        }
                        _ => return Err(PersistenceError::InvalidFormat),
                    };
                    let config = DeviceConfig { enabled, kind };
                    config
                        .validate(sample_rate as f32)
                        .map_err(|_| PersistenceError::InvalidFormat)?;
                    devices.push(Device {
                        id: DeviceId(device_id),
                        config,
                    });
                }
            }
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
                latency_frames,
                devices,
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
                audio_clips: Vec::new(),
                routes: Vec::new(),
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

        let mut audio_clips = Vec::new();
        if version >= 3 {
            let audio_count = self.count()?;
            if audio_count > self.remaining.len() / 54 {
                return Err(PersistenceError::InvalidFormat);
            }
            audio_clips.reserve(audio_count);
            for _ in 0..audio_count {
                let id = u64::from_le_bytes(self.array()?);
                if id == 0 || id >= next_id || !ids.insert(id) {
                    return Err(PersistenceError::InvalidFormat);
                }
                let [color_index, flags] = self.array()?;
                let loop_start_beats = f64::from_le_bytes(self.array()?);
                let loop_length_beats = f64::from_le_bytes(self.array()?);
                let gain_db = f64::from_le_bytes(self.array()?);
                let source_tempo = f64::from_le_bytes(self.array()?);
                if color_index >= 16
                    || flags & !3 != 0
                    || !loop_start_beats.is_finite()
                    || loop_start_beats < 0.0
                    || !loop_length_beats.is_finite()
                    || loop_length_beats <= 0.0
                    || project::validate_volume(gain_db).is_err()
                    || !source_tempo.is_finite()
                    || !(20.0..=999.0).contains(&source_tempo)
                {
                    return Err(PersistenceError::InvalidFormat);
                }
                let name_length = self.count()?;
                if name_length > 1024 {
                    return Err(PersistenceError::InvalidFormat);
                }
                let name = std::str::from_utf8(self.bytes(name_length)?)
                    .map_err(|_| PersistenceError::InvalidFormat)?;
                project::validate_name(name).map_err(|_| PersistenceError::InvalidFormat)?;
                let path_length = self.count()?;
                if path_length > 4096 {
                    return Err(PersistenceError::InvalidFormat);
                }
                let media_path = std::str::from_utf8(self.bytes(path_length)?)
                    .map_err(|_| PersistenceError::InvalidFormat)?;
                project::validate_media_path(media_path)
                    .map_err(|_| PersistenceError::InvalidFormat)?;
                audio_clips.push(AudioClip {
                    id: ClipId(id),
                    name: name.into(),
                    color_index,
                    loop_start_beats,
                    loop_length_beats,
                    media_path: media_path.into(),
                    gain_db,
                    reverse: flags & 1 != 0,
                    warp: flags & 2 != 0,
                    source_tempo,
                });
            }
        }

        let mut routes = Vec::new();
        if version >= 4 {
            let route_count = self.count()?;
            if route_count > crate::project::MAX_PROJECT_ROUTES {
                return Err(PersistenceError::InvalidFormat);
            }
            for _ in 0..route_count {
                let id = u64::from_le_bytes(self.array()?);
                let source = TrackId(u64::from_le_bytes(self.array()?));
                let destination = TrackId(u64::from_le_bytes(self.array()?));
                let kind = match self.array::<1>()?[0] {
                    0 => EdgeKind::Main,
                    1 => EdgeKind::SendPreFader,
                    2 => EdgeKind::SendPostFader,
                    3 => EdgeKind::Sidechain,
                    _ => return Err(PersistenceError::InvalidFormat),
                };
                let gain = f32::from_le_bytes(self.array()?);
                if id == 0
                    || id >= next_id
                    || !ids.insert(id)
                    || !tracks.iter().any(|track| track.id == source)
                    || !tracks.iter().any(|track| track.id == destination)
                    || !gain.is_finite()
                    || !(0.0..=4.0).contains(&gain)
                {
                    return Err(PersistenceError::InvalidFormat);
                }
                routes.push(Route {
                    id: RouteId(id),
                    source,
                    destination,
                    kind,
                    gain,
                });
            }
        }

        for track in &mut tracks {
            let slot_count = self.count()?;
            if slot_count != scenes.len() {
                return Err(PersistenceError::InvalidFormat);
            }
            for _ in 0..slot_count {
                let id = u64::from_le_bytes(self.array()?);
                if id != 0
                    && !clips.iter().any(|clip| clip.id.0 == id)
                    && !audio_clips.iter().any(|clip| clip.id.0 == id)
                {
                    return Err(PersistenceError::InvalidFormat);
                }
                if id != 0
                    && ((clips.iter().any(|clip| clip.id.0 == id) && track.kind != TrackKind::Midi)
                        || (audio_clips.iter().any(|clip| clip.id.0 == id)
                            && track.kind != TrackKind::Audio))
                {
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
                    && !audio_clips.iter().any(|item| item.id.0 == clip)
                {
                    return Err(PersistenceError::InvalidFormat);
                }
                if (clips.iter().any(|item| item.id.0 == clip) && track.kind != TrackKind::Midi)
                    || (audio_clips.iter().any(|item| item.id.0 == clip)
                        && track.kind != TrackKind::Audio)
                {
                    return Err(PersistenceError::InvalidFormat);
                }
                if !start_beats.is_finite()
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
        if audio_clips.iter().any(|clip| {
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
        let snapshot = Arc::new(Snapshot {
            tempo,
            numerator,
            denominator,
            sample_rate,
            tracks,
            scenes,
            clips,
            audio_clips,
            routes,
        });
        snapshot
            .compiled_routing()
            .map_err(|_| PersistenceError::InvalidFormat)?;
        Ok(snapshot)
    }
}

fn filter_code(kind: FilterKind) -> u8 {
    match kind {
        FilterKind::LowPass => 0,
        FilterKind::HighPass => 1,
        FilterKind::BandPass => 2,
        FilterKind::Notch => 3,
        FilterKind::AllPass => 4,
        FilterKind::Peaking => 5,
        FilterKind::LowShelf => 6,
        FilterKind::HighShelf => 7,
    }
}

fn filter_from_code(code: u8) -> Option<FilterKind> {
    match code {
        0 => Some(FilterKind::LowPass),
        1 => Some(FilterKind::HighPass),
        2 => Some(FilterKind::BandPass),
        3 => Some(FilterKind::Notch),
        4 => Some(FilterKind::AllPass),
        5 => Some(FilterKind::Peaking),
        6 => Some(FilterKind::LowShelf),
        7 => Some(FilterKind::HighShelf),
        _ => None,
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
            bundle_directory: None,
            revision: 0,
            saved_revision: 0,
        })
    }

    /// Write a complete temporary document, synchronize it, then replace the
    /// bundle document. A failed write leaves the previous document intact.
    pub fn save_bundle(&mut self, directory: &Path) -> Result<(), PersistenceError> {
        let bytes = self.to_bytes()?;
        fs::create_dir_all(directory)?;
        let media_paths: HashSet<&str> = std::iter::once(&self.current)
            .chain(self.undo.iter())
            .chain(self.redo.iter())
            .flat_map(|snapshot| snapshot.audio_clips.iter())
            .map(|clip| clip.media_path.as_str())
            .collect();
        if !media_paths.is_empty() && self.bundle_directory.as_deref() != Some(directory) {
            let source = self
                .bundle_directory
                .as_deref()
                .ok_or(PersistenceError::Io)?;
            for media_path in media_paths {
                let destination = directory.join(media_path);
                let parent = destination.parent().ok_or(PersistenceError::Io)?;
                fs::create_dir_all(parent)?;
                fs::copy(source.join(media_path), destination)?;
            }
        }
        write_document(directory, DOCUMENT_NAME, &bytes)?;
        self.bundle_directory = Some(directory.to_path_buf());
        self.saved_revision = self.revision;
        Self::discard_recovery(directory)?;
        Ok(())
    }

    pub fn load_bundle(directory: &Path) -> Result<Self, PersistenceError> {
        let bytes = read_document(&directory.join(DOCUMENT_NAME))?;
        let mut project = Self::from_bytes(&bytes)?;
        project.bundle_directory = Some(directory.to_path_buf());
        Ok(project)
    }

    /// Write the current state to the bundle recovery sidecar.
    pub fn autosave(&self) -> Result<(), PersistenceError> {
        let directory = self
            .bundle_directory
            .as_deref()
            .ok_or(PersistenceError::Io)?;
        if !self.is_modified() {
            return Self::discard_recovery(directory);
        }
        write_document(directory, RECOVERY_NAME, &self.to_bytes()?)
    }

    /// Report whether a complete recovery document is present.
    pub fn recovery_available(directory: &Path) -> bool {
        read_document(&directory.join(RECOVERY_NAME))
            .and_then(|bytes| Self::from_bytes(&bytes))
            .is_ok()
    }

    /// Load the recovery document while retaining its bundle origin.
    pub fn recover_bundle(directory: &Path) -> Result<Self, PersistenceError> {
        let bytes = read_document(&directory.join(RECOVERY_NAME))?;
        let mut project = Self::from_bytes(&bytes)?;
        project.bundle_directory = Some(directory.to_path_buf());
        project.revision = 1;
        project.saved_revision = 0;
        Ok(project)
    }

    pub fn discard_recovery(directory: &Path) -> Result<(), PersistenceError> {
        match fs::remove_file(directory.join(RECOVERY_NAME)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(PersistenceError::Io),
        }
    }
}

fn read_document(path: &Path) -> Result<Vec<u8>, PersistenceError> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_BYTES as u64 {
        return Err(PersistenceError::SizeLimit);
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn write_document(directory: &Path, name: &str, bytes: &[u8]) -> Result<(), PersistenceError> {
    fs::create_dir_all(directory)?;
    let sequence = SAVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(".{name}-{}-{sequence}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result: Result<(), PersistenceError> = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace_document(&temporary, &directory.join(name))?;
        #[cfg(unix)]
        File::open(directory)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn replace_document(source: &Path, destination: &Path) -> Result<(), PersistenceError> {
    fs::rename(source, destination)?;
    Ok(())
}

#[cfg(windows)]
fn replace_document(source: &Path, destination: &Path) -> Result<(), PersistenceError> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
    }

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: Both strings are NUL-terminated and live for the duration of the call.
    let replaced = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        Err(PersistenceError::Io)
    } else {
        Ok(())
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
