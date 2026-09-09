//! Deterministic project rendering into a WAVE destination.
//!
//! A bounce uses the same playback engine and mixer as a device stream,
//! but drives them through the offline backend. The output is produced a
//! block at a time, so memory use does not grow with project duration.

use crate::audio::offline::{DEVICE, OfflineBackend};
use crate::audio::{AudioError, Backend, Stream, StreamConfig};
use crate::engine::device::DeviceConfig;
use crate::engine::playback::{MAX_INSTRUMENTS, PlaybackEngine, Score};
use crate::engine::schedule::ScheduledNote;
use crate::media::{MediaError, timeline_from_project};
use crate::project::Project;
use crate::runtime::{playback_devices, playback_routing, state_from_snapshot};
use crate::wave::{Format, WaveError, WaveWriter};
use std::io::{Seek, Write};

/// Range and file settings for one bounce.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// First beat written to the file.
    pub start_beats: f64,
    /// Beat immediately after the rendered range.
    pub end_beats: f64,
    /// Frames rendered per offline callback.
    pub block_frames: usize,
    /// File encoding. A bounce requires two channels.
    pub format: Format,
}

impl Options {
    /// A full-range stereo 24-bit bounce at the given rate.
    #[must_use]
    pub const fn stereo(end_beats: f64, sample_rate: u32) -> Self {
        Self {
            start_beats: 0.0,
            end_beats,
            block_frames: 512,
            format: Format::stereo(sample_rate),
        }
    }

    /// Checks the range and file configuration before a destination is opened.
    ///
    /// # Errors
    ///
    /// Returns [`BounceError`] when the range, stream block, or file format
    /// cannot be rendered.
    pub fn validate(&self) -> Result<(), BounceError> {
        if !self.start_beats.is_finite()
            || !self.end_beats.is_finite()
            || self.start_beats < 0.0
            || self.end_beats <= self.start_beats
        {
            return Err(BounceError::InvalidRange);
        }
        if self.format.channels != 2 {
            return Err(BounceError::Wave(WaveError::Unsupported("channel count")));
        }
        StreamConfig {
            device: DEVICE,
            sample_rate: self.format.sample_rate,
            block_frames: self.block_frames,
            channels: 2,
        }
        .validate()
        .map_err(BounceError::Audio)
    }
}

/// Measurements collected while rendering.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Report {
    /// Stereo frames written.
    pub frames: u64,
    /// Largest absolute sample on the left channel.
    pub peak_left: f32,
    /// Largest absolute sample on the right channel.
    pub peak_right: f32,
}

/// Why a bounce could not finish.
#[derive(Debug)]
pub enum BounceError {
    /// The beat range is empty, negative, or not finite.
    InvalidRange,
    /// The offline stream rejected its configuration or state.
    Audio(AudioError),
    /// The destination or file encoding failed.
    Wave(WaveError),
    /// Referenced project media could not be decoded.
    Media(MediaError),
}

impl core::fmt::Display for BounceError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidRange => formatter.write_str("invalid bounce range"),
            Self::Audio(error) => write!(formatter, "audio: {error}"),
            Self::Wave(error) => write!(formatter, "wave: {error}"),
            Self::Media(error) => write!(formatter, "media: {error}"),
        }
    }
}

impl std::error::Error for BounceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidRange => None,
            Self::Audio(error) => Some(error),
            Self::Wave(error) => Some(error),
            Self::Media(error) => Some(error),
        }
    }
}

impl From<AudioError> for BounceError {
    fn from(error: AudioError) -> Self {
        Self::Audio(error)
    }
}

impl From<WaveError> for BounceError {
    fn from(error: WaveError) -> Self {
        Self::Wave(error)
    }
}

impl From<MediaError> for BounceError {
    fn from(error: MediaError) -> Self {
        Self::Media(error)
    }
}

/// Renders the selected beat range and returns the completed destination
/// with its measurements.
///
/// # Errors
///
/// Returns [`BounceError`] for an invalid range, unsupported file format,
/// offline stream failure, or destination I/O failure.
pub fn render_wave<W: Write + Seek>(
    project: &Project,
    destination: W,
    options: Options,
) -> Result<(W, Report), BounceError> {
    options.validate()?;
    let snapshot = project.snapshot();
    let tempo = snapshot.tempo();
    let duration_beats = options.end_beats - options.start_beats;
    let exact_frames = duration_beats * 60.0 / tempo * f64::from(options.format.sample_rate);
    if !exact_frames.is_finite() || exact_frames <= 0.0 || exact_frames > u64::MAX as f64 {
        return Err(BounceError::InvalidRange);
    }
    let frames = exact_frames.ceil() as u64;
    let config = StreamConfig {
        device: DEVICE,
        sample_rate: options.format.sample_rate,
        block_frames: options.block_frames,
        channels: 2,
    };

    let (engine, mut publisher) = PlaybackEngine::new(f64::from(options.format.sample_rate));
    let (mut settings, mut score) = state_from_snapshot(&snapshot, true);
    prepare_score_range(&mut score, options.start_beats);
    settings.set_locate_beats(Some(options.start_beats));
    if !publisher.publish(&settings) || !publisher.publish_score(&score) {
        return Err(BounceError::Audio(AudioError::Host(
            "initial state could not be published",
        )));
    }
    if !publisher.publish_audio(timeline_from_project(project)?) {
        return Err(BounceError::Audio(AudioError::Host(
            "initial media could not be published",
        )));
    }
    let (routing, output_node) = playback_routing(&snapshot).map_err(|_| {
        BounceError::Audio(AudioError::Host("project routing could not be compiled"))
    })?;
    let devices = playback_devices(&snapshot);
    let device_slices: Vec<&[DeviceConfig]> = devices.iter().map(Vec::as_slice).collect();
    if !publisher
        .publish_routing_with_devices(&routing, output_node, &device_slices)
        .map_err(|_| {
            BounceError::Audio(AudioError::Host("project routing could not be prepared"))
        })?
    {
        return Err(BounceError::Audio(AudioError::Host(
            "initial routing could not be published",
        )));
    }
    let backend = OfflineBackend::new();
    let mut stream = backend.open_output(config, engine)?;
    stream.start()?;

    let mut writer = WaveWriter::new(destination, options.format)?;
    let mut block = vec![[0.0_f32; 2]; options.block_frames];
    let mut report = Report::default();
    while report.frames < frames {
        let remaining = usize::try_from(frames - report.frames).unwrap_or(usize::MAX);
        let count = remaining.min(block.len());
        stream.render_into(&mut block[..count])?;
        for frame in &block[..count] {
            report.peak_left = report.peak_left.max(frame[0].abs());
            report.peak_right = report.peak_right.max(frame[1].abs());
        }
        writer.write_stereo(&block[..count])?;
        report.frames += count as u64;
    }
    stream.stop()?;
    let destination = writer.finish()?;
    Ok((destination, report))
}

fn prepare_score_range(score: &mut Score, start_beats: f64) {
    for index in 0..MAX_INSTRUMENTS {
        let Some(source) = score.track(index).copied() else {
            continue;
        };
        let mut notes = Vec::with_capacity(source.notes().len());
        for note in source.notes() {
            let end = note.start_beats + note.length_beats;
            if note.start_beats < start_beats && end > start_beats {
                notes.push(ScheduledNote {
                    start_beats,
                    length_beats: end - start_beats,
                    pitch: note.pitch,
                    velocity: note.velocity,
                });
            } else {
                notes.push(*note);
            }
        }
        let destination = score.track_mut(index).unwrap_or_else(|| unreachable!());
        destination.set_notes(&notes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::device::DeviceKind;
    use crate::project::{Command, MidiNote, TrackKind};
    use crate::wave;
    use std::io::Cursor;

    fn project_with_note() -> Project {
        let mut project = Project::new();
        project
            .apply(&[
                Command::CreateTrack {
                    name: "Instrument".into(),
                    kind: TrackKind::Midi,
                },
                Command::CreateScene { name: "A".into() },
            ])
            .unwrap();
        let snapshot = project.snapshot();
        let track = snapshot.tracks()[0].id();
        let scene = snapshot.scenes()[0].id();
        project
            .apply(&[Command::CreateMidiClip {
                track,
                scene,
                name: "Note".into(),
                length_beats: 4.0,
            }])
            .unwrap();
        let clip = project.snapshot().clip_at(0, 0).unwrap().id();
        project
            .apply(&[
                Command::AddNote {
                    id: clip,
                    note: MidiNote {
                        pitch: 69,
                        velocity: 100,
                        start_beats: 0.0,
                        length_beats: 1.0,
                    },
                },
                Command::PlaceClip {
                    track,
                    clip,
                    start_beats: 0.0,
                    length_beats: 4.0,
                },
            ])
            .unwrap();
        project
    }

    #[test]
    fn a_project_renders_to_a_readable_file() {
        let project = project_with_note();
        let destination = Cursor::new(Vec::new());
        let (destination, report) =
            render_wave(&project, destination, Options::stereo(1.0, 48_000)).unwrap();
        assert_eq!(report.frames, 24_000);
        assert!(report.peak_left > 0.01, "{report:?}");
        assert!(report.peak_right > 0.01, "{report:?}");
        let file = wave::read(Cursor::new(destination.into_inner())).unwrap();
        assert_eq!(file.frames(), 24_000);
        assert_eq!(file.format, Format::stereo(48_000));
    }

    #[test]
    fn the_same_project_is_bit_identical() {
        let project = project_with_note();
        let render = || {
            render_wave(
                &project,
                Cursor::new(Vec::new()),
                Options::stereo(2.0, 44_100),
            )
            .unwrap()
            .0
            .into_inner()
        };
        assert_eq!(render(), render());
    }

    #[test]
    fn project_routing_changes_the_offline_mix() {
        let direct = project_with_note();
        let direct_peak = render_wave(
            &direct,
            Cursor::new(Vec::new()),
            Options::stereo(0.25, 48_000),
        )
        .unwrap()
        .1
        .peak_left;

        let mut routed = project_with_note();
        routed
            .apply(&[Command::CreateTrack {
                name: "Bus".into(),
                kind: TrackKind::Return,
            }])
            .unwrap();
        let snapshot = routed.snapshot();
        let source = snapshot.tracks()[0].id();
        let bus = snapshot.tracks()[1].id();
        routed
            .apply(&[
                Command::SetTrackVolume {
                    id: bus,
                    db: -6.020_6,
                },
                Command::CreateRoute {
                    source,
                    destination: bus,
                    kind: crate::routing::EdgeKind::Main,
                    gain: 1.0,
                },
            ])
            .unwrap();
        let routed_peak = render_wave(
            &routed,
            Cursor::new(Vec::new()),
            Options::stereo(0.25, 48_000),
        )
        .unwrap()
        .1
        .peak_left;

        assert!((routed_peak / direct_peak - 0.5).abs() < 0.01);
    }

    #[test]
    fn project_devices_process_the_offline_mix() {
        let direct = project_with_note();
        let direct_peak = render_wave(
            &direct,
            Cursor::new(Vec::new()),
            Options::stereo(0.25, 48_000),
        )
        .unwrap()
        .1
        .peak_left;
        let mut processed = project_with_note();
        let track = processed.snapshot().tracks()[0].id();
        processed
            .apply(&[Command::AddDevice {
                track,
                kind: DeviceKind::Utility {
                    gain_db: -6.020_6,
                    width: 1.0,
                    balance: 0.0,
                },
            }])
            .unwrap();
        let processed_peak = render_wave(
            &processed,
            Cursor::new(Vec::new()),
            Options::stereo(0.25, 48_000),
        )
        .unwrap()
        .1
        .peak_left;
        assert!((processed_peak / direct_peak - 0.5).abs() < 0.01);
    }

    #[test]
    fn a_partial_range_starts_at_its_requested_beat() {
        let project = project_with_note();
        let options = Options {
            start_beats: 0.5,
            end_beats: 1.0,
            block_frames: 127,
            format: Format::stereo(48_000),
        };
        let (_, report) = render_wave(&project, Cursor::new(Vec::new()), options).unwrap();
        assert_eq!(report.frames, 12_000);
        assert!(report.peak_left > 0.0);
    }

    #[test]
    fn invalid_ranges_and_formats_are_rejected_before_writing() {
        let project = Project::new();
        for (start, end) in [(0.0, 0.0), (2.0, 1.0), (-1.0, 1.0), (f64::NAN, 1.0)] {
            let options = Options {
                start_beats: start,
                end_beats: end,
                ..Options::stereo(1.0, 48_000)
            };
            assert!(matches!(
                render_wave(&project, Cursor::new(Vec::new()), options),
                Err(BounceError::InvalidRange)
            ));
        }
        let mono = Options {
            format: Format {
                sample_rate: 48_000,
                channels: 1,
                bits: 24,
                sample_format: wave::SampleFormat::Integer,
            },
            ..Options::stereo(1.0, 48_000)
        };
        assert!(matches!(
            render_wave(&project, Cursor::new(Vec::new()), mono),
            Err(BounceError::Wave(WaveError::Unsupported("channel count")))
        ));
    }
}
