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
use crate::engine::rack::DeviceRack;
use crate::engine::schedule::ScheduledNote;
#[cfg(platform_audio)]
use crate::media::{ImportReport, MediaError, PendingRecording, prepare_recording};
use crate::media::{SessionAudioRegion, timeline_from_project, timeline_from_project_with_session};
use crate::mixer::{AutomationCurve as PlaybackAutomationCurve, MAX_TRACKS, Parameter};
use crate::plugin::bridge::Bridge;
use crate::plugin::clap::ParameterEvent;
use crate::plugin::worker::Client as PluginClient;
use crate::plugin::{Catalog as PluginCatalog, Format as PluginFormat, State as PluginState};
use crate::project::{AutomationCurve, AutomationParameter, Project, Snapshot, TrackKind};
use crate::routing::{CompiledRouting, Edge, EdgeKind, RoutingError, RoutingGraph};
use std::path::{Path, PathBuf};

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
#[derive(Clone, Copy, Debug, PartialEq)]
struct SessionSelection {
    scene: usize,
    launch_beats: f64,
}

pub struct AudioRuntime {
    settings: MixSettings,
    score: Box<Score>,
    publisher: Option<Publisher>,
    #[cfg(platform_audio)]
    stream: Option<PlatformStream>,
    state: PlaybackState,
    dirty_settings: bool,
    dirty_score: bool,
    sessions: [Option<SessionSelection>; MAX_TRACKS],
    plugin_worker: Option<PathBuf>,
    plugin_roots: Vec<PathBuf>,
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
            sessions: [None; MAX_TRACKS],
            plugin_worker: None,
            plugin_roots: Vec::new(),
        }
    }

    /// Configures isolated plugin processing for later stream opens.
    pub fn configure_plugin_host(
        &mut self,
        worker: impl Into<PathBuf>,
        roots: Vec<PathBuf>,
    ) -> bool {
        if self.is_open() || roots.is_empty() || roots.len() > MAX_DEVICES {
            return false;
        }
        let worker = worker.into();
        if worker.as_os_str().is_empty() || roots.iter().any(|root| root.as_os_str().is_empty()) {
            return false;
        }
        self.plugin_worker = Some(worker);
        self.plugin_roots = roots;
        true
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
        self.sessions = [None; MAX_TRACKS];
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
        let devices = playback_racks(
            &snapshot,
            sample_rate as f32,
            block_frames,
            self.plugin_worker.as_deref(),
            &self.plugin_roots,
        )?;
        let device_latencies: Vec<u32> = devices
            .iter()
            .take(snapshot.tracks().len())
            .map(DeviceRack::latency_frames)
            .collect();
        let (routing, output_node) =
            playback_routing_with_device_latencies(&snapshot, &device_latencies)
                .map_err(|_| AudioError::Host("project routing could not be compiled"))?;
        let automation = automation_from_snapshot(&snapshot);
        let (engine, mut publisher) = PlaybackEngine::new(f64::from(sample_rate));
        let playing = false;
        (self.settings, self.score) = state_from_snapshot(&snapshot, playing);
        if !publisher.publish(&self.settings)
            || !publisher.publish_score(&self.score)
            || !publisher.publish_audio(timeline)
            || !publisher.publish_automation(automation)
            || !publisher
                .publish_prepared_routing(&routing, output_node, devices)
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
        self.sessions = [None; MAX_TRACKS];
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

    /// Frames handed to the platform output callback.
    #[must_use]
    pub fn frames_rendered(&self) -> u64 {
        #[cfg(platform_audio)]
        {
            self.stream.as_ref().map_or(0, Stream::frames_rendered)
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
        let snapshot = project.snapshot();
        for (track, session) in self.sessions.iter_mut().enumerate() {
            if session.is_some_and(|active| {
                snapshot.clip_at(track, active.scene).is_none()
                    && snapshot.audio_clip_at(track, active.scene).is_none()
            }) {
                *session = None;
            }
        }
        let audio_sessions = session_audio_regions(&snapshot, &self.sessions);
        let Ok(timeline) = timeline_from_project_with_session(project, &audio_sessions) else {
            return false;
        };
        let sample_rate = self
            .config()
            .map_or(snapshot.sample_rate(), |config| config.sample_rate);
        let Some(block_frames) = self.config().map(|config| config.block_frames) else {
            return false;
        };
        let Ok(devices) = playback_racks(
            &snapshot,
            sample_rate as f32,
            block_frames,
            self.plugin_worker.as_deref(),
            &self.plugin_roots,
        ) else {
            return false;
        };
        let device_latencies: Vec<u32> = devices
            .iter()
            .take(snapshot.tracks().len())
            .map(DeviceRack::latency_frames)
            .collect();
        let Ok((routing, output_node)) =
            playback_routing_with_device_latencies(&snapshot, &device_latencies)
        else {
            return false;
        };
        let automation = automation_from_snapshot(&snapshot);
        let playing = self.settings.is_playing();
        (self.settings, self.score) =
            state_from_snapshot_with_session(&snapshot, playing, &self.sessions);
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
                .publish_prepared_routing(&routing, output_node, devices)
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

    /// Launches one Session clip at the next quantization boundary.
    pub fn launch_clip(
        &mut self,
        project: &Project,
        track: usize,
        scene: usize,
        quantization_beats: f64,
    ) -> bool {
        if !self.is_open()
            || track >= MAX_TRACKS
            || !quantization_beats.is_finite()
            || quantization_beats < 0.0
        {
            return false;
        }
        let snapshot = project.snapshot();
        if snapshot.clip_at(track, scene).is_none()
            && snapshot.audio_clip_at(track, scene).is_none()
        {
            return false;
        }
        let launch_beats = self.next_launch_beat(quantization_beats);
        let mut sessions = self.sessions;
        sessions[track] = Some(SessionSelection {
            scene,
            launch_beats,
        });
        self.publish_session_state(project, &snapshot, sessions, true)
    }

    /// Launches every occupied slot in one scene as one quantized action.
    pub fn launch_scene(
        &mut self,
        project: &Project,
        scene: usize,
        quantization_beats: f64,
    ) -> bool {
        if !self.is_open() || !quantization_beats.is_finite() || quantization_beats < 0.0 {
            return false;
        }
        let snapshot = project.snapshot();
        if scene >= snapshot.scenes().len() {
            return false;
        }
        let launch_beats = self.next_launch_beat(quantization_beats);
        let mut sessions = self.sessions;
        let mut found = false;
        for (track, session) in sessions
            .iter_mut()
            .enumerate()
            .take(snapshot.tracks().len())
        {
            if snapshot.clip_at(track, scene).is_some()
                || snapshot.audio_clip_at(track, scene).is_some()
            {
                *session = Some(SessionSelection {
                    scene,
                    launch_beats,
                });
                found = true;
            }
        }
        found && self.publish_session_state(project, &snapshot, sessions, true)
    }

    /// Stops a launched clip and restores Arrangement playback on the track.
    pub fn stop_session_track(&mut self, project: &Project, track: usize) -> bool {
        if !self.is_open() || track >= MAX_TRACKS || self.sessions[track].is_none() {
            return false;
        }
        let snapshot = project.snapshot();
        let mut sessions = self.sessions;
        sessions[track] = None;
        let playing = self.settings.is_playing();
        self.publish_session_state(project, &snapshot, sessions, playing)
    }

    /// Active scene index on one track.
    #[must_use]
    pub fn active_session_scene(&self, track: usize) -> Option<usize> {
        self.sessions
            .get(track)
            .and_then(|session| session.map(|value| value.scene))
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

    fn next_launch_beat(&mut self, quantization_beats: f64) -> f64 {
        let position = self.state().position_beats.max(0.0);
        if !self.settings.is_playing() {
            return position;
        }
        let lead = self.config().map_or(0.0, |config| {
            config.block_frames as f64 / f64::from(config.sample_rate) * self.settings.tempo()
                / 60.0
        });
        let earliest = position + lead;
        if quantization_beats == 0.0 {
            earliest
        } else {
            (earliest / quantization_beats).ceil() * quantization_beats
        }
    }

    fn publish_session_state(
        &mut self,
        project: &Project,
        snapshot: &Snapshot,
        sessions: [Option<SessionSelection>; MAX_TRACKS],
        playing: bool,
    ) -> bool {
        let audio_sessions = session_audio_regions(snapshot, &sessions);
        let Ok(timeline) = timeline_from_project_with_session(project, &audio_sessions) else {
            return false;
        };
        let (settings, score) = state_from_snapshot_with_session(snapshot, playing, &sessions);
        let audio_sent = self
            .publisher
            .as_mut()
            .is_some_and(|publisher| publisher.publish_audio(timeline));
        if !audio_sent {
            return false;
        }
        self.sessions = sessions;
        self.settings = settings;
        self.score = score;
        self.dirty_settings = true;
        self.dirty_score = true;
        self.flush()
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
    let device_latencies = snapshot
        .tracks()
        .iter()
        .map(|track| {
            track.devices().iter().try_fold(0_u32, |total, device| {
                total
                    .checked_add(
                        device
                            .latency_frames(sample_rate)
                            .map_err(|_| RoutingError::LatencyOverflow)?,
                    )
                    .ok_or(RoutingError::LatencyOverflow)
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    playback_routing_with_device_latencies(snapshot, &device_latencies)
}

fn playback_routing_with_device_latencies(
    snapshot: &Snapshot,
    device_latencies: &[u32],
) -> Result<(CompiledRouting, u16), RoutingError> {
    let track_count = snapshot.tracks().len();
    if device_latencies.len() != track_count {
        return Err(RoutingError::InvalidNode);
    }
    let output_node = u16::try_from(track_count).map_err(|_| RoutingError::NodeCapacity)?;
    let mut graph = RoutingGraph::new(track_count + 1)?;
    for (index, track) in snapshot.tracks().iter().enumerate() {
        let latency = track
            .latency_frames()
            .checked_add(device_latencies[index])
            .ok_or(RoutingError::LatencyOverflow)?;
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

fn playback_racks(
    snapshot: &Snapshot,
    sample_rate: f32,
    block_frames: usize,
    worker: Option<&Path>,
    roots: &[PathBuf],
) -> Result<Vec<DeviceRack>, AudioError> {
    let has_plugins = snapshot.tracks().iter().any(|track| {
        track
            .devices()
            .iter()
            .any(|device| device.enabled() && device.plugin().is_some())
    });
    let catalog = has_plugins.then(|| PluginCatalog::scan(roots));
    let mut racks = Vec::with_capacity(snapshot.tracks().len() + 1);
    for track in snapshot.tracks() {
        let mut rack = DeviceRack::new(block_frames)
            .map_err(|_| AudioError::Host("device rack could not be prepared"))?;
        let mut native = Vec::new();
        for device in track.devices() {
            if let Some(config) = device.native_config() {
                native.push(config);
                continue;
            }
            if !native.is_empty() {
                rack.push_native(&native, sample_rate)
                    .map_err(|_| AudioError::Host("native device could not be prepared"))?;
                native.clear();
            }
            if !device.enabled() {
                continue;
            }
            let plugin = device
                .plugin()
                .ok_or(AudioError::Host("plugin device is invalid"))?;
            if plugin.format() != PluginFormat::Clap {
                return Err(AudioError::Host(
                    "plugin format is not available for playback",
                ));
            }
            let worker = worker.ok_or(AudioError::Host("plugin worker is not configured"))?;
            let catalog = catalog
                .as_ref()
                .ok_or(AudioError::Host("plugin catalog is not available"))?;
            let mut matches = catalog.entries().iter().filter(|entry| {
                entry.state() == PluginState::Discovered
                    && entry.format() == plugin.format()
                    && entry.path().file_name().and_then(|name| name.to_str())
                        == Some(plugin.package())
            });
            let package = matches
                .next()
                .ok_or(AudioError::Host("plugin package could not be resolved"))?;
            if matches.next().is_some() {
                return Err(AudioError::Host("plugin package name is ambiguous"));
            }
            let mut client = PluginClient::spawn(
                worker,
                package.path(),
                plugin.identifier(),
                f64::from(sample_rate),
                block_frames,
            )
            .map_err(|_| AudioError::Host("plugin worker could not be opened"))?;
            if !plugin.state().is_empty() {
                client
                    .load_state(plugin.state())
                    .map_err(|_| AudioError::Host("plugin state could not be restored"))?;
            }
            let bridge = Bridge::from_client(client, block_frames, 3)
                .map_err(|_| AudioError::Host("plugin bridge could not be prepared"))?;
            let parameter_events: Vec<_> = plugin
                .parameters()
                .iter()
                .map(|parameter| ParameterEvent {
                    sample_offset: 0,
                    identifier: parameter.identifier,
                    value: parameter.value,
                })
                .collect();
            rack.push_plugin_with_parameters(bridge, &parameter_events)
                .map_err(|_| AudioError::Host("plugin rack could not be prepared"))?;
        }
        if !native.is_empty() {
            rack.push_native(&native, sample_rate)
                .map_err(|_| AudioError::Host("native device could not be prepared"))?;
        }
        racks.push(rack);
    }
    racks.push(
        DeviceRack::new(block_frames)
            .map_err(|_| AudioError::Host("master rack could not be prepared"))?,
    );
    Ok(racks)
}

pub(crate) fn playback_devices(snapshot: &Snapshot) -> Vec<Vec<DeviceConfig>> {
    let mut result: Vec<Vec<DeviceConfig>> = snapshot
        .tracks()
        .iter()
        .map(|track| {
            track
                .devices()
                .iter()
                .filter_map(|device| device.native_config())
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
    state_from_snapshot_with_session(snapshot, playing, &[None; MAX_TRACKS])
}

fn state_from_snapshot_with_session(
    snapshot: &Snapshot,
    playing: bool,
    sessions: &[Option<SessionSelection>; MAX_TRACKS],
) -> (MixSettings, Box<Score>) {
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
        destination.set_patch(track.instrument_patch());
        if let Some(session) = sessions[index] {
            let Some(clip) = snapshot.clip_at(index, session.scene) else {
                continue;
            };
            let notes: Vec<ScheduledNote> = clip
                .notes()
                .iter()
                .map(|note| ScheduledNote {
                    start_beats: note.start_beats,
                    length_beats: note.length_beats,
                    pitch: note.pitch,
                    velocity: note.velocity,
                })
                .collect();
            let (loop_start, loop_length) = clip.loop_range();
            destination.set_session_notes(&notes, loop_start, loop_length, session.launch_beats);
            continue;
        }
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

fn session_audio_regions(
    snapshot: &Snapshot,
    sessions: &[Option<SessionSelection>; MAX_TRACKS],
) -> Vec<SessionAudioRegion> {
    sessions
        .iter()
        .enumerate()
        .filter_map(|(track, active)| {
            let active = (*active)?;
            snapshot
                .audio_clip_at(track, active.scene)
                .map(|_| SessionAudioRegion {
                    track,
                    scene: active.scene,
                    launch_beats: active.launch_beats,
                })
        })
        .collect()
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
    use crate::engine::voice::Patch;
    use crate::project::{AutomationPoint, Command, MidiNote, PluginDevice};

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
        let mut project = midi_project();
        let track = project.snapshot().tracks()[1].id();
        let patch = Patch {
            oscillator_mix: 0.75,
            unison_voices: 3,
            cutoff: 1_800.0,
            ..Patch::default()
        };
        project
            .apply(&[Command::SetInstrumentPatch { id: track, patch }])
            .unwrap();
        let (settings, score) = state_from_snapshot(&project.snapshot(), true);
        assert_eq!(settings.track_count(), 2);
        assert_eq!(settings.tempo(), 96.0);
        assert!(settings.is_playing());
        assert_eq!(settings.track(1).volume_db, -6.0);
        assert!(score.track(1).unwrap().is_enabled());
        assert_eq!(score.track(1).unwrap().patch(), patch);
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
    fn enabled_plugins_require_a_configured_worker() {
        let mut project = Project::new();
        project
            .apply(&[Command::CreateTrack {
                name: "Audio".into(),
                kind: TrackKind::Audio,
            }])
            .unwrap();
        let track = project.snapshot().tracks()[0].id();
        let plugin = PluginDevice::new(
            PluginFormat::Clap,
            "Effect.clap",
            "app.nylon.effect",
            0,
            Vec::new(),
        )
        .unwrap();
        project
            .apply(&[Command::AddPluginDevice {
                track,
                enabled: true,
                plugin,
            }])
            .unwrap();
        assert!(playback_racks(&project.snapshot(), 48_000.0, 256, None, &[]).is_err());
    }

    #[test]
    fn bypassed_plugins_do_not_require_a_worker_or_add_latency() {
        let mut project = Project::new();
        project
            .apply(&[Command::CreateTrack {
                name: "Audio".into(),
                kind: TrackKind::Audio,
            }])
            .unwrap();
        let track = project.snapshot().tracks()[0].id();
        let plugin = PluginDevice::new(
            PluginFormat::Clap,
            "Effect.clap",
            "app.nylon.effect",
            900,
            Vec::new(),
        )
        .unwrap();
        project
            .apply(&[Command::AddPluginDevice {
                track,
                enabled: false,
                plugin,
            }])
            .unwrap();
        let racks = playback_racks(&project.snapshot(), 48_000.0, 256, None, &[]).unwrap();
        assert!(racks[0].is_empty());
        assert_eq!(racks[0].latency_frames(), 0);
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
    fn a_session_clip_replaces_arrangement_notes_on_its_track() {
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
                        pitch: 72,
                        velocity: 90,
                        start_beats: 0.25,
                        length_beats: 0.5,
                    },
                },
                Command::PlaceClip {
                    track,
                    clip,
                    start_beats: 8.0,
                    length_beats: 2.0,
                },
            ])
            .unwrap();
        let mut sessions = [None; MAX_TRACKS];
        sessions[1] = Some(SessionSelection {
            scene: 0,
            launch_beats: 4.0,
        });
        let (_, score) = state_from_snapshot_with_session(&project.snapshot(), true, &sessions);
        let part = score.track(1).unwrap();
        assert_eq!(part.notes().len(), 1);
        assert_eq!(part.notes()[0].start_beats, 0.25);
        assert_eq!(
            part.session_loop(),
            Some(crate::engine::playback::SessionLoop {
                launch_beats: 4.0,
                loop_start_beats: 0.0,
                loop_length_beats: 2.0,
            })
        );
    }

    #[test]
    fn closed_runtime_rejects_transport_commands() {
        let mut runtime = AudioRuntime::new();
        assert!(!runtime.is_open());
        assert!(!runtime.play());
        assert!(!runtime.stop());
        assert!(!runtime.locate(2.0));
        assert!(!runtime.launch_clip(&Project::new(), 0, 0, 1.0));
        assert!(!runtime.launch_scene(&Project::new(), 0, 1.0));
        assert!(!runtime.stop_session_track(&Project::new(), 0));
        assert_eq!(runtime.active_session_scene(0), None);
        assert_eq!(runtime.state(), PlaybackState::default());
        assert_eq!(runtime.dropouts(), 0);
        assert_eq!(runtime.frames_rendered(), 0);
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
