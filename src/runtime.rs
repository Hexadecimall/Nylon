//! Control-thread owner for live audio playback.
//!
//! Project snapshots are translated into fixed-capacity mixer and score
//! values before publication. The platform stream owns the renderer; this
//! type retains the control endpoint used by transport, meters, and edits.

#[cfg(platform_audio)]
use crate::audio::recording::{RecordingError, RecordingSession};
use crate::audio::{AudioError, Backend, DeviceId, DeviceInfo, InputBackend, Stream, StreamConfig};
use crate::engine::automation::{
    Lane as PlaybackAutomationLane, Point as PlaybackAutomationPoint,
    Timeline as AutomationTimeline,
};
use crate::engine::device::DeviceConfig;
use crate::engine::playback::{
    MixSettings, PlaybackEngine, PlaybackState, Publisher, Score, TrackSettings,
};
use crate::engine::schedule::ScheduledNote;
use crate::media::timeline_from_project;
#[cfg(platform_audio)]
use crate::media::{ImportReport, MediaError, PendingRecording, prepare_recording};
use crate::mixer::{AutomationCurve as PlaybackAutomationCurve, Parameter};
use crate::project::{AutomationCurve, AutomationParameter, Project, Snapshot, TrackKind};
use crate::routing::{CompiledRouting, Edge, EdgeKind, RoutingError, RoutingGraph};

#[cfg(target_os = "linux")]
use crate::audio::alsa::{AlsaBackend, AlsaCapture, AlsaStream};
#[cfg(target_os = "macos")]
use crate::audio::coreaudio::{CoreAudioBackend, CoreAudioCapture, CoreAudioStream};
#[cfg(target_os = "windows")]
use crate::audio::wasapi::{WasapiBackend, WasapiCapture, WasapiStream};

/// Maximum number of devices returned through the native interface.
pub const MAX_DEVICES: usize = 64;

#[cfg(target_os = "macos")]
type PlatformStream = CoreAudioStream;
#[cfg(target_os = "linux")]
type PlatformStream = AlsaStream;
#[cfg(target_os = "windows")]
type PlatformStream = WasapiStream;

#[cfg(target_os = "macos")]
type PlatformCapture = CoreAudioCapture;
#[cfg(target_os = "linux")]
type PlatformCapture = AlsaCapture;
#[cfg(target_os = "windows")]
type PlatformCapture = WasapiCapture;

#[cfg(platform_audio)]
fn open_platform_recording(
    config: StreamConfig,
    file: std::fs::File,
    destination: &std::path::Path,
) -> Result<RecordingSession<PlatformCapture>, RecordingRuntimeError> {
    #[cfg(target_os = "macos")]
    let backend = CoreAudioBackend::new();
    #[cfg(target_os = "linux")]
    let backend = AlsaBackend::new()?;
    #[cfg(target_os = "windows")]
    let backend = WasapiBackend::new();
    RecordingSession::open_file(
        &backend,
        config,
        file,
        destination,
        crate::audio::recording::DEFAULT_QUEUE_BLOCKS,
    )
    .map_err(Into::into)
}

/// Why a project recording could not start or finish.
#[cfg(platform_audio)]
#[derive(Debug)]
pub enum RecordingRuntimeError {
    Audio(AudioError),
    Recording(RecordingError),
    Media(MediaError),
    WrongState,
}

#[cfg(platform_audio)]
impl From<AudioError> for RecordingRuntimeError {
    fn from(error: AudioError) -> Self {
        Self::Audio(error)
    }
}

#[cfg(platform_audio)]
impl From<RecordingError> for RecordingRuntimeError {
    fn from(error: RecordingError) -> Self {
        Self::Recording(error)
    }
}

#[cfg(platform_audio)]
impl From<MediaError> for RecordingRuntimeError {
    fn from(error: MediaError) -> Self {
        Self::Media(error)
    }
}

/// Result of registering one recorded file in the project.
#[cfg(platform_audio)]
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRecordingReport {
    pub media: ImportReport,
    pub lost_blocks: u64,
    pub lost_frames: u64,
}

/// Control-thread owner for one platform input and project media reservation.
#[cfg(platform_audio)]
pub struct ProjectRecording {
    session: Option<RecordingSession<PlatformCapture>>,
    target: Option<PendingRecording>,
}

#[cfg(platform_audio)]
impl ProjectRecording {
    /// Reserves the project slot and opens a stopped platform input.
    pub fn open(
        project: &Project,
        track: usize,
        scene: usize,
        device: DeviceId,
        sample_rate: u32,
        block_frames: usize,
    ) -> Result<Self, RecordingRuntimeError> {
        let mut target = prepare_recording(project, track, scene)?;
        let destination = target.destination().to_path_buf();
        let file = target.take_file()?;
        let config = StreamConfig {
            device,
            sample_rate,
            block_frames,
            channels: 2,
        };
        let session = open_platform_recording(config, file, &destination)?;
        Ok(Self {
            session: Some(session),
            target: Some(target),
        })
    }

    pub fn start(&mut self) -> Result<(), RecordingRuntimeError> {
        self.session
            .as_mut()
            .ok_or(RecordingRuntimeError::WrongState)?
            .start()
            .map_err(Into::into)
    }

    pub fn stop(&mut self) -> Result<(), RecordingRuntimeError> {
        self.session
            .as_mut()
            .ok_or(RecordingRuntimeError::WrongState)?
            .stop()
            .map_err(Into::into)
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        self.session
            .as_ref()
            .and_then(|session| session.stream())
            .is_some_and(Stream::is_running)
    }

    pub fn finish(
        &mut self,
        project: &mut Project,
    ) -> Result<ProjectRecordingReport, RecordingRuntimeError> {
        let session = self
            .session
            .take()
            .ok_or(RecordingRuntimeError::WrongState)?;
        let target = self
            .target
            .take()
            .ok_or(RecordingRuntimeError::WrongState)?;
        let captured = session.finish()?;
        let media = target.commit(project, &captured)?;
        Ok(ProjectRecordingReport {
            media,
            lost_blocks: captured.lost_blocks,
            lost_frames: captured.lost_frames,
        })
    }
}

/// Opens the host's output and starts it.
///
/// Each platform has one backend; a platform with none does not compile
/// this and reports the absence at the call.
#[cfg(platform_audio)]
fn start_platform_stream(
    config: StreamConfig,
    engine: PlaybackEngine,
) -> Result<PlatformStream, AudioError> {
    #[cfg(target_os = "macos")]
    let backend = CoreAudioBackend::new();
    #[cfg(target_os = "linux")]
    let backend = AlsaBackend::new()?;
    #[cfg(target_os = "windows")]
    let backend = WasapiBackend::new();
    let mut stream = backend.open_output(config, engine)?;
    stream.start()?;
    Ok(stream)
}

/// Live engine state owned by the control thread.
pub struct AudioRuntime {
    settings: MixSettings,
    score: Box<Score>,
    publisher: Option<Publisher>,
    #[cfg(platform_audio)]
    stream: Option<PlatformStream>,
    state: PlaybackState,
    dirty_settings: bool,
    dirty_score: bool,
}

impl Default for AudioRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioRuntime {
    /// Creates a closed runtime.
    #[must_use]
    pub fn new() -> Self {
        Self {
            settings: MixSettings::new(),
            score: Score::boxed(),
            publisher: None,
            #[cfg(platform_audio)]
            stream: None,
            state: PlaybackState::default(),
            dirty_settings: false,
            dirty_score: false,
        }
    }

    /// Opens and starts a platform output stream. The musical transport
    /// remains stopped until [`play`](Self::play) is called.
    ///
    /// # Errors
    ///
    /// Returns the platform error when the device or configuration cannot
    /// be opened.
    pub fn open(
        &mut self,
        project: &Project,
        device: DeviceId,
        sample_rate: u32,
        block_frames: usize,
    ) -> Result<StreamConfig, AudioError> {
        self.close();
        let config = StreamConfig {
            device,
            sample_rate,
            block_frames,
            channels: 2,
        };
        config.validate()?;
        let snapshot = project.snapshot();
        let timeline = timeline_from_project(project)
            .map_err(|_| AudioError::Host("project media could not be loaded"))?;
        let (routing, output_node) = playback_routing(&snapshot, sample_rate as f32)
            .map_err(|_| AudioError::Host("project routing could not be compiled"))?;
        let devices = playback_devices(&snapshot);
        let automation = automation_from_snapshot(&snapshot);
        let device_slices: Vec<&[DeviceConfig]> = devices.iter().map(Vec::as_slice).collect();
        let (engine, mut publisher) = PlaybackEngine::new(f64::from(sample_rate));
        let playing = false;
        (self.settings, self.score) = state_from_snapshot(&snapshot, playing);
        if !publisher.publish(&self.settings)
            || !publisher.publish_score(&self.score)
            || !publisher.publish_audio(timeline)
            || !publisher.publish_automation(automation)
            || !publisher
                .publish_routing_with_devices(&routing, output_node, &device_slices)
                .map_err(|_| AudioError::Host("project routing could not be prepared"))?
        {
            return Err(AudioError::Host("initial state could not be published"));
        }

        #[cfg(platform_audio)]
        {
            let stream = start_platform_stream(config, engine)?;
            let granted = stream.config();
            self.publisher = Some(publisher);
            self.stream = Some(stream);
            self.state = PlaybackState::default();
            self.dirty_settings = false;
            self.dirty_score = false;
            Ok(granted)
        }

        #[cfg(not(platform_audio))]
        {
            let _ = engine;
            let _ = publisher;
            Err(AudioError::Host("no platform output backend"))
        }
    }

    /// Stops and releases the platform stream. A closed runtime may be
    /// opened again.
    pub fn close(&mut self) {
        #[cfg(platform_audio)]
        {
            self.stream = None;
        }
        self.publisher = None;
        self.settings.set_playing(false);
        self.state = PlaybackState::default();
        self.dirty_settings = false;
        self.dirty_score = false;
    }

    /// Whether a platform stream is open and producing callbacks.
    #[must_use]
    pub fn is_open(&self) -> bool {
        #[cfg(platform_audio)]
        {
            self.stream
                .as_ref()
                .is_some_and(|stream| stream.is_running() && !stream.is_lost())
        }
        #[cfg(not(platform_audio))]
        {
            false
        }
    }

    /// Configuration granted by the host.
    #[must_use]
    pub fn config(&self) -> Option<StreamConfig> {
        #[cfg(platform_audio)]
        {
            self.stream.as_ref().map(Stream::config)
        }
        #[cfg(not(platform_audio))]
        {
            None
        }
    }

    /// Whether the device was taken away while the stream was open.
    #[must_use]
    pub fn is_device_lost(&self) -> bool {
        #[cfg(platform_audio)]
        {
            self.stream.as_ref().is_some_and(Stream::is_lost)
        }
        #[cfg(not(platform_audio))]
        {
            false
        }
    }

    /// Blocks dropped by the host callback.
    #[must_use]
    pub fn dropouts(&self) -> u64 {
        #[cfg(platform_audio)]
        {
            self.stream.as_ref().map_or(0, Stream::dropouts)
        }
        #[cfg(not(platform_audio))]
        {
            0
        }
    }

    /// Rebuilds mixer and note state from the current project snapshot.
    /// The transport's play state is preserved.
    pub fn sync_project(&mut self, project: &Project) -> bool {
        if !self.is_open() {
            return false;
        }
        let Ok(timeline) = timeline_from_project(project) else {
            return false;
        };
        let snapshot = project.snapshot();
        let sample_rate = self
            .config()
            .map_or(snapshot.sample_rate(), |config| config.sample_rate);
        let Ok((routing, output_node)) = playback_routing(&snapshot, sample_rate as f32) else {
            return false;
        };
        let devices = playback_devices(&snapshot);
        let automation = automation_from_snapshot(&snapshot);
        let device_slices: Vec<&[DeviceConfig]> = devices.iter().map(Vec::as_slice).collect();
        let playing = self.settings.is_playing();
        (self.settings, self.score) = state_from_snapshot(&snapshot, playing);
        self.dirty_settings = true;
        self.dirty_score = true;
        let state_sent = self.flush();
        let audio_sent = self
            .publisher
            .as_mut()
            .is_some_and(|publisher| publisher.publish_audio(timeline));
        let automation_sent = self
            .publisher
            .as_mut()
            .is_some_and(|publisher| publisher.publish_automation(automation));
        let routing_sent = self.publisher.as_mut().is_some_and(|publisher| {
            publisher
                .publish_routing_with_devices(&routing, output_node, &device_slices)
                .unwrap_or(false)
        });
        state_sent && audio_sent && automation_sent && routing_sent
    }

    /// Starts the musical transport.
    pub fn play(&mut self) -> bool {
        if !self.is_open() {
            return false;
        }
        self.settings.set_playing(true);
        self.dirty_settings = true;
        let _ = self.flush_settings();
        true
    }

    /// Stops the musical transport while leaving the device stream open.
    pub fn stop(&mut self) -> bool {
        if !self.is_open() {
            return false;
        }
        self.settings.set_playing(false);
        self.dirty_settings = true;
        let _ = self.flush_settings();
        true
    }

    /// Moves the playhead at the next block boundary.
    pub fn locate(&mut self, beats: f64) -> bool {
        if !self.is_open() || !beats.is_finite() || beats < 0.0 {
            return false;
        }
        self.settings.set_locate_beats(Some(beats));
        self.dirty_settings = true;
        let _ = self.flush_settings();
        true
    }

    /// Latest engine state. Polling also retries publications that arrived
    /// faster than the audio thread could accept them.
    pub fn state(&mut self) -> PlaybackState {
        if let Some(publisher) = self.publisher.as_mut() {
            self.state = publisher.state();
        }
        let _ = self.flush();
        self.state
    }

    fn flush(&mut self) -> bool {
        let settings = self.flush_settings();
        let score = if self.dirty_score {
            let accepted = self
                .publisher
                .as_mut()
                .is_some_and(|publisher| publisher.publish_score(&self.score));
            if accepted {
                self.dirty_score = false;
            }
            accepted
        } else {
            true
        };
        settings && score
    }

    fn flush_settings(&mut self) -> bool {
        if !self.dirty_settings {
            return true;
        }
        let accepted = self
            .publisher
            .as_mut()
            .is_some_and(|publisher| publisher.publish(&self.settings));
        if accepted {
            self.dirty_settings = false;
            // A locate is a one-shot command carried by this publication.
            // Clearing it locally prevents an unrelated later edit from
            // moving the playhead back to the same point.
            self.settings.set_locate_beats(None);
        }
        accepted
    }
}

pub(crate) fn playback_routing(
    snapshot: &Snapshot,
    sample_rate: f32,
) -> Result<(CompiledRouting, u16), RoutingError> {
    let track_count = snapshot.tracks().len();
    let output_node = u16::try_from(track_count).map_err(|_| RoutingError::NodeCapacity)?;
    let mut graph = RoutingGraph::new(track_count + 1)?;
    for (index, track) in snapshot.tracks().iter().enumerate() {
        let latency =
            track
                .devices()
                .iter()
                .try_fold(track.latency_frames(), |total, device| {
                    total
                        .checked_add(
                            device
                                .config()
                                .latency_frames(sample_rate)
                                .map_err(|_| RoutingError::LatencyOverflow)?,
                        )
                        .ok_or(RoutingError::LatencyOverflow)
                })?;
        graph.set_node_latency(index as u16, latency)?;
    }
    for route in snapshot.routes() {
        let source = snapshot
            .tracks()
            .iter()
            .position(|track| track.id() == route.source())
            .ok_or(RoutingError::InvalidNode)?;
        let destination = snapshot
            .tracks()
            .iter()
            .position(|track| track.id() == route.destination())
            .ok_or(RoutingError::InvalidNode)?;
        graph.add_edge(Edge {
            source: source as u16,
            destination: destination as u16,
            kind: route.kind(),
            gain: route.gain(),
        })?;
    }
    for (index, track) in snapshot.tracks().iter().enumerate() {
        let has_main_route = snapshot
            .routes()
            .iter()
            .any(|route| route.source() == track.id() && route.kind() == EdgeKind::Main);
        if !has_main_route {
            graph.add_edge(Edge {
                source: index as u16,
                destination: output_node,
                kind: EdgeKind::Main,
                gain: 1.0,
            })?;
        }
    }
    Ok((graph.compile()?, output_node))
}

pub(crate) fn playback_devices(snapshot: &Snapshot) -> Vec<Vec<DeviceConfig>> {
    let mut result: Vec<Vec<DeviceConfig>> = snapshot
        .tracks()
        .iter()
        .map(|track| {
            track
                .devices()
                .iter()
                .map(|device| device.config())
                .collect()
        })
        .collect();
    result.push(Vec::new());
    result
}

pub(crate) fn automation_from_snapshot(snapshot: &Snapshot) -> AutomationTimeline {
    let mut timeline = AutomationTimeline::new();
    for (track_index, track) in snapshot.tracks().iter().enumerate() {
        for lane in track.automation() {
            let parameter = match lane.parameter() {
                AutomationParameter::Volume => Parameter::Volume,
                AutomationParameter::Pan => Parameter::Pan,
                AutomationParameter::Mute => Parameter::Mute,
                AutomationParameter::Solo => Parameter::Solo,
            };
            let points = lane
                .points()
                .iter()
                .map(|point| PlaybackAutomationPoint {
                    beat: point.beat,
                    value: point.value,
                    curve: match point.curve {
                        AutomationCurve::Step => PlaybackAutomationCurve::Step,
                        AutomationCurve::Linear => PlaybackAutomationCurve::Linear,
                        AutomationCurve::Smooth => PlaybackAutomationCurve::Smooth,
                    },
                })
                .collect();
            let added = timeline.add_lane(PlaybackAutomationLane {
                track: track_index as u16,
                parameter,
                points,
            });
            debug_assert!(added);
        }
    }
    timeline
}

/// Lists output devices available from the platform backend.
///
/// # Errors
///
/// Returns the platform query error.
pub fn output_devices(out: &mut [DeviceInfo]) -> Result<usize, AudioError> {
    #[cfg(target_os = "macos")]
    {
        CoreAudioBackend::new().devices(out)
    }
    #[cfg(target_os = "linux")]
    {
        // A machine without the sound library has no devices rather than
        // an error to report.
        AlsaBackend::new().map_or(Ok(0), |backend| backend.devices(out))
    }
    #[cfg(target_os = "windows")]
    {
        WasapiBackend::new().devices(out)
    }
    #[cfg(not(platform_audio))]
    {
        let _ = out;
        Ok(0)
    }
}

/// Lists input devices available from the platform backend.
///
/// # Errors
///
/// Returns the platform query error.
pub fn input_devices(out: &mut [DeviceInfo]) -> Result<usize, AudioError> {
    #[cfg(target_os = "macos")]
    {
        CoreAudioBackend::new().input_devices(out)
    }
    #[cfg(target_os = "linux")]
    {
        AlsaBackend::new().map_or(Ok(0), |backend| backend.input_devices(out))
    }
    #[cfg(target_os = "windows")]
    {
        WasapiBackend::new().input_devices(out)
    }
    #[cfg(not(platform_audio))]
    {
        let _ = out;
        Ok(0)
    }
}

/// Default platform output device.
///
/// # Errors
///
/// Returns [`AudioError::DeviceMissing`] when no platform backend or
/// default device exists.
pub fn default_output() -> Result<DeviceId, AudioError> {
    #[cfg(target_os = "macos")]
    {
        CoreAudioBackend::new().default_output()
    }
    #[cfg(target_os = "linux")]
    {
        AlsaBackend::new()?.default_output()
    }
    #[cfg(target_os = "windows")]
    {
        WasapiBackend::new().default_output()
    }
    #[cfg(not(platform_audio))]
    {
        Err(AudioError::DeviceMissing)
    }
}

/// Default platform input device.
///
/// # Errors
///
/// Returns [`AudioError::DeviceMissing`] when no platform backend or
/// default device exists.
pub fn default_input() -> Result<DeviceId, AudioError> {
    #[cfg(target_os = "macos")]
    {
        CoreAudioBackend::new().default_input()
    }
    #[cfg(target_os = "linux")]
    {
        AlsaBackend::new()?.default_input()
    }
    #[cfg(target_os = "windows")]
    {
        WasapiBackend::new().default_input()
    }
    #[cfg(not(platform_audio))]
    {
        Err(AudioError::DeviceMissing)
    }
}

pub(crate) fn state_from_snapshot(snapshot: &Snapshot, playing: bool) -> (MixSettings, Box<Score>) {
    let mut settings = MixSettings::new();
    settings.set_track_count(snapshot.tracks().len());
    settings.set_tempo(snapshot.tempo());
    let (numerator, denominator) = snapshot.time_signature();
    settings.set_time_signature(numerator, denominator);
    settings.set_playing(playing);

    let mut score = Score::boxed();
    for (index, track) in snapshot.tracks().iter().enumerate() {
        settings.set_track(
            index,
            TrackSettings {
                volume_db: track.volume_db() as f32,
                pan: track.pan() as f32,
                muted: track.muted(),
                soloed: track.solo(),
            },
        );
        if track.kind() != TrackKind::Midi {
            continue;
        }
        let Some(destination) = score.track_mut(index) else {
            continue;
        };
        destination.set_enabled(true);
        let mut notes = Vec::new();
        for placement in track.arrangement() {
            let Some(clip) = snapshot.clips.iter().find(|clip| clip.id == placement.clip) else {
                continue;
            };
            append_placement_notes(placement, clip, &mut notes);
        }
        destination.set_notes(&notes);
    }
    (settings, score)
}

fn append_placement_notes(
    placement: &crate::project::ArrangementPlacement,
    clip: &crate::project::MidiClip,
    out: &mut Vec<ScheduledNote>,
) {
    let (loop_start, loop_length) = clip.loop_range();
    if loop_length <= 0.0 || placement.length_beats() <= 0.0 {
        return;
    }
    let end = placement.start_beats() + placement.length_beats();
    let mut cycle = placement.start_beats();
    while cycle < end && out.len() < crate::engine::playback::MAX_NOTES_PER_TRACK {
        for note in clip.notes() {
            let relative = note.start_beats - loop_start;
            if relative < 0.0 || relative >= loop_length {
                continue;
            }
            let start = cycle + relative;
            if start >= end {
                continue;
            }
            let length = note.length_beats.min(end - start);
            if length > 0.0 {
                out.push(ScheduledNote {
                    start_beats: start,
                    length_beats: length,
                    pitch: note.pitch,
                    velocity: note.velocity,
                });
                if out.len() == crate::engine::playback::MAX_NOTES_PER_TRACK {
                    break;
                }
            }
        }
        cycle += loop_length;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::limiter::Parameters as LimiterParameters;
    use crate::engine::device::{DeviceConfig, DeviceKind};
    use crate::project::{AutomationPoint, Command, MidiNote};

    fn midi_project() -> Project {
        let mut project = Project::new();
        project
            .apply(&[
                Command::SetTempo(96.0),
                Command::CreateTrack {
                    name: "Audio".into(),
                    kind: TrackKind::Audio,
                },
                Command::CreateTrack {
                    name: "Keys".into(),
                    kind: TrackKind::Midi,
                },
            ])
            .unwrap();
        let snapshot = project.snapshot();
        let track = snapshot.tracks()[1].id();
        project
            .apply(&[Command::SetTrackVolume {
                id: track,
                db: -6.0,
            }])
            .unwrap();
        project
    }

    #[test]
    fn project_mix_state_maps_to_engine_settings() {
        let project = midi_project();
        let (settings, score) = state_from_snapshot(&project.snapshot(), true);
        assert_eq!(settings.track_count(), 2);
        assert_eq!(settings.tempo(), 96.0);
        assert!(settings.is_playing());
        assert_eq!(settings.track(1).volume_db, -6.0);
        assert!(score.track(1).unwrap().is_enabled());
        assert!(!score.track(0).unwrap().is_enabled());
    }

    #[test]
    fn project_automation_maps_to_the_playback_timeline() {
        let mut project = midi_project();
        let track = project.snapshot().tracks()[1].id();
        project
            .apply(&[Command::SetAutomation {
                track,
                parameter: AutomationParameter::Pan,
                points: vec![AutomationPoint {
                    beat: 2.0,
                    value: 0.5,
                    curve: AutomationCurve::Smooth,
                }],
            }])
            .unwrap();
        let timeline = automation_from_snapshot(&project.snapshot());
        assert_eq!(timeline.lanes().len(), 1);
        assert_eq!(timeline.lanes()[0].track, 1);
        assert_eq!(timeline.lanes()[0].parameter, Parameter::Pan);
        assert_eq!(timeline.lanes()[0].points[0].value, 0.5);
        assert_eq!(
            timeline.lanes()[0].points[0].curve,
            PlaybackAutomationCurve::Smooth
        );
    }

    #[test]
    fn project_routes_compile_with_an_explicit_master_bus() {
        let mut project = midi_project();
        let snapshot = project.snapshot();
        let audio = snapshot.tracks()[0].id();
        let keys = snapshot.tracks()[1].id();
        project
            .apply(&[
                Command::SetTrackLatency {
                    id: audio,
                    frames: 192,
                },
                Command::CreateRoute {
                    source: audio,
                    destination: keys,
                    kind: EdgeKind::Main,
                    gain: 0.75,
                },
            ])
            .unwrap();

        let (compiled, output) = playback_routing(&project.snapshot(), 48_000.0).unwrap();
        assert_eq!(output, 2);
        assert!(compiled.edges().iter().any(|edge| {
            edge.source == 0
                && edge.destination == 1
                && edge.kind == EdgeKind::Main
                && edge.gain == 0.75
        }));
        assert!(compiled.edges().iter().any(|edge| {
            edge.source == 1 && edge.destination == output && edge.kind == EdgeKind::Main
        }));
        assert!(
            !compiled
                .edges()
                .iter()
                .any(|edge| edge.source == 0 && edge.destination == output)
        );
        assert_eq!(compiled.output_latency(output), Some(192));
    }

    #[test]
    fn device_lookahead_participates_in_route_compensation() {
        let mut project = midi_project();
        let snapshot = project.snapshot();
        let audio = snapshot.tracks()[0].id();
        let keys = snapshot.tracks()[1].id();
        project
            .apply(&[Command::AddDevice {
                track: audio,
                config: DeviceConfig {
                    enabled: true,
                    kind: DeviceKind::Limiter {
                        parameters: LimiterParameters {
                            ceiling_db: -0.3,
                            release_seconds: 0.1,
                            lookahead_seconds: 0.005,
                        },
                    },
                },
            }])
            .unwrap();

        let (compiled, output) = playback_routing(&project.snapshot(), 48_000.0).unwrap();
        assert_eq!(compiled.output_latency(output), Some(240));
        assert!(compiled.edges().iter().enumerate().any(|(index, edge)| {
            edge.source == 0 && edge.destination == output && compiled.edge_delay(index) == Some(0)
        }));
        assert!(compiled.edges().iter().enumerate().any(|(index, edge)| {
            edge.source == 1
                && edge.destination == output
                && compiled.edge_delay(index) == Some(240)
        }));
        assert_ne!(audio, keys);
    }

    #[test]
    fn arrangement_placements_repeat_clip_notes() {
        let mut project = midi_project();
        project
            .apply(&[Command::CreateScene { name: "A".into() }])
            .unwrap();
        let snapshot = project.snapshot();
        let track = snapshot.tracks()[1].id();
        let scene = snapshot.scenes()[0].id();
        project
            .apply(&[Command::CreateMidiClip {
                track,
                scene,
                name: "Pattern".into(),
                length_beats: 2.0,
            }])
            .unwrap();
        let clip = project.snapshot().clip_at(1, 0).unwrap().id();
        project
            .apply(&[
                Command::AddNote {
                    id: clip,
                    note: MidiNote {
                        pitch: 60,
                        velocity: 100,
                        start_beats: 0.5,
                        length_beats: 0.75,
                    },
                },
                Command::PlaceClip {
                    track,
                    clip,
                    start_beats: 4.0,
                    length_beats: 5.0,
                },
            ])
            .unwrap();
        let (_, score) = state_from_snapshot(&project.snapshot(), false);
        let notes = score.track(1).unwrap().notes();
        assert_eq!(notes.len(), 3);
        assert_eq!(notes[0].start_beats, 4.5);
        assert_eq!(notes[1].start_beats, 6.5);
        assert_eq!(notes[2].start_beats, 8.5);
        assert_eq!(notes[2].length_beats, 0.5);
    }

    #[test]
    fn closed_runtime_rejects_transport_commands() {
        let mut runtime = AudioRuntime::new();
        assert!(!runtime.is_open());
        assert!(!runtime.play());
        assert!(!runtime.stop());
        assert!(!runtime.locate(2.0));
        assert_eq!(runtime.state(), PlaybackState::default());
        assert_eq!(runtime.dropouts(), 0);
        assert_eq!(runtime.config(), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "needs a real audio output device"]
    fn platform_runtime_opens_and_advances() {
        let project = midi_project();
        let device = default_output().unwrap();
        let mut runtime = AudioRuntime::new();
        let granted = runtime.open(&project, device, 48_000, 256).unwrap();
        assert!(runtime.is_open());
        assert_eq!(runtime.config(), Some(granted));
        assert!(runtime.play());
        let mut state = PlaybackState::default();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            state = runtime.state();
            if state.playing && state.position_beats > 0.0 {
                break;
            }
        }
        assert!(state.playing);
        assert!(state.position_beats > 0.0, "{}", state.position_beats);
        assert!(runtime.stop());
        runtime.close();
        assert!(!runtime.is_open());
    }
}
