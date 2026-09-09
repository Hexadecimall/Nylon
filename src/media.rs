//! Project media import and control-thread timeline construction.

use crate::audio::recording::RecordingReport;
use crate::dsp::db;
use crate::engine::sample::{Sample, SampleError};
use crate::engine::timeline::{AudioRegion, AudioTimeline};
use crate::project::{Command, Project, ProjectError, Snapshot, TrackKind, validate_name};
use crate::wave::{self, WaveError};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// Why project media could not be imported or prepared for playback.
#[derive(Debug)]
pub enum MediaError {
    MissingBundle,
    InvalidSlot,
    InvalidRecording,
    Capacity,
    Io(io::Error),
    Wave(WaveError),
    Sample(SampleError),
    Project(ProjectError),
}

impl core::fmt::Display for MediaError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingBundle => formatter.write_str("the project has no bundle directory"),
            Self::InvalidSlot => formatter.write_str("the audio clip slot is invalid"),
            Self::InvalidRecording => formatter.write_str("the recording result is invalid"),
            Self::Capacity => formatter.write_str("the audio timeline is full"),
            Self::Io(error) => write!(formatter, "media I/O: {error}"),
            Self::Wave(error) => write!(formatter, "media file: {error}"),
            Self::Sample(error) => write!(formatter, "decoded sample: {error:?}"),
            Self::Project(error) => write!(formatter, "project edit: {error:?}"),
        }
    }
}

impl std::error::Error for MediaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Wave(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for MediaError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<WaveError> for MediaError {
    fn from(error: WaveError) -> Self {
        Self::Wave(error)
    }
}

impl From<SampleError> for MediaError {
    fn from(error: SampleError) -> Self {
        Self::Sample(error)
    }
}

impl From<ProjectError> for MediaError {
    fn from(error: ProjectError) -> Self {
        Self::Project(error)
    }
}

/// Result of copying one WAVE file into a project bundle.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportReport {
    pub media_path: String,
    pub frames: usize,
    pub sample_rate: u32,
    pub length_beats: f64,
}

/// Reserved project media and stable clip destination for one recording.
pub struct PendingRecording {
    track: crate::project::TrackId,
    scene: crate::project::SceneId,
    relative_path: String,
    destination: PathBuf,
    file: Option<File>,
    name: String,
    source_tempo: f64,
    committed: bool,
}

impl PendingRecording {
    /// Hands the reserved file to the recording writer.
    ///
    /// # Errors
    ///
    /// Returns [`MediaError::InvalidRecording`] after it has already been taken.
    pub fn take_file(&mut self) -> Result<File, MediaError> {
        self.file.take().ok_or(MediaError::InvalidRecording)
    }

    /// Absolute runtime destination of the reserved media file.
    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// Creates the audio clip after capture has fully finalized its file.
    ///
    /// # Errors
    ///
    /// Returns an error when the result does not belong to this reservation,
    /// contains no audio, or the selected project slot changed during capture.
    pub fn commit(
        mut self,
        project: &mut Project,
        report: &RecordingReport,
    ) -> Result<ImportReport, MediaError> {
        if self.file.is_some()
            || report.path != self.destination
            || report.frames == 0
            || report.sample_rate == 0
        {
            return Err(MediaError::InvalidRecording);
        }
        let length_beats =
            report.frames as f64 / f64::from(report.sample_rate) * self.source_tempo / 60.0;
        project.apply(&[Command::CreateAudioClip {
            track: self.track,
            scene: self.scene,
            name: self.name.clone(),
            media_path: self.relative_path.clone(),
            length_beats,
            source_tempo: self.source_tempo,
        }])?;
        self.committed = true;
        Ok(ImportReport {
            media_path: self.relative_path.clone(),
            frames: usize::try_from(report.frames).unwrap_or(usize::MAX),
            sample_rate: report.sample_rate,
            length_beats,
        })
    }
}

impl Drop for PendingRecording {
    fn drop(&mut self) {
        if !self.committed {
            self.file.take();
            let _ = fs::remove_file(&self.destination);
        }
    }
}

/// Reserves a unique media file for an audio clip slot.
///
/// # Errors
///
/// Returns an error for an unsaved project, a non-audio or occupied slot,
/// or a destination that cannot be created.
pub fn prepare_recording(
    project: &Project,
    track_index: usize,
    scene_index: usize,
) -> Result<PendingRecording, MediaError> {
    let bundle = project
        .bundle_directory()
        .ok_or(MediaError::MissingBundle)?;
    let track = project
        .current
        .tracks
        .get(track_index)
        .filter(|track| track.kind() == TrackKind::Audio)
        .ok_or(MediaError::InvalidSlot)?;
    let scene = project
        .current
        .scenes
        .get(scene_index)
        .ok_or(MediaError::InvalidSlot)?;
    if track
        .session_slots
        .get(scene_index)
        .is_some_and(Option::is_some)
    {
        return Err(MediaError::InvalidSlot);
    }
    let media_directory = bundle.join("Media");
    fs::create_dir_all(&media_directory)?;
    let (relative_path, destination, file) = create_destination(&media_directory, project.next_id)?;
    Ok(PendingRecording {
        track: track.id(),
        scene: scene.id(),
        relative_path,
        destination,
        file: Some(file),
        name: format!("Recording {}", project.next_id),
        source_tempo: project.current.tempo(),
        committed: false,
    })
}

/// Validates and copies a WAVE file into the current project bundle, then
/// creates an audio clip in the selected session slot.
pub fn import_wave(
    project: &mut Project,
    source: &Path,
    track_index: usize,
    scene_index: usize,
    source_tempo: f64,
) -> Result<ImportReport, MediaError> {
    let bundle = project
        .bundle_directory()
        .ok_or(MediaError::MissingBundle)?
        .to_path_buf();
    let source_file = File::open(source)?;
    let decoded = wave::read(source_file)?;
    let _ = Sample::from_wave(&decoded)?;
    let length_beats = decoded.seconds() * source_tempo / 60.0;
    if !length_beats.is_finite() || length_beats <= 0.0 {
        return Err(MediaError::InvalidSlot);
    }

    let snapshot = project.snapshot();
    let track = snapshot
        .tracks()
        .get(track_index)
        .filter(|track| track.kind() == TrackKind::Audio)
        .ok_or(MediaError::InvalidSlot)?
        .id();
    let scene = snapshot
        .scenes()
        .get(scene_index)
        .ok_or(MediaError::InvalidSlot)?
        .id();
    if snapshot.audio_clip_at(track_index, scene_index).is_some()
        || snapshot.clip_at(track_index, scene_index).is_some()
    {
        return Err(MediaError::InvalidSlot);
    }
    drop(snapshot);

    let media_directory = bundle.join("Media");
    fs::create_dir_all(&media_directory)?;
    let (relative_path, destination, mut destination_file) =
        create_destination(&media_directory, project.next_id)?;
    let copied = (|| -> Result<(), io::Error> {
        let mut source_file = File::open(source)?;
        io::copy(&mut source_file, &mut destination_file)?;
        destination_file.sync_all()
    })();
    if let Err(error) = copied {
        let _ = fs::remove_file(&destination);
        return Err(MediaError::Io(error));
    }

    let name = clip_name(source);
    let result = project.apply(&[Command::CreateAudioClip {
        track,
        scene,
        name,
        media_path: relative_path.clone(),
        length_beats,
        source_tempo,
    }]);
    if let Err(error) = result {
        let _ = fs::remove_file(&destination);
        return Err(MediaError::Project(error));
    }
    Ok(ImportReport {
        media_path: relative_path,
        frames: decoded.frames(),
        sample_rate: decoded.format.sample_rate,
        length_beats,
    })
}

/// One launched audio clip added to a live timeline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SessionAudioRegion {
    pub track: usize,
    pub scene: usize,
    pub launch_beats: f64,
}

/// Builds immutable decoded media and arrangement regions for a project.
pub fn timeline_from_project(project: &Project) -> Result<AudioTimeline, MediaError> {
    timeline_from_project_with_session(project, &[])
}

/// Builds decoded media with launched Session clips replacing Arrangement
/// playback on their tracks.
pub(crate) fn timeline_from_project_with_session(
    project: &Project,
    sessions: &[SessionAudioRegion],
) -> Result<AudioTimeline, MediaError> {
    let snapshot = project.snapshot();
    if snapshot.audio_clips().is_empty() {
        return Ok(AudioTimeline::new());
    }
    let bundle = project
        .bundle_directory()
        .ok_or(MediaError::MissingBundle)?;
    let mut timeline = AudioTimeline::new();
    let mut media_indices = Vec::with_capacity(snapshot.audio_clips().len());
    let mut media_lengths = Vec::with_capacity(snapshot.audio_clips().len());
    let mut media_rates = Vec::with_capacity(snapshot.audio_clips().len());
    for clip in snapshot.audio_clips() {
        let decoded = wave::read(File::open(bundle.join(clip.media_path()))?)?;
        let rate = decoded.format.sample_rate;
        let sample = Sample::from_wave(&decoded)?;
        let length = sample.len();
        let index = timeline
            .add_sample(sample)
            .map_err(|_| MediaError::Capacity)?;
        media_indices.push(index);
        media_lengths.push(length);
        media_rates.push(rate);
    }

    for (track_index, track) in snapshot.tracks().iter().enumerate() {
        if track.kind() != TrackKind::Audio {
            continue;
        }
        if let Some(session) = sessions.iter().find(|session| session.track == track_index) {
            let clip = snapshot
                .audio_clip_at(track_index, session.scene)
                .ok_or(MediaError::InvalidSlot)?;
            let clip_index = snapshot
                .audio_clips()
                .iter()
                .position(|candidate| candidate.id() == clip.id())
                .ok_or(MediaError::InvalidSlot)?;
            add_audio_region(
                &mut timeline,
                &snapshot,
                clip,
                clip_index,
                track_index,
                session.launch_beats,
                1_000_000_000.0,
                &media_indices,
                &media_lengths,
                &media_rates,
            )?;
            continue;
        }
        for placement in track.arrangement() {
            let Some(clip_index) = snapshot
                .audio_clips()
                .iter()
                .position(|clip| clip.id() == placement.clip)
            else {
                return Err(MediaError::InvalidSlot);
            };
            add_audio_region(
                &mut timeline,
                &snapshot,
                &snapshot.audio_clips()[clip_index],
                clip_index,
                track_index,
                placement.start_beats(),
                placement.length_beats(),
                &media_indices,
                &media_lengths,
                &media_rates,
            )?;
        }
    }
    Ok(timeline)
}

#[allow(clippy::too_many_arguments)]
fn add_audio_region(
    timeline: &mut AudioTimeline,
    snapshot: &Snapshot,
    clip: &crate::project::AudioClip,
    clip_index: usize,
    track_index: usize,
    start_beats: f64,
    length_beats: f64,
    media_indices: &[usize],
    media_lengths: &[usize],
    media_rates: &[u32],
) -> Result<(), MediaError> {
    let frames_per_beat = f64::from(media_rates[clip_index]) * 60.0
        / if clip.warped() {
            clip.source_tempo()
        } else {
            snapshot.tempo()
        };
    let (loop_start, loop_length) = clip.loop_range();
    let source_start = loop_start * frames_per_beat;
    let mut region = AudioRegion::new(
        media_indices[clip_index],
        track_index,
        start_beats,
        length_beats,
        source_start,
        frames_per_beat,
    )
    .ok_or(MediaError::InvalidSlot)?;
    region.set_gain(db::to_linear(clip.gain_db() as f32));
    region.set_reverse(clip.reversed());
    let start_frame = source_start.floor().max(0.0) as usize;
    let end_frame = ((loop_start + loop_length) * frames_per_beat)
        .ceil()
        .min(media_lengths[clip_index] as f64) as usize;
    if start_frame < end_frame {
        let _ = region.set_loop(Some(start_frame..end_frame));
    }
    timeline
        .add_region(region)
        .map_err(|_| MediaError::Capacity)?;
    Ok(())
}

fn create_destination(
    media_directory: &Path,
    identifier: u64,
) -> Result<(String, PathBuf, File), MediaError> {
    for suffix in 0_u32..10_000 {
        let file_name = if suffix == 0 {
            format!("clip-{identifier}.wav")
        } else {
            format!("clip-{identifier}-{suffix}.wav")
        };
        let destination = media_directory.join(&file_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
        {
            Ok(file) => return Ok((format!("Media/{file_name}"), destination, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(MediaError::Io(error)),
        }
    }
    Err(MediaError::Capacity)
}

fn clip_name(source: &Path) -> String {
    let candidate = source
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Audio Clip");
    if validate_name(candidate).is_ok() {
        candidate.to_owned()
    } else {
        "Audio Clip".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::schedule::Span;
    use crate::project::TrackKind;
    use crate::wave::{Format, WaveWriter};

    #[test]
    fn a_launched_audio_clip_replaces_its_arrangement_regions() {
        let tick = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = PathBuf::from("target").join(format!("session-audio-{tick}"));
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("source.wav");
        let mut writer =
            WaveWriter::new(File::create(&source).unwrap(), Format::stereo(48_000)).unwrap();
        writer.write_stereo(&[[0.5, -0.5]; 4_800]).unwrap();
        writer.finish().unwrap();

        let bundle = directory.join("Session.nylonproject");
        let mut project = Project::new();
        project.save_bundle(&bundle).unwrap();
        project
            .apply(&[
                Command::CreateTrack {
                    name: "Audio".into(),
                    kind: TrackKind::Audio,
                },
                Command::CreateScene { name: "A".into() },
            ])
            .unwrap();
        import_wave(&mut project, &source, 0, 0, 120.0).unwrap();
        let snapshot = project.snapshot();
        let track = snapshot.tracks()[0].id();
        let clip = snapshot.audio_clip_at(0, 0).unwrap().id();
        project
            .apply(&[Command::PlaceClip {
                track,
                clip,
                start_beats: 8.0,
                length_beats: 1.0,
            }])
            .unwrap();

        let timeline = timeline_from_project_with_session(
            &project,
            &[SessionAudioRegion {
                track: 0,
                scene: 0,
                launch_beats: 2.0,
            }],
        )
        .unwrap();
        assert_eq!(timeline.region_count(), 1);
        let mut before = [[0.0; 2]; 16];
        timeline.render_track(
            0,
            Span {
                start_beats: 1.0,
                length_beats: 0.1,
                frames: before.len(),
            },
            &mut before,
        );
        assert!(before.iter().all(|frame| *frame == [0.0; 2]));
        let mut after = [[0.0; 2]; 16];
        timeline.render_track(
            0,
            Span {
                start_beats: 2.0,
                length_beats: 0.1,
                frames: after.len(),
            },
            &mut after,
        );
        assert!(after.iter().any(|frame| frame[0].abs() > 0.1));
        fs::remove_dir_all(directory).unwrap();
    }
}
