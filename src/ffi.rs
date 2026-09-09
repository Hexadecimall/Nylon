//! Native control interface. Handles belong to one control thread and must
//! not be accessed concurrently or used after release.

use crate::audio::{DeviceId, DeviceInfo, Direction, Name, Rates};
use crate::bounce::{Options as BounceOptions, render_wave};
use crate::dsp::auto_filter::{Mode as AutoFilterMode, Parameters as AutoFilterParameters};
use crate::dsp::biquad::Kind as FilterKind;
use crate::dsp::chorus::Parameters as ChorusParameters;
use crate::dsp::compressor::Parameters as CompressorParameters;
use crate::dsp::env::Settings as EnvelopeSettings;
use crate::dsp::gate::Parameters as GateParameters;
use crate::dsp::limiter::Parameters as LimiterParameters;
use crate::dsp::osc::Shape;
use crate::dsp::phaser::Parameters as PhaserParameters;
use crate::dsp::reverb::Parameters as ReverbParameters;
use crate::dsp::saturator::{
    Curve as SaturatorCurve, Oversampling as SaturatorOversampling,
    Parameters as SaturatorParameters,
};
use crate::engine::device::DeviceKind as TrackDeviceKind;
use crate::engine::voice::Patch;
use crate::media::import_wave;
use crate::mixer::Levels;
use crate::plugin::Catalog as PluginCatalog;
use crate::plugin::bridge::Bridge as ClapBridge;
use crate::plugin::clap::{
    Instance as ClapInstance, NoteEvent as ClapNoteEvent, ParameterEvent as ClapParameterEvent,
};
use crate::plugin::worker::Client as ClapWorker;
use crate::project::{
    AutomationCurve, AutomationParameter, AutomationPoint, ClipId, Command, MidiNote, Project,
    SceneId, TrackId, TrackKind,
};
use crate::routing::{CompiledRouting, Edge, EdgeKind, RoutingGraph};
#[cfg(platform_audio)]
use crate::runtime::ProjectRecording;
use crate::runtime::{
    AudioRuntime, MAX_DEVICES, default_input, default_output, input_devices, output_devices,
};
use crate::wave::Format;
use std::ffi::{CStr, c_char};
use std::fs::File;
use std::path::PathBuf;

/// Fixed-size audio-device record for the C interface.
#[repr(C)]
pub struct NylonAudioDevice {
    pub id: u64,
    pub name: [c_char; crate::audio::MAX_NAME + 1],
    pub channels: u32,
    pub is_default: i32,
    pub sample_rates: [u32; crate::audio::MAX_RATES],
    pub sample_rate_count: u32,
}

/// Stream configuration granted by the host.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NylonAudioConfig {
    pub device_id: u64,
    pub sample_rate: u32,
    pub block_frames: u32,
    pub channels: u32,
}

/// One stereo meter reading from the live mixer.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NylonLevels {
    pub peak_left: f32,
    pub peak_right: f32,
    pub rms_left: f32,
    pub rms_right: f32,
    pub clipped: i32,
}

/// Measurements from a completed offline render.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NylonBounceReport {
    pub frames: u64,
    pub peak_left: f32,
    pub peak_right: f32,
}

/// Measurements from a completed input recording.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NylonRecordingReport {
    pub frames: u64,
    pub sample_rate: u32,
    pub length_beats: f64,
    pub lost_blocks: u64,
    pub lost_frames: u64,
}

/// Fixed-size native device description. Parameter meanings depend on kind.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NylonTrackDevice {
    pub kind: i32,
    pub enabled: i32,
    pub parameters: [f32; 16],
}

/// Fixed-size CLAP parameter description for native clients.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NylonClapParameterInfo {
    pub identifier: u32,
    pub flags: u32,
    pub name: [c_char; 256],
    pub module: [c_char; 1024],
    pub minimum: f64,
    pub maximum: f64,
    pub default_value: f64,
}

impl Default for NylonClapParameterInfo {
    fn default() -> Self {
        Self {
            identifier: 0,
            flags: 0,
            name: [0; 256],
            module: [0; 1024],
            minimum: 0.0,
            maximum: 0.0,
            default_value: 0.0,
        }
    }
}

/// One point in a native automation lane. Curve is 0 step, 1 linear, or 2 smooth.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NylonAutomationPoint {
    pub beat: f64,
    pub value: f32,
    pub curve: i32,
}

/// Fixed-size subtractive instrument patch for a MIDI track.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NylonInstrumentPatch {
    pub shape_a: i32,
    pub shape_b: i32,
    pub oscillator_mix: f32,
    pub oscillator_b_detune_cents: f32,
    pub sub_level: f32,
    pub noise_level: f32,
    pub unison_voices: u32,
    pub unison_detune_cents: f32,
    pub attack_seconds: f32,
    pub decay_seconds: f32,
    pub sustain: f32,
    pub release_seconds: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub level_db: f32,
}

impl From<Levels> for NylonLevels {
    fn from(levels: Levels) -> Self {
        Self {
            peak_left: levels.peak_left,
            peak_right: levels.peak_right,
            rms_left: levels.rms_left,
            rms_right: levels.rms_right,
            clipped: i32::from(levels.clipped),
        }
    }
}

impl From<crate::bounce::Report> for NylonBounceReport {
    fn from(report: crate::bounce::Report) -> Self {
        Self {
            frames: report.frames,
            peak_left: report.peak_left,
            peak_right: report.peak_right,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn nylon_project_new() -> *mut Project {
    Box::into_raw(Box::new(Project::new()))
}

/// # Safety
/// A non-null handle must originate from `nylon_project_new`, remain live,
/// and have no outstanding references. Release it exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_free(handle: *mut Project) {
    if !handle.is_null() {
        // SAFETY: Ownership of the original allocation is transferred back.
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// # Safety
/// A non-null handle must refer to a live project with no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_tempo(handle: *const Project) -> f64 {
    // SAFETY: Validity and access exclusion are required by the interface.
    unsafe { handle.as_ref() }.map_or(0.0, |project| project.snapshot().tempo())
}

/// # Safety
/// A non-null handle must refer to a live project exclusively owned by this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_set_tempo(handle: *mut Project, tempo: f64) -> i32 {
    // SAFETY: Validity and exclusive access are required by the interface.
    unsafe { handle.as_mut() }.map_or(0, |project| {
        i32::from(project.apply(&[Command::SetTempo(tempo)]).is_ok())
    })
}

/// # Safety
/// A non-null handle must refer to a live project exclusively owned by this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_add_track(handle: *mut Project) -> i32 {
    // SAFETY: Validity and exclusive access are required by the interface.
    unsafe { handle.as_mut() }.map_or(0, |project| {
        let name = format!("Audio {}", project.snapshot().tracks().len() + 1);
        i32::from(
            project
                .apply(&[Command::CreateTrack {
                    name,
                    kind: TrackKind::Audio,
                }])
                .is_ok(),
        )
    })
}

/// # Safety
/// A non-null handle must refer to a live project with no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_track_count(handle: *const Project) -> u64 {
    // SAFETY: Validity and access exclusion are required by the interface.
    unsafe { handle.as_ref() }.map_or(0, |project| project.snapshot().tracks().len() as u64)
}

/// # Safety
/// A non-null handle must refer to a live project exclusively owned by this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_undo(handle: *mut Project) -> i32 {
    // SAFETY: Validity and exclusive access are required by the interface.
    unsafe { handle.as_mut() }.map_or(0, |project| i32::from(project.undo()))
}

/// # Safety
/// A non-null handle must refer to a live project exclusively owned by this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_redo(handle: *mut Project) -> i32 {
    // SAFETY: Validity and exclusive access are required by the interface.
    unsafe { handle.as_mut() }.map_or(0, |project| i32::from(project.redo()))
}

macro_rules! track_getter {
    ($name:ident, $result:ty, $fallback:expr, $method:ident) => {
        /// # Safety
        /// A non-null handle must be live and have no concurrent mutation.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(handle: *const Project, index: u64) -> $result {
            // SAFETY: Handle validity is required by the native interface.
            let Some(project) = (unsafe { handle.as_ref() }) else {
                return $fallback;
            };
            let Ok(index) = usize::try_from(index) else {
                return $fallback;
            };
            project
                .current
                .tracks
                .get(index)
                .map_or($fallback, |track| track.$method().into())
        }
    };
}

track_getter!(nylon_track_volume_db, f64, f64::NEG_INFINITY, volume_db);
track_getter!(nylon_track_pan, f64, 0.0, pan);
track_getter!(nylon_track_mute, i32, 0, muted);
track_getter!(nylon_track_solo, i32, 0, solo);
track_getter!(nylon_track_arm, i32, 0, armed);
track_getter!(nylon_track_color_index, i32, -1, color_index);

/// # Safety
/// A non-null handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_latency_frames(handle: *const Project, index: u64) -> u32 {
    // SAFETY: Handle validity and access exclusion are required by the interface.
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    // SAFETY: Handle validity and access exclusion are required by the interface.
    unsafe { handle.as_ref() }
        .and_then(|project| project.current.tracks.get(index))
        .map_or(0, |track| track.latency_frames())
}

fn oscillator_shape(code: i32) -> Option<Shape> {
    match code {
        0 => Some(Shape::Sine),
        1 => Some(Shape::Saw),
        2 => Some(Shape::Square),
        3 => Some(Shape::Triangle),
        _ => None,
    }
}

fn oscillator_shape_code(shape: Shape) -> i32 {
    match shape {
        Shape::Sine => 0,
        Shape::Saw => 1,
        Shape::Square => 2,
        Shape::Triangle => 3,
    }
}

fn instrument_patch(record: NylonInstrumentPatch) -> Option<Patch> {
    Some(Patch {
        shape: oscillator_shape(record.shape_a)?,
        shape_b: oscillator_shape(record.shape_b)?,
        oscillator_mix: record.oscillator_mix,
        oscillator_b_detune_cents: record.oscillator_b_detune_cents,
        sub_level: record.sub_level,
        noise_level: record.noise_level,
        unison_voices: u8::try_from(record.unison_voices).ok()?,
        unison_detune_cents: record.unison_detune_cents,
        envelope: EnvelopeSettings {
            attack: record.attack_seconds,
            decay: record.decay_seconds,
            sustain: record.sustain,
            release: record.release_seconds,
        },
        cutoff: record.cutoff_hz,
        resonance: record.resonance,
        level_db: record.level_db,
    })
}

fn native_instrument_patch(patch: Patch) -> NylonInstrumentPatch {
    NylonInstrumentPatch {
        shape_a: oscillator_shape_code(patch.shape),
        shape_b: oscillator_shape_code(patch.shape_b),
        oscillator_mix: patch.oscillator_mix,
        oscillator_b_detune_cents: patch.oscillator_b_detune_cents,
        sub_level: patch.sub_level,
        noise_level: patch.noise_level,
        unison_voices: u32::from(patch.unison_voices),
        unison_detune_cents: patch.unison_detune_cents,
        attack_seconds: patch.envelope.attack,
        decay_seconds: patch.envelope.decay,
        sustain: patch.envelope.sustain,
        release_seconds: patch.envelope.release,
        cutoff_hz: patch.cutoff,
        resonance: patch.resonance,
        level_db: patch.level_db,
    }
}

/// # Safety
/// The handle must be live and `out` must reference writable storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_instrument_get(
    handle: *const Project,
    track: u64,
    out: *mut NylonInstrumentPatch,
) -> i32 {
    if out.is_null() {
        return 0;
    }
    let Ok(track) = usize::try_from(track) else {
        return 0;
    };
    // SAFETY: Handle validity is required by the native interface.
    let Some(patch) = (unsafe { handle.as_ref() })
        .and_then(|project| project.current.tracks.get(track))
        .filter(|track| track.kind() == TrackKind::Midi)
        .map(|track| track.instrument_patch())
    else {
        return 0;
    };
    // SAFETY: The caller supplies writable storage for one record.
    unsafe { out.write(native_instrument_patch(patch)) };
    1
}

/// # Safety
/// The handle must be live and exclusive; `patch` must reference readable storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_instrument_set(
    handle: *mut Project,
    track: u64,
    patch: *const NylonInstrumentPatch,
) -> i32 {
    if patch.is_null() {
        return 0;
    }
    // SAFETY: The caller supplies readable storage for one record.
    let Some(patch) = instrument_patch(unsafe { patch.read() }) else {
        return 0;
    };
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_track(handle, track, |id| {
            Some(Command::SetInstrumentPatch { id, patch })
        })
    }
}

unsafe fn edit_track(
    handle: *mut Project,
    index: u64,
    make: impl FnOnce(TrackId) -> Option<Command>,
) -> i32 {
    // SAFETY: The caller transfers the native interface's exclusive access contract.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    let Some(track) = project.current.tracks.get(index) else {
        return 0;
    };
    let Some(command) = make(track.id()) else {
        return 0;
    };
    i32::from(project.apply(&[command]).is_ok())
}

macro_rules! track_setter {
    ($name:ident, $value:ty, $make:expr) => {
        /// # Safety
        /// A non-null handle must be live and exclusively accessible to this call.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(handle: *mut Project, index: u64, value: $value) -> i32 {
            // SAFETY: Handle validity and exclusivity are required by the interface.
            unsafe { edit_track(handle, index, |id| ($make)(id, value)) }
        }
    };
}

track_setter!(nylon_track_set_volume_db, f64, |id, db| Some(
    Command::SetTrackVolume { id, db }
));
track_setter!(nylon_track_set_pan, f64, |id, pan| Some(
    Command::SetTrackPan { id, pan }
));
track_setter!(nylon_track_set_mute, i32, |id, value| match value {
    0 | 1 => Some(Command::SetTrackMute {
        id,
        enabled: value == 1
    }),
    _ => None,
});
track_setter!(nylon_track_set_solo, i32, |id, value| match value {
    0 | 1 => Some(Command::SetTrackSolo {
        id,
        enabled: value == 1
    }),
    _ => None,
});
track_setter!(nylon_track_set_arm, i32, |id, value| match value {
    0 | 1 => Some(Command::SetTrackArm {
        id,
        enabled: value == 1
    }),
    _ => None,
});
track_setter!(nylon_track_set_color_index, i32, |id, value| u8::try_from(
    value
)
.ok()
.map(|index| Command::SetTrackColor { id, index }));
track_setter!(nylon_track_set_latency_frames, u32, |id, frames| Some(
    Command::SetTrackLatency { id, frames }
));

fn automation_parameter(code: i32) -> Option<AutomationParameter> {
    match code {
        0 => Some(AutomationParameter::Volume),
        1 => Some(AutomationParameter::Pan),
        2 => Some(AutomationParameter::Mute),
        3 => Some(AutomationParameter::Solo),
        _ => None,
    }
}

fn automation_curve(code: i32) -> Option<AutomationCurve> {
    match code {
        0 => Some(AutomationCurve::Step),
        1 => Some(AutomationCurve::Linear),
        2 => Some(AutomationCurve::Smooth),
        _ => None,
    }
}

/// # Safety
/// A non-null handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_automation_count(
    handle: *const Project,
    track: u64,
    parameter: i32,
) -> u64 {
    let (Ok(track), Some(parameter)) = (usize::try_from(track), automation_parameter(parameter))
    else {
        return 0;
    };
    // SAFETY: Handle validity and access exclusion are required by the interface.
    unsafe { handle.as_ref() }
        .and_then(|project| project.current.tracks.get(track))
        .and_then(|track| {
            track
                .automation()
                .iter()
                .find(|lane| lane.parameter() == parameter)
        })
        .map_or(0, |lane| lane.points().len() as u64)
}

/// # Safety
/// A non-null handle must be live. `out` must point to one writable record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_automation_get(
    handle: *const Project,
    track: u64,
    parameter: i32,
    index: u64,
    out: *mut NylonAutomationPoint,
) -> i32 {
    if out.is_null() {
        return 0;
    }
    let (Ok(track), Ok(index), Some(parameter)) = (
        usize::try_from(track),
        usize::try_from(index),
        automation_parameter(parameter),
    ) else {
        return 0;
    };
    // SAFETY: Handle validity and access exclusion are required by the interface.
    let point = unsafe { handle.as_ref() }
        .and_then(|project| project.current.tracks.get(track))
        .and_then(|track| {
            track
                .automation()
                .iter()
                .find(|lane| lane.parameter() == parameter)
        })
        .and_then(|lane| lane.points().get(index));
    let Some(point) = point else {
        return 0;
    };
    // SAFETY: The caller supplies one writable record.
    unsafe {
        out.write(NylonAutomationPoint {
            beat: point.beat,
            value: point.value,
            curve: point.curve.code().into(),
        });
    }
    1
}

/// # Safety
/// The project must be exclusively accessible. `points` must hold `count`
/// readable records when count is nonzero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_automation_set(
    handle: *mut Project,
    track: u64,
    parameter: i32,
    points: *const NylonAutomationPoint,
    count: u64,
) -> i32 {
    let (Some(parameter), Ok(count)) = (automation_parameter(parameter), usize::try_from(count))
    else {
        return 0;
    };
    if points.is_null() || count == 0 || count > crate::project::MAX_AUTOMATION_POINTS {
        return 0;
    }
    // SAFETY: The native contract requires this many readable records.
    let records = unsafe { std::slice::from_raw_parts(points, count) };
    let mut converted = Vec::with_capacity(count);
    for point in records {
        let Some(curve) = automation_curve(point.curve) else {
            return 0;
        };
        converted.push(AutomationPoint {
            beat: point.beat,
            value: point.value,
            curve,
        });
    }
    // SAFETY: This call retains the native interface's exclusive access contract.
    unsafe {
        edit_track(handle, track, |track| {
            Some(Command::SetAutomation {
                track,
                parameter,
                points: converted,
            })
        })
    }
}

/// # Safety
/// The project must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_automation_clear(
    handle: *mut Project,
    track: u64,
    parameter: i32,
) -> i32 {
    let Some(parameter) = automation_parameter(parameter) else {
        return 0;
    };
    // SAFETY: This call retains the native interface's exclusive access contract.
    unsafe {
        edit_track(handle, track, |track| {
            Some(Command::ClearAutomation { track, parameter })
        })
    }
}

fn track_device_kind(record: NylonTrackDevice) -> Option<TrackDeviceKind> {
    let parameters = record.parameters;
    match record.kind {
        0 => Some(TrackDeviceKind::Utility {
            gain_db: parameters[0],
            width: parameters[1],
            balance: parameters[2],
        }),
        1 => {
            let filter = match parameters[0] {
                0.0 => FilterKind::LowPass,
                1.0 => FilterKind::HighPass,
                2.0 => FilterKind::BandPass,
                3.0 => FilterKind::Notch,
                4.0 => FilterKind::AllPass,
                5.0 => FilterKind::Peaking,
                6.0 => FilterKind::LowShelf,
                7.0 => FilterKind::HighShelf,
                _ => return None,
            };
            Some(TrackDeviceKind::Equalizer {
                kind: filter,
                frequency: parameters[1],
                q: parameters[2],
                gain_db: parameters[3],
            })
        }
        2 => Some(TrackDeviceKind::Compressor {
            parameters: CompressorParameters {
                threshold_db: parameters[0],
                ratio: parameters[1],
                knee_db: parameters[2],
                attack_seconds: parameters[3],
                release_seconds: parameters[4],
                makeup_db: parameters[5],
            },
            external_sidechain: match parameters[6] {
                0.0 => false,
                1.0 => true,
                _ => return None,
            },
        }),
        3 => Some(TrackDeviceKind::StereoDelay {
            delay_seconds: parameters[0],
            feedback: parameters[1],
            mix: parameters[2],
        }),
        4 => Some(TrackDeviceKind::Limiter {
            parameters: LimiterParameters {
                ceiling_db: parameters[0],
                release_seconds: parameters[1],
                lookahead_seconds: parameters[2],
            },
        }),
        5 => Some(TrackDeviceKind::Saturator {
            parameters: SaturatorParameters {
                drive_db: parameters[0],
                output_db: parameters[1],
                mix: parameters[2],
                curve: match parameters[3] {
                    0.0 => SaturatorCurve::SoftClip,
                    1.0 => SaturatorCurve::Tanh,
                    2.0 => SaturatorCurve::HardClip,
                    3.0 => SaturatorCurve::Diode,
                    _ => return None,
                },
                oversampling: match parameters[4] {
                    0.0 => SaturatorOversampling::One,
                    1.0 => SaturatorOversampling::Two,
                    2.0 => SaturatorOversampling::Four,
                    _ => return None,
                },
                dc_filter: match parameters[5] {
                    0.0 => false,
                    1.0 => true,
                    _ => return None,
                },
            },
        }),
        6 => Some(TrackDeviceKind::Gate {
            parameters: GateParameters {
                threshold_db: parameters[0],
                hysteresis_db: parameters[1],
                attack_seconds: parameters[2],
                hold_seconds: parameters[3],
                release_seconds: parameters[4],
                external_sidechain: match parameters[5] {
                    0.0 => false,
                    1.0 => true,
                    _ => return None,
                },
            },
        }),
        7 => Some(TrackDeviceKind::Chorus {
            parameters: ChorusParameters {
                rate_hz: parameters[0],
                center_seconds: parameters[1],
                depth_seconds: parameters[2],
                feedback: parameters[3],
                mix: parameters[4],
                stereo_phase: parameters[5],
            },
        }),
        8 => Some(TrackDeviceKind::Reverb {
            parameters: ReverbParameters {
                size: parameters[0],
                decay_seconds: parameters[1],
                damping: parameters[2],
                diffusion: parameters[3],
                pre_delay_seconds: parameters[4],
                width: parameters[5],
                mix: parameters[6],
            },
        }),
        9 => Some(TrackDeviceKind::AutoFilter {
            parameters: AutoFilterParameters {
                mode: match parameters[0] {
                    0.0 => AutoFilterMode::LowPass,
                    1.0 => AutoFilterMode::HighPass,
                    2.0 => AutoFilterMode::BandPass,
                    3.0 => AutoFilterMode::Notch,
                    _ => return None,
                },
                cutoff_hz: parameters[1],
                resonance: parameters[2],
                drive_db: parameters[3],
                envelope_amount_octaves: parameters[4],
                envelope_attack_seconds: parameters[5],
                envelope_release_seconds: parameters[6],
                lfo_rate_hz: parameters[7],
                lfo_amount_octaves: parameters[8],
                mix: parameters[9],
            },
            external_sidechain: match parameters[10] {
                0.0 => false,
                1.0 => true,
                _ => return None,
            },
        }),
        10 => {
            let stages = parameters[6];
            if !stages.is_finite() || stages.fract() != 0.0 || !(0.0..=255.0).contains(&stages) {
                return None;
            }
            Some(TrackDeviceKind::Phaser {
                parameters: PhaserParameters {
                    rate_hz: parameters[0],
                    center_hz: parameters[1],
                    depth_octaves: parameters[2],
                    feedback: parameters[3],
                    mix: parameters[4],
                    stereo_phase: parameters[5],
                    stages: stages as u8,
                },
            })
        }
        _ => None,
    }
}

fn native_track_device(kind: TrackDeviceKind, enabled: bool) -> NylonTrackDevice {
    let mut record = NylonTrackDevice {
        enabled: i32::from(enabled),
        ..NylonTrackDevice::default()
    };
    match kind {
        TrackDeviceKind::Utility {
            gain_db,
            width,
            balance,
        } => {
            record.kind = 0;
            record.parameters[..3].copy_from_slice(&[gain_db, width, balance]);
        }
        TrackDeviceKind::Equalizer {
            kind,
            frequency,
            q,
            gain_db,
        } => {
            record.kind = 1;
            record.parameters[..4].copy_from_slice(&[
                match kind {
                    FilterKind::LowPass => 0.0,
                    FilterKind::HighPass => 1.0,
                    FilterKind::BandPass => 2.0,
                    FilterKind::Notch => 3.0,
                    FilterKind::AllPass => 4.0,
                    FilterKind::Peaking => 5.0,
                    FilterKind::LowShelf => 6.0,
                    FilterKind::HighShelf => 7.0,
                },
                frequency,
                q,
                gain_db,
            ]);
        }
        TrackDeviceKind::Compressor {
            parameters,
            external_sidechain,
        } => {
            record.kind = 2;
            record.parameters[..7].copy_from_slice(&[
                parameters.threshold_db,
                parameters.ratio,
                parameters.knee_db,
                parameters.attack_seconds,
                parameters.release_seconds,
                parameters.makeup_db,
                f32::from(u8::from(external_sidechain)),
            ]);
        }
        TrackDeviceKind::StereoDelay {
            delay_seconds,
            feedback,
            mix,
        } => {
            record.kind = 3;
            record.parameters[..3].copy_from_slice(&[delay_seconds, feedback, mix]);
        }
        TrackDeviceKind::Limiter { parameters } => {
            record.kind = 4;
            record.parameters[..3].copy_from_slice(&[
                parameters.ceiling_db,
                parameters.release_seconds,
                parameters.lookahead_seconds,
            ]);
        }
        TrackDeviceKind::Saturator { parameters } => {
            record.kind = 5;
            record.parameters[..6].copy_from_slice(&[
                parameters.drive_db,
                parameters.output_db,
                parameters.mix,
                match parameters.curve {
                    SaturatorCurve::SoftClip => 0.0,
                    SaturatorCurve::Tanh => 1.0,
                    SaturatorCurve::HardClip => 2.0,
                    SaturatorCurve::Diode => 3.0,
                },
                match parameters.oversampling {
                    SaturatorOversampling::One => 0.0,
                    SaturatorOversampling::Two => 1.0,
                    SaturatorOversampling::Four => 2.0,
                },
                f32::from(u8::from(parameters.dc_filter)),
            ]);
        }
        TrackDeviceKind::Gate { parameters } => {
            record.kind = 6;
            record.parameters[..6].copy_from_slice(&[
                parameters.threshold_db,
                parameters.hysteresis_db,
                parameters.attack_seconds,
                parameters.hold_seconds,
                parameters.release_seconds,
                f32::from(u8::from(parameters.external_sidechain)),
            ]);
        }
        TrackDeviceKind::Chorus { parameters } => {
            record.kind = 7;
            record.parameters[..6].copy_from_slice(&[
                parameters.rate_hz,
                parameters.center_seconds,
                parameters.depth_seconds,
                parameters.feedback,
                parameters.mix,
                parameters.stereo_phase,
            ]);
        }
        TrackDeviceKind::Reverb { parameters } => {
            record.kind = 8;
            record.parameters[..7].copy_from_slice(&[
                parameters.size,
                parameters.decay_seconds,
                parameters.damping,
                parameters.diffusion,
                parameters.pre_delay_seconds,
                parameters.width,
                parameters.mix,
            ]);
        }
        TrackDeviceKind::AutoFilter {
            parameters,
            external_sidechain,
        } => {
            record.kind = 9;
            record.parameters[..11].copy_from_slice(&[
                match parameters.mode {
                    AutoFilterMode::LowPass => 0.0,
                    AutoFilterMode::HighPass => 1.0,
                    AutoFilterMode::BandPass => 2.0,
                    AutoFilterMode::Notch => 3.0,
                },
                parameters.cutoff_hz,
                parameters.resonance,
                parameters.drive_db,
                parameters.envelope_amount_octaves,
                parameters.envelope_attack_seconds,
                parameters.envelope_release_seconds,
                parameters.lfo_rate_hz,
                parameters.lfo_amount_octaves,
                parameters.mix,
                f32::from(u8::from(external_sidechain)),
            ]);
        }
        TrackDeviceKind::Phaser { parameters } => {
            record.kind = 10;
            record.parameters[..7].copy_from_slice(&[
                parameters.rate_hz,
                parameters.center_hz,
                parameters.depth_octaves,
                parameters.feedback,
                parameters.mix,
                parameters.stereo_phase,
                f32::from(parameters.stages),
            ]);
        }
    }
    record
}

/// # Safety
/// A non-null handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_device_count(handle: *const Project, track: u64) -> u64 {
    let Ok(track) = usize::try_from(track) else {
        return 0;
    };
    // SAFETY: Handle validity and access exclusion are required by the interface.
    unsafe { handle.as_ref() }
        .and_then(|project| project.current.tracks.get(track))
        .map_or(0, |track| track.devices().len() as u64)
}

/// # Safety
/// A non-null handle must be live. `out` must point to writable storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_device_get(
    handle: *const Project,
    track: u64,
    device: u64,
    out: *mut NylonTrackDevice,
) -> i32 {
    if out.is_null() {
        return 0;
    }
    let (Ok(track), Ok(device)) = (usize::try_from(track), usize::try_from(device)) else {
        return 0;
    };
    // SAFETY: Handle validity and access exclusion are required by the interface.
    let Some(item) = (unsafe { handle.as_ref() })
        .and_then(|project| project.current.tracks.get(track))
        .and_then(|track| track.devices().get(device))
    else {
        return 0;
    };
    // SAFETY: The caller supplies writable storage for one record.
    unsafe { out.write(native_track_device(item.kind(), item.enabled())) };
    1
}

/// # Safety
/// A non-null handle must be exclusively accessible. `device` must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_device_add(
    handle: *mut Project,
    track: u64,
    device: *const NylonTrackDevice,
) -> i32 {
    // SAFETY: The caller supplies a readable record for the duration of this call.
    let Some(record) = (unsafe { device.as_ref() }).copied() else {
        return 0;
    };
    let Some(kind) = track_device_kind(record) else {
        return 0;
    };
    let enabled = match record.enabled {
        0 => false,
        1 => true,
        _ => return 0,
    };
    // SAFETY: Handle validity and exclusivity are required by the interface.
    unsafe {
        edit_track(handle, track, |track| {
            Some(Command::AddDevice {
                track,
                config: crate::engine::device::DeviceConfig { enabled, kind },
            })
        })
    }
}

/// # Safety
/// A non-null handle must be exclusively accessible. `device` must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_device_set(
    handle: *mut Project,
    track: u64,
    index: u64,
    device: *const NylonTrackDevice,
) -> i32 {
    // SAFETY: Handle validity and exclusivity are required by the interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let (Ok(track), Ok(index)) = (usize::try_from(track), usize::try_from(index)) else {
        return 0;
    };
    // SAFETY: The caller supplies a readable record for the duration of this call.
    let Some(record) = (unsafe { device.as_ref() }).copied() else {
        return 0;
    };
    let Some(kind) = track_device_kind(record) else {
        return 0;
    };
    let enabled = match record.enabled {
        0 => false,
        1 => true,
        _ => return 0,
    };
    let Some(id) = project
        .current
        .tracks
        .get(track)
        .and_then(|track| track.devices().get(index))
        .map(|device| device.id())
    else {
        return 0;
    };
    i32::from(
        project
            .apply(&[
                Command::SetDeviceKind { id, kind },
                Command::SetDeviceEnabled { id, enabled },
            ])
            .is_ok(),
    )
}

/// # Safety
/// A non-null handle must be exclusively accessible.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_device_delete(
    handle: *mut Project,
    track: u64,
    index: u64,
) -> i32 {
    // SAFETY: Handle validity and exclusivity are required by the interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let (Ok(track), Ok(index)) = (usize::try_from(track), usize::try_from(index)) else {
        return 0;
    };
    let Some(id) = project
        .current
        .tracks
        .get(track)
        .and_then(|track| track.devices().get(index))
        .map(|device| device.id())
    else {
        return 0;
    };
    i32::from(project.apply(&[Command::DeleteDevice(id)]).is_ok())
}

/// # Safety
/// A non-null handle must be exclusively accessible.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_device_move(
    handle: *mut Project,
    track: u64,
    from: u64,
    to: u64,
) -> i32 {
    // SAFETY: Handle validity and exclusivity are required by the interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let (Ok(track), Ok(from), Ok(to)) = (
        usize::try_from(track),
        usize::try_from(from),
        usize::try_from(to),
    ) else {
        return 0;
    };
    let Some(id) = project
        .current
        .tracks
        .get(track)
        .and_then(|track| track.devices().get(from))
        .map(|device| device.id())
    else {
        return 0;
    };
    i32::from(
        project
            .apply(&[Command::MoveDevice { id, index: to }])
            .is_ok(),
    )
}

fn edge_kind_from_code(code: i32) -> Option<EdgeKind> {
    match code {
        0 => Some(EdgeKind::Main),
        1 => Some(EdgeKind::SendPreFader),
        2 => Some(EdgeKind::SendPostFader),
        3 => Some(EdgeKind::Sidechain),
        _ => None,
    }
}

fn edge_kind_code(kind: EdgeKind) -> i32 {
    match kind {
        EdgeKind::Main => 0,
        EdgeKind::SendPreFader => 1,
        EdgeKind::SendPostFader => 2,
        EdgeKind::Sidechain => 3,
    }
}

/// # Safety
/// A non-null handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_route_count(handle: *const Project) -> u64 {
    // SAFETY: Handle validity and access exclusion are required by the interface.
    unsafe { handle.as_ref() }.map_or(0, |project| project.current.routes.len() as u64)
}

/// # Safety
/// A non-null handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_route_source(handle: *const Project, index: u64) -> u64 {
    // SAFETY: Handle validity and access exclusion are required by the interface.
    let Some(project) = (unsafe { handle.as_ref() }) else {
        return u64::MAX;
    };
    let Ok(index) = usize::try_from(index) else {
        return u64::MAX;
    };
    let Some(route) = project.current.routes.get(index).copied() else {
        return u64::MAX;
    };
    project
        .current
        .tracks
        .iter()
        .position(|track| track.id() == route.source())
        .map_or(u64::MAX, |value| value as u64)
}

/// # Safety
/// A non-null handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_route_destination(
    handle: *const Project,
    index: u64,
) -> u64 {
    // SAFETY: Handle validity and access exclusion are required by the interface.
    let Some(project) = (unsafe { handle.as_ref() }) else {
        return u64::MAX;
    };
    let Ok(index) = usize::try_from(index) else {
        return u64::MAX;
    };
    let Some(route) = project.current.routes.get(index).copied() else {
        return u64::MAX;
    };
    project
        .current
        .tracks
        .iter()
        .position(|track| track.id() == route.destination())
        .map_or(u64::MAX, |value| value as u64)
}

/// # Safety
/// A non-null handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_route_kind(handle: *const Project, index: u64) -> i32 {
    // SAFETY: Handle validity and access exclusion are required by the interface.
    let Some(project) = (unsafe { handle.as_ref() }) else {
        return -1;
    };
    let Ok(index) = usize::try_from(index) else {
        return -1;
    };
    project
        .current
        .routes
        .get(index)
        .map_or(-1, |route| edge_kind_code(route.kind()))
}

/// # Safety
/// A non-null handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_route_gain(handle: *const Project, index: u64) -> f32 {
    // SAFETY: Handle validity and access exclusion are required by the interface.
    let Some(project) = (unsafe { handle.as_ref() }) else {
        return f32::NAN;
    };
    let Ok(index) = usize::try_from(index) else {
        return f32::NAN;
    };
    project
        .current
        .routes
        .get(index)
        .map_or(f32::NAN, |route| route.gain())
}

/// # Safety
/// A non-null handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_route_add(
    handle: *mut Project,
    source: u64,
    destination: u64,
    kind: i32,
    gain: f32,
) -> i32 {
    // SAFETY: Handle validity and exclusive access are required by the interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let (Ok(source), Ok(destination)) = (usize::try_from(source), usize::try_from(destination))
    else {
        return 0;
    };
    let (Some(source), Some(destination), Some(kind)) = (
        project.current.tracks.get(source),
        project.current.tracks.get(destination),
        edge_kind_from_code(kind),
    ) else {
        return 0;
    };
    let command = Command::CreateRoute {
        source: source.id(),
        destination: destination.id(),
        kind,
        gain,
    };
    i32::from(project.apply(&[command]).is_ok())
}

/// # Safety
/// A non-null handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_route_delete(handle: *mut Project, index: u64) -> i32 {
    // SAFETY: Handle validity and exclusive access are required by the interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    let Some(route) = project.current.routes.get(index) else {
        return 0;
    };
    i32::from(project.apply(&[Command::DeleteRoute(route.id())]).is_ok())
}

/// # Safety
/// A non-null handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_delete(handle: *mut Project, index: u64) -> i32 {
    // SAFETY: The caller provides a valid exclusive handle.
    unsafe { edit_track(handle, index, |id| Some(Command::DeleteTrack(id))) }
}

/// # Safety
/// A non-null handle must be live with no concurrent mutation. A non-null buffer
/// must be writable for `capacity` bytes and disjoint from the project storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_name(
    handle: *const Project,
    index: u64,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    // SAFETY: Handle validity is required by the interface.
    let name = unsafe { handle.as_ref() }
        .and_then(|p| {
            usize::try_from(index)
                .ok()
                .and_then(|i| p.current.tracks.get(i))
        })
        .map_or("", |track| track.name());
    if !buffer.is_null() && capacity > 0 {
        let count = name
            .len()
            .min(usize::try_from(capacity - 1).unwrap_or(usize::MAX));
        // SAFETY: The caller provides writable capacity and nonoverlapping storage.
        unsafe {
            std::ptr::copy_nonoverlapping(name.as_ptr(), buffer.cast(), count);
            buffer.add(count).write(0);
        }
    }
    name.len() as u64
}

/// # Safety
/// The project must be live and exclusive; name must be a valid NUL-terminated
/// string, or null. UTF-8 is required. Null and invalid text are rejected.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_set_name(
    handle: *mut Project,
    index: u64,
    name: *const c_char,
) -> i32 {
    if name.is_null() {
        return 0;
    }
    // SAFETY: The caller provides a terminated readable string.
    let Ok(name) = (unsafe { CStr::from_ptr(name) }).to_str() else {
        return 0;
    };
    // SAFETY: The caller provides a live exclusive project handle.
    unsafe {
        edit_track(handle, index, |id| {
            Some(Command::RenameTrack {
                id,
                name: name.into(),
            })
        })
    }
}

pub(crate) fn kind_from_int(kind: i32) -> Option<TrackKind> {
    match kind {
        0 => Some(TrackKind::Audio),
        1 => Some(TrackKind::Midi),
        2 => Some(TrackKind::Return),
        3 => Some(TrackKind::Master),
        4 => Some(TrackKind::Group),
        5 => Some(TrackKind::Cue),
        _ => None,
    }
}

pub(crate) fn kind_to_int(kind: TrackKind) -> i32 {
    match kind {
        TrackKind::Audio => 0,
        TrackKind::Midi => 1,
        TrackKind::Return => 2,
        TrackKind::Master => 3,
        TrackKind::Group => 4,
        TrackKind::Cue => 5,
    }
}

/// # Safety
/// A non-null handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_kind(handle: *const Project, index: u64) -> i32 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { handle.as_ref() }
        .and_then(|p| {
            usize::try_from(index)
                .ok()
                .and_then(|i| p.current.tracks.get(i))
        })
        .map_or(-1, |track| kind_to_int(track.kind()))
}

/// # Safety
/// A non-null handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_add_track_kind(handle: *mut Project, kind: i32) -> i32 {
    let Some(kind) = kind_from_int(kind) else {
        return 0;
    };
    // SAFETY: Handle validity and exclusivity are required by the interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let label = match kind {
        TrackKind::Midi => "MIDI",
        TrackKind::Audio => "Audio",
        TrackKind::Return => "Return",
        TrackKind::Master => "Master",
        TrackKind::Group => "Group",
        TrackKind::Cue => "Cue",
    };
    let name = format!("{label} {}", project.current.tracks.len() + 1);
    i32::from(
        project
            .apply(&[Command::CreateTrack { name, kind }])
            .is_ok(),
    )
}

macro_rules! project_getter {
    ($name:ident, $result:ty, $fallback:expr, $read:expr) => {
        /// # Safety
        /// A non-null handle must be live and have no concurrent mutation.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(handle: *const Project) -> $result {
            // SAFETY: Handle validity is required by the interface.
            unsafe { handle.as_ref() }.map_or($fallback, $read)
        }
    };
}
project_getter!(nylon_project_can_undo, i32, 0, |p| i32::from(p.can_undo()));
project_getter!(nylon_project_can_redo, i32, 0, |p| i32::from(p.can_redo()));
project_getter!(nylon_project_time_signature_numerator, i32, 0, |p| {
    i32::from(p.current.numerator)
});
project_getter!(nylon_project_time_signature_denominator, i32, 0, |p| {
    i32::from(p.current.denominator)
});
project_getter!(nylon_project_sample_rate, u32, 0, |p| p.current.sample_rate);

/// # Safety
/// A non-null handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_new_in_place(handle: *mut Project) -> i32 {
    // SAFETY: Handle validity and exclusivity are required by the interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    *project = Project::new();
    1
}

/// # Safety
/// A non-null handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_set_time_signature(
    handle: *mut Project,
    numerator: i32,
    denominator: i32,
) -> i32 {
    let (Ok(numerator), Ok(denominator)) = (u16::try_from(numerator), u16::try_from(denominator))
    else {
        return 0;
    };
    // SAFETY: Handle validity and exclusivity are required by the interface.
    unsafe { handle.as_mut() }.map_or(0, |p| {
        i32::from(
            p.apply(&[Command::SetTimeSignature {
                numerator,
                denominator,
            }])
            .is_ok(),
        )
    })
}

/// # Safety
/// A non-null handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_set_sample_rate(handle: *mut Project, rate: u32) -> i32 {
    // SAFETY: Handle validity and exclusivity are required by the interface.
    unsafe { handle.as_mut() }.map_or(0, |p| {
        i32::from(p.apply(&[Command::SetSampleRate(rate)]).is_ok())
    })
}

/// # Safety
/// The project must be live with no concurrent mutation. A non-null directory
/// must be a readable NUL-terminated UTF-8 string. This performs control-thread I/O.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_save(handle: *mut Project, directory: *const c_char) -> i32 {
    if directory.is_null() {
        return 0;
    }
    // SAFETY: The caller supplies a terminated readable directory string.
    let Ok(directory) = (unsafe { CStr::from_ptr(directory) }).to_str() else {
        return 0;
    };
    if directory.is_empty() {
        return 0;
    }
    // SAFETY: The handle remains live and cannot be concurrently accessed.
    unsafe { handle.as_mut() }.map_or(0, |p| {
        i32::from(p.save_bundle(std::path::Path::new(directory)).is_ok())
    })
}

/// # Safety
/// A non-null handle must refer to a live project with no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_is_modified(handle: *const Project) -> i32 {
    // SAFETY: Handle validity and access exclusion are required by the interface.
    unsafe { handle.as_ref() }.map_or(0, |project| i32::from(project.is_modified()))
}

/// # Safety
/// The project must be live with no concurrent mutation. This performs control-thread I/O.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_autosave(handle: *const Project) -> i32 {
    // SAFETY: Handle validity and access exclusion are required by the interface.
    unsafe { handle.as_ref() }.map_or(0, |project| i32::from(project.autosave().is_ok()))
}

unsafe fn recovery_directory(directory: *const c_char) -> Option<std::path::PathBuf> {
    if directory.is_null() {
        return None;
    }
    // SAFETY: Callers supply a terminated readable directory string.
    let directory = unsafe { CStr::from_ptr(directory) }.to_str().ok()?;
    if directory.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(directory))
}

/// # Safety
/// `directory` must be a readable NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_recovery_available(directory: *const c_char) -> i32 {
    // SAFETY: The caller supplies the directory storage for this call.
    unsafe { recovery_directory(directory) }
        .map_or(0, |path| i32::from(Project::recovery_available(&path)))
}

/// # Safety
/// The project must be live and exclusive. `directory` must be a readable
/// NUL-terminated UTF-8 string. Failure preserves the current project.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_recover(
    handle: *mut Project,
    directory: *const c_char,
) -> i32 {
    // SAFETY: The caller provides a live exclusive handle.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    // SAFETY: The caller supplies the directory storage for this call.
    let Some(directory) = (unsafe { recovery_directory(directory) }) else {
        return 0;
    };
    let Ok(recovered) = Project::recover_bundle(&directory) else {
        return 0;
    };
    *project = recovered;
    1
}

/// # Safety
/// `directory` must be a readable NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_discard_recovery(directory: *const c_char) -> i32 {
    // SAFETY: The caller supplies the directory storage for this call.
    unsafe { recovery_directory(directory) }.map_or(0, |path| {
        i32::from(Project::discard_recovery(&path).is_ok())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn nylon_routing_new(node_count: u32) -> *mut RoutingGraph {
    let Ok(node_count) = usize::try_from(node_count) else {
        return std::ptr::null_mut();
    };
    RoutingGraph::new(node_count)
        .map(|graph| Box::into_raw(Box::new(graph)))
        .unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// A non-null handle must originate from `nylon_routing_new` and be released once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_routing_free(handle: *mut RoutingGraph) {
    if !handle.is_null() {
        // SAFETY: Ownership of the original allocation is transferred back.
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// # Safety
/// A non-null handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_routing_set_node_latency(
    handle: *mut RoutingGraph,
    node: u32,
    frames: u32,
) -> i32 {
    let Ok(node) = u16::try_from(node) else {
        return 0;
    };
    // SAFETY: Handle validity and exclusive access are required by the interface.
    unsafe { handle.as_mut() }.map_or(0, |graph| {
        i32::from(graph.set_node_latency(node, frames).is_ok())
    })
}

/// # Safety
/// The graph and output index must each point to live, disjoint storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_routing_add_edge(
    handle: *mut RoutingGraph,
    source: u32,
    destination: u32,
    kind: i32,
    gain: f32,
    out_index: *mut u32,
) -> i32 {
    let (Ok(source), Ok(destination)) = (u16::try_from(source), u16::try_from(destination)) else {
        return 0;
    };
    let Some(kind) = edge_kind_from_code(kind) else {
        return 0;
    };
    // SAFETY: The caller supplies one writable output index.
    let Some(out_index) = (unsafe { out_index.as_mut() }) else {
        return 0;
    };
    // SAFETY: Handle validity and exclusive access are required by the interface.
    let Some(graph) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Ok(index) = graph.add_edge(Edge {
        source,
        destination,
        kind,
        gain,
    }) else {
        return 0;
    };
    let Ok(index) = u32::try_from(index) else {
        return 0;
    };
    *out_index = index;
    1
}

/// # Safety
/// A non-null graph must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_routing_compile(
    handle: *const RoutingGraph,
) -> *mut CompiledRouting {
    // SAFETY: Handle validity and access exclusion are required by the interface.
    let Some(graph) = (unsafe { handle.as_ref() }) else {
        return std::ptr::null_mut();
    };
    graph
        .compile()
        .map(|compiled| Box::into_raw(Box::new(compiled)))
        .unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// A non-null handle must originate from `nylon_routing_compile` and be released once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_compiled_routing_free(handle: *mut CompiledRouting) {
    if !handle.is_null() {
        // SAFETY: Ownership of the original allocation is transferred back.
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// # Safety
/// A non-null handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_compiled_routing_node_count(handle: *const CompiledRouting) -> u32 {
    // SAFETY: Handle validity and access exclusion are required by the interface.
    unsafe { handle.as_ref() }.map_or(0, |routing| routing.order().len() as u32)
}

/// # Safety
/// A non-null handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_compiled_routing_order_at(
    handle: *const CompiledRouting,
    index: u32,
) -> i32 {
    // SAFETY: Handle validity and access exclusion are required by the interface.
    unsafe { handle.as_ref() }
        .and_then(|routing| routing.order().get(index as usize))
        .map_or(-1, |node| i32::from(*node))
}

/// # Safety
/// A non-null handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_compiled_routing_edge_delay(
    handle: *const CompiledRouting,
    index: u32,
    out_frames: *mut u32,
) -> i32 {
    // SAFETY: The caller supplies one writable frame count.
    let Some(out_frames) = (unsafe { out_frames.as_mut() }) else {
        return 0;
    };
    // SAFETY: Handle validity and access exclusion are required by the interface.
    let Some(frames) =
        (unsafe { handle.as_ref() }).and_then(|routing| routing.edge_delay(index as usize))
    else {
        return 0;
    };
    *out_frames = frames;
    1
}

/// # Safety
/// A non-null handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_compiled_routing_output_latency(
    handle: *const CompiledRouting,
    node: u32,
    out_frames: *mut u32,
) -> i32 {
    let Ok(node) = u16::try_from(node) else {
        return 0;
    };
    // SAFETY: The caller supplies one writable frame count.
    let Some(out_frames) = (unsafe { out_frames.as_mut() }) else {
        return 0;
    };
    // SAFETY: Handle validity and access exclusion are required by the interface.
    let Some(frames) =
        (unsafe { handle.as_ref() }).and_then(|routing| routing.output_latency(node))
    else {
        return 0;
    };
    *out_frames = frames;
    1
}

/// # Safety
/// The project must be live and exclusive. A non-null directory must be a readable
/// NUL-terminated UTF-8 string. Failed loads preserve the current project and history.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_project_open(handle: *mut Project, directory: *const c_char) -> i32 {
    if directory.is_null() {
        return 0;
    }
    // SAFETY: The caller provides a live exclusive handle.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    // SAFETY: The caller supplies a terminated readable directory string.
    let Ok(directory) = (unsafe { CStr::from_ptr(directory) }).to_str() else {
        return 0;
    };
    if directory.is_empty() {
        return 0;
    }
    let Ok(loaded) = Project::load_bundle(std::path::Path::new(directory)) else {
        return 0;
    };
    *project = loaded;
    1
}

/// Renders a beat range to a stereo 24-bit WAVE file.
///
/// # Safety
/// The project must be live with no concurrent mutation. `path` must point
/// to a readable NUL-terminated UTF-8 string. `out` must point to one writable
/// report. This performs control-thread I/O and offline rendering.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_render_bounce_wave(
    handle: *const Project,
    path: *const c_char,
    start_beats: f64,
    end_beats: f64,
    sample_rate: u32,
    out: *mut NylonBounceReport,
) -> i32 {
    // SAFETY: The caller keeps the project live and immutable for this call.
    let Some(project) = (unsafe { handle.as_ref() }) else {
        return 0;
    };
    // SAFETY: The caller supplies one writable report record.
    let Some(out) = (unsafe { out.as_mut() }) else {
        return 0;
    };
    if path.is_null() {
        return 0;
    }
    // SAFETY: The caller supplies a terminated readable path string.
    let Ok(path) = (unsafe { CStr::from_ptr(path) }).to_str() else {
        return 0;
    };
    if path.is_empty() {
        return 0;
    }
    let options = BounceOptions {
        start_beats,
        end_beats,
        block_frames: 512,
        format: Format::stereo(sample_rate),
    };
    if options.validate().is_err() {
        return 0;
    }
    let Ok(file) = File::create(path) else {
        return 0;
    };
    let Ok((_, report)) = render_wave(project, file, options) else {
        return 0;
    };
    *out = report.into();
    1
}

fn copy_text(text: &str, buffer: *mut c_char, capacity: u64) -> u64 {
    if !buffer.is_null() && capacity > 0 {
        let count = text
            .len()
            .min(usize::try_from(capacity - 1).unwrap_or(usize::MAX));
        // SAFETY: Native callers provide writable, nonoverlapping output storage.
        unsafe {
            std::ptr::copy_nonoverlapping(text.as_ptr(), buffer.cast(), count);
            buffer.add(count).write(0);
        }
    }
    text.len() as u64
}

fn write_fixed_text<const N: usize>(text: &str, output: &mut [c_char; N]) {
    output.fill(0);
    let count = text.len().min(N.saturating_sub(1));
    for (destination, source) in output.iter_mut().zip(text.as_bytes()).take(count) {
        *destination = *source as c_char;
    }
}

unsafe fn input_text(value: *const c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    // SAFETY: Native callers provide a readable terminated string.
    unsafe { CStr::from_ptr(value) }
        .to_str()
        .ok()
        .map(str::to_owned)
}

fn slot_ids(project: &Project, track: u64, scene: u64) -> Option<(TrackId, SceneId, ClipId)> {
    let track = usize::try_from(track).ok()?;
    let scene = usize::try_from(scene).ok()?;
    let track_ref = project.current.tracks.get(track)?;
    let scene_ref = project.current.scenes.get(scene)?;
    Some((
        track_ref.id,
        scene_ref.id,
        track_ref.session_slots.get(scene)?.as_ref().copied()?,
    ))
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_scene_count(handle: *const Project) -> u64 {
    // SAFETY: Handle validity is required by the native interface.
    unsafe { handle.as_ref() }.map_or(0, |p| p.current.scenes.len() as u64)
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_scene_create(handle: *mut Project, name: *const c_char) -> i32 {
    // SAFETY: The native interface requires readable text and exclusive project access.
    let Some(name) = (unsafe { input_text(name) }) else {
        return 0;
    };
    // SAFETY: Handle validity and exclusivity are required by the native interface.
    unsafe { handle.as_mut() }.map_or(0, |p| {
        i32::from(p.apply(&[Command::CreateScene { name }]).is_ok())
    })
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_scene_delete(handle: *mut Project, scene: u64) -> i32 {
    // SAFETY: Handle validity and exclusivity are required by the native interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Some(id) = usize::try_from(scene)
        .ok()
        .and_then(|i| project.current.scenes.get(i))
        .map(|s| s.id)
    else {
        return 0;
    };
    i32::from(project.apply(&[Command::DeleteScene(id)]).is_ok())
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_scene_name(
    handle: *const Project,
    scene: u64,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    // SAFETY: Handle validity is required by the native interface.
    let text = unsafe { handle.as_ref() }
        .and_then(|p| {
            usize::try_from(scene)
                .ok()
                .and_then(|i| p.current.scenes.get(i))
        })
        .map_or("", |s| s.name.as_str());
    copy_text(text, buffer, capacity)
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_scene_set_name(
    handle: *mut Project,
    scene: u64,
    name: *const c_char,
) -> i32 {
    // SAFETY: The native interface requires readable text and exclusive project access.
    let Some(name) = (unsafe { input_text(name) }) else {
        return 0;
    };
    // SAFETY: Handle validity and exclusivity are required by the native interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Some(id) = usize::try_from(scene)
        .ok()
        .and_then(|i| project.current.scenes.get(i))
        .map(|s| s.id)
    else {
        return 0;
    };
    i32::from(project.apply(&[Command::RenameScene { id, name }]).is_ok())
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_slot_state(
    handle: *const Project,
    track: u64,
    scene: u64,
) -> i32 {
    // SAFETY: Handle validity is required by the native interface.
    unsafe { handle.as_ref() }.map_or(0, |project| {
        let Some((_, _, clip)) = slot_ids(project, track, scene) else {
            return 0;
        };
        if project
            .current
            .audio_clips
            .iter()
            .any(|item| item.id == clip)
        {
            2
        } else {
            1
        }
    })
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_create_midi(
    handle: *mut Project,
    track: u64,
    scene: u64,
    length_beats: f64,
) -> i32 {
    // SAFETY: Handle validity and exclusivity are required by the native interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let (Some(track_id), Some(scene_id)) = (
        usize::try_from(track)
            .ok()
            .and_then(|i| project.current.tracks.get(i))
            .map(|t| t.id),
        usize::try_from(scene)
            .ok()
            .and_then(|i| project.current.scenes.get(i))
            .map(|s| s.id),
    ) else {
        return 0;
    };
    let name = format!("MIDI Clip {}", project.current.clips.len() + 1);
    i32::from(
        project
            .apply(&[Command::CreateMidiClip {
                track: track_id,
                scene: scene_id,
                name,
                length_beats,
            }])
            .is_ok(),
    )
}

/// # Safety
/// The handle must be live and exclusive. `source_path` must be readable,
/// terminated UTF-8. Import performs control-thread file I/O.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_import_wave(
    handle: *mut Project,
    source_path: *const c_char,
    track: u64,
    scene: u64,
    source_tempo: f64,
) -> i32 {
    // SAFETY: The native interface requires a live exclusive project handle.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    // SAFETY: The caller supplies a readable terminated path string.
    let Some(source_path) = (unsafe { input_text(source_path) }) else {
        return 0;
    };
    let (Ok(track), Ok(scene)) = (usize::try_from(track), usize::try_from(scene)) else {
        return 0;
    };
    i32::from(
        import_wave(
            project,
            std::path::Path::new(&source_path),
            track,
            scene,
            source_tempo,
        )
        .is_ok(),
    )
}

fn audio_slot(project: &Project, track: u64, scene: u64) -> Option<&crate::project::AudioClip> {
    let (_, _, clip) = slot_ids(project, track, scene)?;
    project
        .current
        .audio_clips
        .iter()
        .find(|item| item.id == clip)
}

/// # Safety
/// The handle and output buffer must obey the native interface contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_media_path(
    handle: *const Project,
    track: u64,
    scene: u64,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    // SAFETY: Handle validity is required by the native interface.
    let path = unsafe { handle.as_ref() }
        .and_then(|project| audio_slot(project, track, scene))
        .map_or("", |clip| clip.media_path());
    copy_text(path, buffer, capacity)
}

/// # Safety
/// The handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_audio_gain_db(
    handle: *const Project,
    track: u64,
    scene: u64,
) -> f64 {
    // SAFETY: Handle validity is required by the native interface.
    unsafe { handle.as_ref() }
        .and_then(|project| audio_slot(project, track, scene))
        .map_or(f64::NEG_INFINITY, |clip| clip.gain_db())
}

/// # Safety
/// The handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_set_audio_gain_db(
    handle: *mut Project,
    track: u64,
    scene: u64,
    db: f64,
) -> i32 {
    // SAFETY: The caller supplies a live exclusive project handle.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::SetAudioClipGain { id, db })
        })
    }
}

/// # Safety
/// The handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_audio_reverse(
    handle: *const Project,
    track: u64,
    scene: u64,
) -> i32 {
    // SAFETY: Handle validity is required by the native interface.
    unsafe { handle.as_ref() }
        .and_then(|project| audio_slot(project, track, scene))
        .map_or(0, |clip| i32::from(clip.reversed()))
}

/// # Safety
/// The handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_set_audio_reverse(
    handle: *mut Project,
    track: u64,
    scene: u64,
    enabled: i32,
) -> i32 {
    if !matches!(enabled, 0 | 1) {
        return 0;
    }
    // SAFETY: The caller supplies a live exclusive project handle.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::SetAudioClipReverse {
                id,
                enabled: enabled != 0,
            })
        })
    }
}

/// # Safety
/// The handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_audio_warp(
    handle: *const Project,
    track: u64,
    scene: u64,
) -> i32 {
    // SAFETY: Handle validity is required by the native interface.
    unsafe { handle.as_ref() }
        .and_then(|project| audio_slot(project, track, scene))
        .map_or(0, |clip| i32::from(clip.warped()))
}

/// # Safety
/// The handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_audio_source_tempo(
    handle: *const Project,
    track: u64,
    scene: u64,
) -> f64 {
    // SAFETY: Handle validity is required by the native interface.
    unsafe { handle.as_ref() }
        .and_then(|project| audio_slot(project, track, scene))
        .map_or(0.0, |clip| clip.source_tempo())
}

/// # Safety
/// The handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_set_audio_warp(
    handle: *mut Project,
    track: u64,
    scene: u64,
    enabled: i32,
    source_tempo: f64,
) -> i32 {
    if !matches!(enabled, 0 | 1) {
        return 0;
    }
    // SAFETY: The caller supplies a live exclusive project handle.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::SetAudioClipWarp {
                id,
                enabled: enabled != 0,
                source_tempo,
            })
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_delete(handle: *mut Project, track: u64, scene: u64) -> i32 {
    // SAFETY: Handle validity and exclusivity are required by the native interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Some((track, scene, _)) = slot_ids(project, track, scene) else {
        return 0;
    };
    i32::from(
        project
            .apply(&[Command::DeleteClip { track, scene }])
            .is_ok(),
    )
}

unsafe fn edit_slot_clip(
    handle: *mut Project,
    track: u64,
    scene: u64,
    make: impl FnOnce(ClipId) -> Option<Command>,
) -> i32 {
    // SAFETY: The caller transfers the native interface's exclusive access contract.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Some((_, _, clip)) = slot_ids(project, track, scene) else {
        return 0;
    };
    let Some(command) = make(clip) else { return 0 };
    i32::from(project.apply(&[command]).is_ok())
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_name(
    handle: *const Project,
    track: u64,
    scene: u64,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    // SAFETY: Handle validity is required by the native interface.
    let text = unsafe { handle.as_ref() }
        .and_then(|p| {
            let (_, _, id) = slot_ids(p, track, scene)?;
            p.current
                .clips
                .iter()
                .find(|clip| clip.id == id)
                .map(|clip| clip.name.as_str())
        })
        .unwrap_or("");
    copy_text(text, buffer, capacity)
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_set_name(
    handle: *mut Project,
    track: u64,
    scene: u64,
    name: *const c_char,
) -> i32 {
    // SAFETY: The native interface requires readable text and exclusive project access.
    let Some(name) = (unsafe { input_text(name) }) else {
        return 0;
    };
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::SetClipName { id, name })
        })
    }
}

macro_rules! clip_getter {
    ($name:ident, $result:ty, $fallback:expr, $read:expr) => {
        /// # Safety
        /// The handle must be live and have no concurrent mutation.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(handle: *const Project, track: u64, scene: u64) -> $result {
            // SAFETY: Handle validity is required by the native interface.
            unsafe { handle.as_ref() }
                .and_then(|p| {
                    let (_, _, id) = slot_ids(p, track, scene)?;
                    p.current.clips.iter().find(|clip| clip.id == id).map($read)
                })
                .unwrap_or($fallback)
        }
    };
}

clip_getter!(nylon_clip_color_index, i32, -1, |clip| i32::from(
    clip.color_index
));
clip_getter!(nylon_clip_loop_start, f64, 0.0, |clip| clip
    .loop_start_beats);
clip_getter!(nylon_clip_loop_length, f64, 0.0, |clip| clip
    .loop_length_beats);
clip_getter!(nylon_clip_note_count, u64, 0, |clip| clip.notes.len()
    as u64);

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_set_color_index(
    handle: *mut Project,
    track: u64,
    scene: u64,
    index: i32,
) -> i32 {
    let Ok(index) = u8::try_from(index) else {
        return 0;
    };
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::SetClipColor { id, index })
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_set_loop(
    handle: *mut Project,
    track: u64,
    scene: u64,
    start_beats: f64,
    length_beats: f64,
) -> i32 {
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::SetClipLoop {
                id,
                start_beats,
                length_beats,
            })
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_note_at(
    handle: *const Project,
    track: u64,
    scene: u64,
    index: u64,
    pitch: *mut u8,
    velocity: *mut u8,
    start_beats: *mut f64,
    length_beats: *mut f64,
) -> i32 {
    if pitch.is_null() || velocity.is_null() || start_beats.is_null() || length_beats.is_null() {
        return 0;
    }
    // SAFETY: Handle validity is required by the native interface.
    let Some(note) = (unsafe { handle.as_ref() }).and_then(|p| {
        let (_, _, id) = slot_ids(p, track, scene)?;
        let clip = p.current.clips.iter().find(|clip| clip.id == id)?;
        clip.notes.get(usize::try_from(index).ok()?).copied()
    }) else {
        return 0;
    };
    // SAFETY: Native callers provide four writable output values.
    unsafe {
        pitch.write(note.pitch);
        velocity.write(note.velocity);
        start_beats.write(note.start_beats);
        length_beats.write(note.length_beats);
    }
    1
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_note_add(
    handle: *mut Project,
    track: u64,
    scene: u64,
    pitch: u8,
    velocity: u8,
    start_beats: f64,
    length_beats: f64,
) -> i32 {
    let note = MidiNote {
        pitch,
        velocity,
        start_beats,
        length_beats,
    };
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::AddNote { id, note })
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_note_remove(
    handle: *mut Project,
    track: u64,
    scene: u64,
    index: u64,
) -> i32 {
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::RemoveNote { id, index })
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_note_move(
    handle: *mut Project,
    track: u64,
    scene: u64,
    index: u64,
    pitch: u8,
    velocity: u8,
    start_beats: f64,
    length_beats: f64,
) -> i32 {
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    let note = MidiNote {
        pitch,
        velocity,
        start_beats,
        length_beats,
    };
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::MoveNote { id, index, note })
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_notes_quantize(
    handle: *mut Project,
    track: u64,
    scene: u64,
    grid_beats: f64,
    strength: f64,
) -> i32 {
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::QuantizeNotes {
                id,
                grid_beats,
                strength,
            })
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_notes_transpose(
    handle: *mut Project,
    track: u64,
    scene: u64,
    semitones: i32,
) -> i32 {
    let Ok(semitones) = i16::try_from(semitones) else {
        return 0;
    };
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::TransposeNotes { id, semitones })
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_notes_set_velocity(
    handle: *mut Project,
    track: u64,
    scene: u64,
    velocity: u8,
) -> i32 {
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::SetNoteVelocity { id, velocity })
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clip_notes_humanize(
    handle: *mut Project,
    track: u64,
    scene: u64,
    timing_beats: f64,
    velocity_range: u8,
    seed: u64,
) -> i32 {
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_slot_clip(handle, track, scene, |id| {
            Some(Command::HumanizeNotes {
                id,
                timing_beats,
                velocity_range,
                seed,
            })
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_arrangement_clip_count(handle: *const Project, track: u64) -> u64 {
    // SAFETY: Handle validity is required by the native interface.
    unsafe { handle.as_ref() }
        .and_then(|p| {
            usize::try_from(track)
                .ok()
                .and_then(|i| p.current.tracks.get(i))
        })
        .map_or(0, |t| t.arrangement.len() as u64)
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_arrangement_clip_add_from_slot(
    handle: *mut Project,
    track: u64,
    scene: u64,
    start_beats: f64,
    length_beats: f64,
) -> i32 {
    // SAFETY: Handle validity and exclusivity are required by the native interface.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Some((track, _, clip)) = slot_ids(project, track, scene) else {
        return 0;
    };
    i32::from(
        project
            .apply(&[Command::PlaceClip {
                track,
                clip,
                start_beats,
                length_beats,
            }])
            .is_ok(),
    )
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_arrangement_clip_range(
    handle: *const Project,
    track: u64,
    index: u64,
    start_beats: *mut f64,
    length_beats: *mut f64,
) -> i32 {
    if start_beats.is_null() || length_beats.is_null() {
        return 0;
    }
    // SAFETY: Handle validity is required by the native interface.
    let Some(placement) = (unsafe { handle.as_ref() }).and_then(|p| {
        p.current
            .tracks
            .get(usize::try_from(track).ok()?)?
            .arrangement
            .get(usize::try_from(index).ok()?)
            .copied()
    }) else {
        return 0;
    };
    // SAFETY: Native callers provide two writable output values.
    unsafe {
        start_beats.write(placement.start_beats);
        length_beats.write(placement.length_beats);
    }
    1
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_arrangement_clip_name(
    handle: *const Project,
    track: u64,
    index: u64,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    // SAFETY: Handle validity is required by the native interface.
    let text = unsafe { handle.as_ref() }
        .and_then(|project| {
            let track = project.current.tracks.get(usize::try_from(track).ok()?)?;
            let placement = track.arrangement.get(usize::try_from(index).ok()?)?;
            project
                .current
                .clips
                .iter()
                .find(|clip| clip.id == placement.clip)
                .map(|clip| clip.name.as_str())
        })
        .unwrap_or("");
    copy_text(text, buffer, capacity)
}

/// # Safety
/// The handle must be live and have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_arrangement_clip_color_index(
    handle: *const Project,
    track: u64,
    index: u64,
) -> i32 {
    // SAFETY: Handle validity is required by the native interface.
    unsafe { handle.as_ref() }
        .and_then(|project| {
            let track = project.current.tracks.get(usize::try_from(track).ok()?)?;
            let placement = track.arrangement.get(usize::try_from(index).ok()?)?;
            project
                .current
                .clips
                .iter()
                .find(|clip| clip.id == placement.clip)
        })
        .map_or(-1, |clip| i32::from(clip.color_index))
}

unsafe fn edit_placement(
    handle: *mut Project,
    track: u64,
    index: u64,
    make: impl FnOnce(TrackId, usize) -> Command,
) -> i32 {
    // SAFETY: The caller transfers the native interface's exclusive access contract.
    let Some(project) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let (Ok(track_index), Ok(index)) = (usize::try_from(track), usize::try_from(index)) else {
        return 0;
    };
    let Some(track) = project.current.tracks.get(track_index) else {
        return 0;
    };
    if index >= track.arrangement.len() {
        return 0;
    }
    i32::from(project.apply(&[make(track.id, index)]).is_ok())
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_arrangement_clip_remove(
    handle: *mut Project,
    track: u64,
    index: u64,
) -> i32 {
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_placement(handle, track, index, |track, index| {
            Command::RemovePlacement { track, index }
        })
    }
}

/// # Safety
/// The handle must be live and obey the access contract in the C header. Pointer
/// arguments must reference readable or writable storage for the documented span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_arrangement_clip_set_range(
    handle: *mut Project,
    track: u64,
    index: u64,
    start_beats: f64,
    length_beats: f64,
) -> i32 {
    // SAFETY: The handle is exclusive for this call.
    unsafe {
        edit_placement(handle, track, index, |track, index| {
            Command::SetPlacementRange {
                track,
                index,
                start_beats,
                length_beats,
            }
        })
    }
}

fn native_device(info: DeviceInfo) -> NylonAudioDevice {
    let mut device = NylonAudioDevice {
        id: info.id.0,
        name: [0; crate::audio::MAX_NAME + 1],
        channels: u32::from(info.channels),
        is_default: i32::from(info.is_default),
        sample_rates: [0; crate::audio::MAX_RATES],
        sample_rate_count: info.rates.len() as u32,
    };
    for (destination, source) in device.name.iter_mut().zip(info.name.as_str().bytes()) {
        *destination = source as c_char;
    }
    for (destination, source) in device.sample_rates.iter_mut().zip(info.rates.as_slice()) {
        *destination = *source;
    }
    device
}

/// Creates a closed live-audio controller.
#[unsafe(no_mangle)]
pub extern "C" fn nylon_audio_new() -> *mut AudioRuntime {
    Box::into_raw(Box::new(AudioRuntime::new()))
}

/// # Safety
/// A non-null handle must originate from `nylon_audio_new`, remain live,
/// and be released exactly once with no outstanding references.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_audio_free(handle: *mut AudioRuntime) {
    if !handle.is_null() {
        // SAFETY: Ownership of the original allocation is transferred back.
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// # Safety
/// When `out` is non-null it must hold `capacity` writable records. Passing
/// null queries the available count without copying records.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_audio_device_list(out: *mut NylonAudioDevice, capacity: u64) -> u64 {
    // SAFETY: The caller provides the output storage described above.
    unsafe { copy_audio_devices(out, capacity, Direction::Output) }
}

/// # Safety
/// When `out` is non-null it must hold `capacity` writable records. Passing
/// null queries the available input count without copying records.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_audio_input_device_list(
    out: *mut NylonAudioDevice,
    capacity: u64,
) -> u64 {
    // SAFETY: The caller provides the output storage described above.
    unsafe { copy_audio_devices(out, capacity, Direction::Input) }
}

unsafe fn copy_audio_devices(
    out: *mut NylonAudioDevice,
    capacity: u64,
    direction: Direction,
) -> u64 {
    let empty = DeviceInfo {
        id: DeviceId(0),
        name: Name::new(),
        direction,
        channels: 0,
        rates: Rates::new(),
        is_default: false,
    };
    let mut devices = [empty; MAX_DEVICES];
    let result = match direction {
        Direction::Input => input_devices(&mut devices),
        Direction::Output => output_devices(&mut devices),
    };
    let Ok(count) = result else {
        return 0;
    };
    if !out.is_null() {
        let capacity = usize::try_from(capacity).unwrap_or(usize::MAX);
        for (index, info) in devices[..count.min(capacity)].iter().enumerate() {
            // SAFETY: The caller guarantees writable storage for `capacity`
            // records and the loop never exceeds it.
            unsafe { out.add(index).write(native_device(*info)) };
        }
    }
    count as u64
}

/// # Safety
/// `device_id` must point to one writable integer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_audio_default_output(device_id: *mut u64) -> i32 {
    if device_id.is_null() {
        return 0;
    }
    let Ok(device) = default_output() else {
        return 0;
    };
    // SAFETY: The caller provides one writable integer.
    unsafe { device_id.write(device.0) };
    1
}

/// # Safety
/// `device_id` must point to one writable integer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_audio_default_input(device_id: *mut u64) -> i32 {
    if device_id.is_null() {
        return 0;
    }
    let Ok(device) = default_input() else {
        return 0;
    };
    // SAFETY: The caller provides one writable integer.
    unsafe { device_id.write(device.0) };
    1
}

/// Opens a stopped recording for one project clip slot.
///
/// # Safety
/// The project must be live with no concurrent mutation.
#[cfg(platform_audio)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_recording_open(
    project: *const Project,
    track: u64,
    scene: u64,
    device_id: u64,
    sample_rate: u32,
    block_frames: u32,
) -> *mut ProjectRecording {
    // SAFETY: The caller keeps the project live and immutable for this call.
    let Some(project) = (unsafe { project.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let (Ok(track), Ok(scene)) = (usize::try_from(track), usize::try_from(scene)) else {
        return std::ptr::null_mut();
    };
    ProjectRecording::open(
        project,
        track,
        scene,
        DeviceId(device_id),
        sample_rate,
        block_frames as usize,
    )
    .map(|recording| Box::into_raw(Box::new(recording)))
    .unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// A non-null handle must originate from `nylon_recording_open`, remain live,
/// and be released exactly once.
#[cfg(platform_audio)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_recording_free(recording: *mut ProjectRecording) {
    if !recording.is_null() {
        // SAFETY: Ownership of the original allocation is transferred back.
        drop(unsafe { Box::from_raw(recording) });
    }
}

/// # Safety
/// The handle must be live and exclusively accessible to this call.
#[cfg(platform_audio)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_recording_start(recording: *mut ProjectRecording) -> i32 {
    // SAFETY: The caller grants exclusive access to the live handle.
    unsafe { recording.as_mut() }.map_or(0, |recording| i32::from(recording.start().is_ok()))
}

/// # Safety
/// The handle must be live and exclusively accessible to this call.
#[cfg(platform_audio)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_recording_stop(recording: *mut ProjectRecording) -> i32 {
    // SAFETY: The caller grants exclusive access to the live handle.
    unsafe { recording.as_mut() }.map_or(0, |recording| i32::from(recording.stop().is_ok()))
}

/// # Safety
/// The handle must be live with no concurrent access.
#[cfg(platform_audio)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_recording_is_running(recording: *const ProjectRecording) -> i32 {
    // SAFETY: The caller keeps the handle live and immutable for this call.
    unsafe { recording.as_ref() }.map_or(0, |recording| i32::from(recording.is_running()))
}

/// Finalizes the file and creates one undoable audio clip.
///
/// # Safety
/// Both handles must be live and exclusive. `out` must point to one writable
/// report. The recording remains valid but finished after a successful call.
#[cfg(platform_audio)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_recording_finish(
    recording: *mut ProjectRecording,
    project: *mut Project,
    out: *mut NylonRecordingReport,
) -> i32 {
    // SAFETY: The caller grants exclusive access to both live handles.
    let recording = unsafe { recording.as_mut() };
    // SAFETY: The caller grants exclusive access to both live handles.
    let project = unsafe { project.as_mut() };
    // SAFETY: The caller supplies one writable report.
    let out = unsafe { out.as_mut() };
    let (Some(recording), Some(project), Some(out)) = (recording, project, out) else {
        return 0;
    };
    let Ok(report) = recording.finish(project) else {
        return 0;
    };
    *out = NylonRecordingReport {
        frames: report.media.frames as u64,
        sample_rate: report.media.sample_rate,
        length_beats: report.media.length_beats,
        lost_blocks: report.lost_blocks,
        lost_frames: report.lost_frames,
    };
    1
}

/// # Safety
/// Both handles must be live, belong to the calling control thread, and
/// the project must have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_audio_open(
    audio: *mut AudioRuntime,
    project: *const Project,
    device_id: u64,
    sample_rate: u32,
    block_frames: u32,
) -> i32 {
    // SAFETY: The interface contract makes the audio handle exclusive.
    let audio = unsafe { audio.as_mut() };
    // SAFETY: The interface contract keeps the project live and immutable.
    let project = unsafe { project.as_ref() };
    let (Some(audio), Some(project)) = (audio, project) else {
        return 0;
    };
    i32::from(
        audio
            .open(
                project,
                DeviceId(device_id),
                sample_rate,
                block_frames as usize,
            )
            .is_ok(),
    )
}

/// # Safety
/// The handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_audio_close(audio: *mut AudioRuntime) -> i32 {
    // SAFETY: The interface contract makes the handle exclusive.
    let Some(audio) = (unsafe { audio.as_mut() }) else {
        return 0;
    };
    audio.close();
    1
}

/// # Safety
/// The handle must be live with no concurrent access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_audio_is_open(audio: *const AudioRuntime) -> i32 {
    // SAFETY: The interface contract keeps the handle live and immutable.
    unsafe { audio.as_ref() }.map_or(0, |audio| i32::from(audio.is_open()))
}

/// # Safety
/// The handle must be live with no concurrent access. `out` must point to
/// one writable configuration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_audio_config(
    audio: *const AudioRuntime,
    out: *mut NylonAudioConfig,
) -> i32 {
    // SAFETY: The interface contract keeps the handle live and immutable.
    let audio = unsafe { audio.as_ref() };
    // SAFETY: The interface contract provides one writable record.
    let out = unsafe { out.as_mut() };
    let (Some(audio), Some(out)) = (audio, out) else {
        return 0;
    };
    let Some(config) = audio.config() else {
        return 0;
    };
    let Ok(block_frames) = u32::try_from(config.block_frames) else {
        return 0;
    };
    *out = NylonAudioConfig {
        device_id: config.device.0,
        sample_rate: config.sample_rate,
        block_frames,
        channels: u32::from(config.channels),
    };
    1
}

/// # Safety
/// Both handles must be live, belong to the calling control thread, and
/// the project must have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_audio_sync(
    audio: *mut AudioRuntime,
    project: *const Project,
) -> i32 {
    // SAFETY: The interface contract makes the audio handle exclusive.
    let audio = unsafe { audio.as_mut() };
    // SAFETY: The interface contract keeps the project live and immutable.
    let project = unsafe { project.as_ref() };
    let (Some(audio), Some(project)) = (audio, project) else {
        return 0;
    };
    i32::from(audio.sync_project(project))
}

/// # Safety
/// Both handles must be live, belong to the calling control thread, and
/// the project must have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_session_launch_clip(
    audio: *mut AudioRuntime,
    project: *const Project,
    track: u64,
    scene: u64,
    quantization_beats: f64,
) -> i32 {
    // SAFETY: The interface contract makes the audio handle exclusive.
    let audio = unsafe { audio.as_mut() };
    // SAFETY: The interface contract keeps the project live and immutable.
    let project = unsafe { project.as_ref() };
    let (Some(audio), Some(project), Ok(track), Ok(scene)) = (
        audio,
        project,
        usize::try_from(track),
        usize::try_from(scene),
    ) else {
        return 0;
    };
    i32::from(audio.launch_clip(project, track, scene, quantization_beats))
}

/// # Safety
/// Both handles must be live, belong to the calling control thread, and
/// the project must have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_session_launch_scene(
    audio: *mut AudioRuntime,
    project: *const Project,
    scene: u64,
    quantization_beats: f64,
) -> i32 {
    // SAFETY: The interface contract makes the audio handle exclusive.
    let audio = unsafe { audio.as_mut() };
    // SAFETY: The interface contract keeps the project live and immutable.
    let project = unsafe { project.as_ref() };
    let (Some(audio), Some(project), Ok(scene)) = (audio, project, usize::try_from(scene)) else {
        return 0;
    };
    i32::from(audio.launch_scene(project, scene, quantization_beats))
}

/// # Safety
/// Both handles must be live, belong to the calling control thread, and
/// the project must have no concurrent mutation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_session_stop_track(
    audio: *mut AudioRuntime,
    project: *const Project,
    track: u64,
) -> i32 {
    // SAFETY: The interface contract makes the audio handle exclusive.
    let audio = unsafe { audio.as_mut() };
    // SAFETY: The interface contract keeps the project live and immutable.
    let project = unsafe { project.as_ref() };
    let (Some(audio), Some(project), Ok(track)) = (audio, project, usize::try_from(track)) else {
        return 0;
    };
    i32::from(audio.stop_session_track(project, track))
}

/// Returns minus one when the track has no launched Session clip.
///
/// # Safety
/// The handle must be live with no concurrent access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_session_active_scene(audio: *const AudioRuntime, track: u64) -> i64 {
    // SAFETY: The interface contract keeps the handle live and immutable.
    let Some(audio) = (unsafe { audio.as_ref() }) else {
        return -1;
    };
    let Ok(track) = usize::try_from(track) else {
        return -1;
    };
    audio
        .active_session_scene(track)
        .and_then(|scene| i64::try_from(scene).ok())
        .unwrap_or(-1)
}

/// # Safety
/// The handle must be live with no concurrent access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_audio_dropouts(audio: *const AudioRuntime) -> u64 {
    // SAFETY: The interface contract keeps the handle live and immutable.
    unsafe { audio.as_ref() }.map_or(0, AudioRuntime::dropouts)
}

/// # Safety
/// The handle must be live with no concurrent access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_audio_frames_rendered(audio: *const AudioRuntime) -> u64 {
    // SAFETY: The interface contract keeps the handle live and immutable.
    unsafe { audio.as_ref() }.map_or(0, AudioRuntime::frames_rendered)
}

/// # Safety
/// The handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_transport_play(audio: *mut AudioRuntime) -> i32 {
    // SAFETY: The interface contract makes the handle exclusive.
    unsafe { audio.as_mut() }.map_or(0, |audio| i32::from(audio.play()))
}

/// # Safety
/// The handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_transport_stop(audio: *mut AudioRuntime) -> i32 {
    // SAFETY: The interface contract makes the handle exclusive.
    unsafe { audio.as_mut() }.map_or(0, |audio| i32::from(audio.stop()))
}

/// # Safety
/// The handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_transport_locate(audio: *mut AudioRuntime, beats: f64) -> i32 {
    // SAFETY: The interface contract makes the handle exclusive.
    unsafe { audio.as_mut() }.map_or(0, |audio| i32::from(audio.locate(beats)))
}

/// # Safety
/// The handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_transport_position_beats(audio: *mut AudioRuntime) -> f64 {
    // SAFETY: The interface contract makes the handle exclusive.
    unsafe { audio.as_mut() }.map_or(0.0, |audio| audio.state().position_beats)
}

/// # Safety
/// The handle must be live and exclusively accessible to this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_transport_is_playing(audio: *mut AudioRuntime) -> i32 {
    // SAFETY: The interface contract makes the handle exclusive.
    unsafe { audio.as_mut() }.map_or(0, |audio| i32::from(audio.state().playing))
}

/// # Safety
/// The handle must be live and exclusively accessible to this call. `out`
/// must point to one writable level record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_track_levels(
    audio: *mut AudioRuntime,
    index: u64,
    out: *mut NylonLevels,
) -> i32 {
    // SAFETY: The interface contract makes the handle exclusive.
    let audio = unsafe { audio.as_mut() };
    // SAFETY: The interface contract provides one writable record.
    let out = unsafe { out.as_mut() };
    let (Some(audio), Some(out)) = (audio, out) else {
        return 0;
    };
    let state = audio.state();
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    if index >= state.track_count {
        return 0;
    }
    *out = state.levels[index].into();
    1
}

/// # Safety
/// The handle must be live and exclusively accessible to this call. `out`
/// must point to one writable level record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_master_levels(
    audio: *mut AudioRuntime,
    out: *mut NylonLevels,
) -> i32 {
    // SAFETY: The interface contract makes the handle exclusive.
    let audio = unsafe { audio.as_mut() };
    // SAFETY: The interface contract provides one writable record.
    let out = unsafe { out.as_mut() };
    let (Some(audio), Some(out)) = (audio, out) else {
        return 0;
    };
    let state = audio.state();
    *out = state.levels[state.track_count].into();
    1
}

const MAX_PLUGIN_SCAN_ROOTS: u64 = 1_024;

/// # Safety
/// `roots` must point to `root_count` readable terminated UTF-8 strings. The
/// returned handle belongs to the calling control thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_scan(
    roots: *const *const c_char,
    root_count: u64,
) -> *mut PluginCatalog {
    if root_count > MAX_PLUGIN_SCAN_ROOTS || (root_count != 0 && roots.is_null()) {
        return std::ptr::null_mut();
    }
    let Ok(count) = usize::try_from(root_count) else {
        return std::ptr::null_mut();
    };
    let inputs: &[*const c_char] = if count == 0 {
        &[]
    } else {
        // SAFETY: The caller provides the pointer array described by the interface.
        unsafe { std::slice::from_raw_parts(roots, count) }
    };
    let mut paths = Vec::with_capacity(count);
    for input in inputs {
        // SAFETY: Each element follows the string contract above.
        let Some(path) = (unsafe { input_text(*input) }) else {
            return std::ptr::null_mut();
        };
        if path.is_empty() {
            return std::ptr::null_mut();
        }
        paths.push(PathBuf::from(path));
    }
    Box::into_raw(Box::new(PluginCatalog::scan(&paths)))
}

/// # Safety
/// `catalog` must be null or a handle returned by `nylon_plugin_catalog_scan`
/// that has not already been released.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_free(catalog: *mut PluginCatalog) {
    if !catalog.is_null() {
        // SAFETY: Ownership is transferred back by the interface contract.
        drop(unsafe { Box::from_raw(catalog) });
    }
}

/// # Safety
/// The catalog must remain live and immutable for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_entry_count(catalog: *const PluginCatalog) -> u64 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { catalog.as_ref() }.map_or(0, |value| value.entries().len() as u64)
}

/// # Safety
/// The catalog must remain live and immutable for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_issue_count(catalog: *const PluginCatalog) -> u64 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { catalog.as_ref() }.map_or(0, |value| value.issues().len() as u64)
}

/// # Safety
/// The catalog must remain live and immutable for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_entry_format(
    catalog: *const PluginCatalog,
    index: u64,
) -> i32 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { catalog.as_ref() }
        .and_then(|value| {
            usize::try_from(index)
                .ok()
                .and_then(|i| value.entries().get(i))
        })
        .map_or(-1, |entry| entry.format().code())
}

/// # Safety
/// The catalog must remain live and immutable for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_entry_state(
    catalog: *const PluginCatalog,
    index: u64,
) -> i32 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { catalog.as_ref() }
        .and_then(|value| {
            usize::try_from(index)
                .ok()
                .and_then(|i| value.entries().get(i))
        })
        .map_or(-1, |entry| entry.state().code())
}

fn plugin_entry_text(
    catalog: *const PluginCatalog,
    index: u64,
    select: impl FnOnce(&crate::plugin::Entry) -> String,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    // SAFETY: Native callers keep handles live and immutable during the call.
    let Some(entry) = (unsafe { catalog.as_ref() }).and_then(|value| {
        usize::try_from(index)
            .ok()
            .and_then(|i| value.entries().get(i))
    }) else {
        return copy_text("", buffer, capacity);
    };
    copy_text(&select(entry), buffer, capacity)
}

/// # Safety
/// The catalog must remain live and immutable. `buffer` must be null or point
/// to `capacity` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_entry_path(
    catalog: *const PluginCatalog,
    index: u64,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    plugin_entry_text(
        catalog,
        index,
        |entry| entry.path().to_string_lossy().into_owned(),
        buffer,
        capacity,
    )
}

/// # Safety
/// The catalog must remain live and immutable. `buffer` must be null or point
/// to `capacity` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_entry_name(
    catalog: *const PluginCatalog,
    index: u64,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    plugin_entry_text(
        catalog,
        index,
        |entry| entry.name().to_owned(),
        buffer,
        capacity,
    )
}

/// # Safety
/// The catalog must remain live and immutable. `buffer` must be null or point
/// to `capacity` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_entry_reason(
    catalog: *const PluginCatalog,
    index: u64,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    plugin_entry_text(
        catalog,
        index,
        |entry| entry.quarantine_reason().to_owned(),
        buffer,
        capacity,
    )
}

fn plugin_issue_text(
    catalog: *const PluginCatalog,
    index: u64,
    select: impl FnOnce(&crate::plugin::ScanIssue) -> String,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    // SAFETY: Native callers keep handles live and immutable during the call.
    let Some(issue) = (unsafe { catalog.as_ref() }).and_then(|value| {
        usize::try_from(index)
            .ok()
            .and_then(|i| value.issues().get(i))
    }) else {
        return copy_text("", buffer, capacity);
    };
    copy_text(&select(issue), buffer, capacity)
}

/// # Safety
/// The catalog must remain live and immutable. `buffer` must be null or point
/// to `capacity` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_issue_path(
    catalog: *const PluginCatalog,
    index: u64,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    plugin_issue_text(
        catalog,
        index,
        |issue| issue.path().to_string_lossy().into_owned(),
        buffer,
        capacity,
    )
}

/// # Safety
/// The catalog must remain live and immutable. `buffer` must be null or point
/// to `capacity` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_issue_message(
    catalog: *const PluginCatalog,
    index: u64,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    plugin_issue_text(
        catalog,
        index,
        |issue| issue.message().to_owned(),
        buffer,
        capacity,
    )
}

/// # Safety
/// The catalog must remain live and exclusive. `reason` must be a readable,
/// terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_quarantine(
    catalog: *mut PluginCatalog,
    index: u64,
    reason: *const c_char,
) -> i32 {
    // SAFETY: The caller provides a live exclusive handle.
    let Some(catalog) = (unsafe { catalog.as_mut() }) else {
        return 0;
    };
    // SAFETY: The caller provides the string described above.
    let Some(reason) = (unsafe { input_text(reason) }) else {
        return 0;
    };
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    i32::from(catalog.quarantine(index, &reason).is_ok())
}

/// # Safety
/// The catalog must remain live and exclusive for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_retry(
    catalog: *mut PluginCatalog,
    index: u64,
) -> i32 {
    // SAFETY: The caller provides a live exclusive handle.
    let Some(catalog) = (unsafe { catalog.as_mut() }) else {
        return 0;
    };
    let Ok(index) = usize::try_from(index) else {
        return 0;
    };
    i32::from(catalog.retry(index).is_ok())
}

/// # Safety
/// The catalog must remain live and exclusive. `bytes` must point to `length`
/// readable bytes, or be null when length is zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_apply_probe(
    catalog: *mut PluginCatalog,
    index: u64,
    bytes: *const u8,
    length: u64,
) -> i32 {
    // SAFETY: The caller provides a live exclusive handle.
    let Some(catalog) = (unsafe { catalog.as_mut() }) else {
        return 0;
    };
    let (Ok(index), Ok(length)) = (usize::try_from(index), usize::try_from(length)) else {
        return 0;
    };
    let bytes = if length == 0 {
        &[]
    } else {
        if bytes.is_null() {
            return 0;
        }
        // SAFETY: The caller provides the readable region described above.
        unsafe { std::slice::from_raw_parts(bytes, length) }
    };
    i32::from(catalog.apply_probe(index, bytes).is_ok())
}

fn with_plugin_descriptor<T>(
    catalog: *const PluginCatalog,
    entry: u64,
    descriptor: u64,
    select: impl FnOnce(&crate::plugin::probe::Descriptor) -> T,
) -> Option<T> {
    // SAFETY: Native callers keep the catalog live and immutable during access.
    let catalog = unsafe { catalog.as_ref() }?;
    let entry = usize::try_from(entry).ok()?;
    let descriptor = usize::try_from(descriptor).ok()?;
    let value = catalog
        .entries()
        .get(entry)?
        .descriptors()
        .get(descriptor)?;
    Some(select(value))
}

/// # Safety
/// The catalog must remain live and immutable for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_descriptor_count(
    catalog: *const PluginCatalog,
    entry: u64,
) -> u64 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { catalog.as_ref() }
        .and_then(|catalog| {
            usize::try_from(entry)
                .ok()
                .and_then(|entry| catalog.entries().get(entry))
        })
        .map_or(0, |entry| entry.descriptors().len() as u64)
}

fn plugin_descriptor_text(
    catalog: *const PluginCatalog,
    entry: u64,
    descriptor: u64,
    select: impl FnOnce(&crate::plugin::probe::Descriptor) -> &str,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    let Some(text) =
        with_plugin_descriptor(catalog, entry, descriptor, |value| select(value).to_owned())
    else {
        return copy_text("", buffer, capacity);
    };
    copy_text(&text, buffer, capacity)
}

macro_rules! plugin_descriptor_accessor {
    ($name:ident, $field:ident) => {
        /// Copies one descriptor field as UTF-8.
        ///
        /// # Safety
        /// The catalog must remain live and immutable. `buffer` must be null or
        /// point to `capacity` writable bytes.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            catalog: *const PluginCatalog,
            entry: u64,
            descriptor: u64,
            buffer: *mut c_char,
            capacity: u64,
        ) -> u64 {
            plugin_descriptor_text(
                catalog,
                entry,
                descriptor,
                |value| &value.$field,
                buffer,
                capacity,
            )
        }
    };
}

plugin_descriptor_accessor!(nylon_plugin_catalog_descriptor_id, id);
plugin_descriptor_accessor!(nylon_plugin_catalog_descriptor_name, name);
plugin_descriptor_accessor!(nylon_plugin_catalog_descriptor_vendor, vendor);
plugin_descriptor_accessor!(nylon_plugin_catalog_descriptor_version, version);

/// # Safety
/// The catalog must remain live and immutable for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_descriptor_feature_count(
    catalog: *const PluginCatalog,
    entry: u64,
    descriptor: u64,
) -> u64 {
    with_plugin_descriptor(catalog, entry, descriptor, |value| {
        value.features.len() as u64
    })
    .unwrap_or(0)
}

/// # Safety
/// The catalog must remain live and immutable. `buffer` must be null or point
/// to `capacity` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_plugin_catalog_descriptor_feature(
    catalog: *const PluginCatalog,
    entry: u64,
    descriptor: u64,
    feature: u64,
    buffer: *mut c_char,
    capacity: u64,
) -> u64 {
    let Some(feature) = with_plugin_descriptor(catalog, entry, descriptor, |value| {
        usize::try_from(feature)
            .ok()
            .and_then(|feature| value.features.get(feature))
            .cloned()
    })
    .flatten() else {
        return copy_text("", buffer, capacity);
    };
    copy_text(&feature, buffer, capacity)
}

/// # Safety
/// Both strings must be terminated and readable for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_open(
    path: *const c_char,
    identifier: *const c_char,
) -> *mut ClapInstance {
    if path.is_null() || identifier.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: The caller supplies terminated strings.
    let (Ok(path), Ok(identifier)) = (unsafe {
        (
            CStr::from_ptr(path).to_str(),
            CStr::from_ptr(identifier).to_str(),
        )
    }) else {
        return std::ptr::null_mut();
    };
    ClapInstance::open(std::path::Path::new(path), identifier)
        .map(Box::new)
        .map_or(std::ptr::null_mut(), Box::into_raw)
}

/// # Safety
/// The handle must be null or returned by `nylon_clap_instance_open` and not freed yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_free(instance: *mut ClapInstance) {
    if !instance.is_null() {
        // SAFETY: Ownership transfers back exactly once.
        drop(unsafe { Box::from_raw(instance) });
    }
}

/// # Safety
/// The handle must remain live and exclusive for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_activate(
    instance: *mut ClapInstance,
    sample_rate: f64,
    min_frames: u32,
    max_frames: u32,
) -> i32 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { instance.as_mut() }
        .is_some_and(|instance| {
            instance
                .activate(sample_rate, min_frames, max_frames)
                .is_ok()
        })
        .into()
}

/// # Safety
/// The handle must remain live and exclusive. Every non-null buffer must
/// contain `frames` samples. Input and output regions must not overlap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_process_stereo(
    instance: *mut ClapInstance,
    input_left: *const f32,
    input_right: *const f32,
    output_left: *mut f32,
    output_right: *mut f32,
    frames: u32,
) -> i32 {
    // SAFETY: Handle validity is required by the interface.
    let Some(instance) = (unsafe { instance.as_mut() }) else {
        return 0;
    };
    if output_left.is_null() || output_right.is_null() || frames == 0 {
        return 0;
    }
    let frames = frames as usize;
    let input = if input_left.is_null() && input_right.is_null() {
        None
    } else if input_left.is_null() || input_right.is_null() {
        return 0;
    } else {
        // SAFETY: The caller supplies two readable, non-overlapping regions.
        Some(unsafe {
            (
                std::slice::from_raw_parts(input_left, frames),
                std::slice::from_raw_parts(input_right, frames),
            )
        })
    };
    // SAFETY: The caller supplies two writable, non-overlapping regions.
    let (output_left, output_right) = unsafe {
        (
            std::slice::from_raw_parts_mut(output_left, frames),
            std::slice::from_raw_parts_mut(output_right, frames),
        )
    };
    instance
        .process_stereo(input, output_left, output_right)
        .is_ok()
        .into()
}

/// # Safety
/// The handle and audio buffers follow `nylon_clap_instance_process_stereo`.
/// `events` must be null when event_count is zero or point to event_count records.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_process_stereo_events(
    instance: *mut ClapInstance,
    input_left: *const f32,
    input_right: *const f32,
    output_left: *mut f32,
    output_right: *mut f32,
    frames: u32,
    events: *const ClapParameterEvent,
    event_count: u32,
) -> i32 {
    // SAFETY: Handle validity is required by the interface.
    let Some(instance) = (unsafe { instance.as_mut() }) else {
        return 0;
    };
    if output_left.is_null()
        || output_right.is_null()
        || frames == 0
        || (events.is_null() && event_count != 0)
    {
        return 0;
    }
    let frames = frames as usize;
    let input = if input_left.is_null() && input_right.is_null() {
        None
    } else if input_left.is_null() || input_right.is_null() {
        return 0;
    } else {
        // SAFETY: The caller supplies two readable regions containing frames samples.
        Some(unsafe {
            (
                std::slice::from_raw_parts(input_left, frames),
                std::slice::from_raw_parts(input_right, frames),
            )
        })
    };
    // SAFETY: Null is accepted only for an empty event slice.
    let events = if event_count == 0 {
        &[]
    } else {
        // SAFETY: The caller supplies event_count readable records.
        unsafe { std::slice::from_raw_parts(events, event_count as usize) }
    };
    // SAFETY: The caller supplies two writable regions containing frames samples.
    let (output_left, output_right) = unsafe {
        (
            std::slice::from_raw_parts_mut(output_left, frames),
            std::slice::from_raw_parts_mut(output_right, frames),
        )
    };
    instance
        .process_stereo_with_events(input, output_left, output_right, events)
        .is_ok()
        .into()
}

/// # Safety
/// The handle and audio buffers follow `nylon_clap_instance_process_stereo`.
/// Each event pointer must be null for a zero count or cover that many records.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_process_stereo_all_events(
    instance: *mut ClapInstance,
    input_left: *const f32,
    input_right: *const f32,
    output_left: *mut f32,
    output_right: *mut f32,
    frames: u32,
    parameter_events: *const ClapParameterEvent,
    parameter_event_count: u32,
    note_events: *const ClapNoteEvent,
    note_event_count: u32,
) -> i32 {
    // SAFETY: Handle validity is required by the interface.
    let Some(instance) = (unsafe { instance.as_mut() }) else {
        return 0;
    };
    if output_left.is_null()
        || output_right.is_null()
        || frames == 0
        || (parameter_events.is_null() && parameter_event_count != 0)
        || (note_events.is_null() && note_event_count != 0)
    {
        return 0;
    }
    let frames = frames as usize;
    let input = if input_left.is_null() && input_right.is_null() {
        None
    } else if input_left.is_null() || input_right.is_null() {
        return 0;
    } else {
        // SAFETY: The caller supplies two readable regions containing frames samples.
        Some(unsafe {
            (
                std::slice::from_raw_parts(input_left, frames),
                std::slice::from_raw_parts(input_right, frames),
            )
        })
    };
    let parameter_events = if parameter_event_count == 0 {
        &[]
    } else {
        // SAFETY: The caller supplies parameter_event_count readable records.
        unsafe { std::slice::from_raw_parts(parameter_events, parameter_event_count as usize) }
    };
    let note_events = if note_event_count == 0 {
        &[]
    } else {
        // SAFETY: The caller supplies note_event_count readable records.
        unsafe { std::slice::from_raw_parts(note_events, note_event_count as usize) }
    };
    // SAFETY: The caller supplies two writable regions containing frames samples.
    let (output_left, output_right) = unsafe {
        (
            std::slice::from_raw_parts_mut(output_left, frames),
            std::slice::from_raw_parts_mut(output_right, frames),
        )
    };
    instance
        .process_stereo_with_all_events(
            input,
            output_left,
            output_right,
            parameter_events,
            note_events,
        )
        .is_ok()
        .into()
}

/// # Safety
/// The handle must remain live for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_input_note_ports(
    instance: *const ClapInstance,
) -> u32 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { instance.as_ref() }.map_or(0, ClapInstance::input_note_ports)
}

/// # Safety
/// The handle must remain live for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_input_audio_ports(
    instance: *const ClapInstance,
) -> u32 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { instance.as_ref() }.map_or(0, ClapInstance::input_audio_ports)
}

/// # Safety
/// The handle must remain live for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_parameter_count(instance: *const ClapInstance) -> u64 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { instance.as_ref() }
        .and_then(|instance| instance.parameters().ok())
        .map_or(0, |parameters| parameters.len() as u64)
}

/// # Safety
/// The handle must remain live and info must point to one writable record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_parameter_info(
    instance: *const ClapInstance,
    index: u64,
    info: *mut NylonClapParameterInfo,
) -> i32 {
    // SAFETY: Handle and output validity are required by the interface.
    let (Some(instance), Some(info)) = (unsafe { instance.as_ref() }, unsafe { info.as_mut() })
    else {
        return 0;
    };
    let Some(parameter) = instance.parameters().ok().and_then(|parameters| {
        usize::try_from(index)
            .ok()
            .and_then(|index| parameters.get(index).cloned())
    }) else {
        return 0;
    };
    *info = NylonClapParameterInfo::default();
    info.identifier = parameter.identifier;
    info.flags = parameter.flags;
    info.minimum = parameter.minimum;
    info.maximum = parameter.maximum;
    info.default_value = parameter.default_value;
    write_fixed_text(&parameter.name, &mut info.name);
    write_fixed_text(&parameter.module, &mut info.module);
    1
}

/// # Safety
/// The handle must remain live and value must point to writable storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_parameter_value(
    instance: *const ClapInstance,
    identifier: u32,
    value: *mut f64,
) -> i32 {
    // SAFETY: Handle and output validity are required by the interface.
    let (Some(instance), Some(value)) = (unsafe { instance.as_ref() }, unsafe { value.as_mut() })
    else {
        return 0;
    };
    instance
        .parameter_value(identifier)
        .map(|current| *value = current)
        .is_ok()
        .into()
}

/// # Safety
/// The handle must remain live and frames must point to writable storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_latency(
    instance: *const ClapInstance,
    frames: *mut u32,
) -> i32 {
    // SAFETY: Handle and output validity are required by the interface.
    let (Some(instance), Some(frames)) = (unsafe { instance.as_ref() }, unsafe { frames.as_mut() })
    else {
        return 0;
    };
    instance
        .latency_frames()
        .map(|latency| *frames = latency)
        .is_ok()
        .into()
}

/// # Safety
/// The handle must remain live for this call. The returned state belongs to
/// the caller and must be released with `nylon_clap_state_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_save_state(
    instance: *const ClapInstance,
) -> *mut Vec<u8> {
    // SAFETY: Handle validity is required by the interface.
    unsafe { instance.as_ref() }
        .and_then(|instance| instance.save_state().ok())
        .map(Box::new)
        .map_or(std::ptr::null_mut(), Box::into_raw)
}

/// # Safety
/// The instance must remain live and exclusive. Bytes must be null when length
/// is zero or point to length readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_load_state(
    instance: *mut ClapInstance,
    bytes: *const u8,
    length: u64,
) -> i32 {
    // SAFETY: Handle validity is required by the interface.
    let Some(instance) = (unsafe { instance.as_mut() }) else {
        return 0;
    };
    let Ok(length) = usize::try_from(length) else {
        return 0;
    };
    if bytes.is_null() && length != 0 {
        return 0;
    }
    let bytes = if length == 0 {
        &[]
    } else {
        // SAFETY: The caller supplies length readable bytes.
        unsafe { std::slice::from_raw_parts(bytes, length) }
    };
    instance.load_state(bytes).is_ok().into()
}

/// # Safety
/// The handle must be null or an owned state returned by the save function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_state_free(state: *mut Vec<u8>) {
    if !state.is_null() {
        // SAFETY: Ownership transfers back exactly once.
        drop(unsafe { Box::from_raw(state) });
    }
}

/// # Safety
/// The state must remain live for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_state_size(state: *const Vec<u8>) -> u64 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { state.as_ref() }.map_or(0, |state| state.len() as u64)
}

/// # Safety
/// The state must remain live until the returned region is no longer read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_state_data(state: *const Vec<u8>) -> *const u8 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { state.as_ref() }.map_or(std::ptr::null(), |state| state.as_ptr())
}

/// # Safety
/// The handle must remain live and exclusive for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_reset(instance: *mut ClapInstance) -> i32 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { instance.as_mut() }
        .is_some_and(|instance| instance.reset().is_ok())
        .into()
}

/// # Safety
/// The handle must remain live for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_instance_take_requests(instance: *const ClapInstance) -> u32 {
    // SAFETY: Handle validity is required by the interface.
    let Some(instance) = (unsafe { instance.as_ref() }) else {
        return 0;
    };
    let requests = instance.take_requests();
    u32::from(requests.restart)
        | (u32::from(requests.process) << 1)
        | (u32::from(requests.callback) << 2)
        | (u32::from(requests.parameter_rescan) << 3)
        | (u32::from(requests.parameter_clear) << 4)
        | (u32::from(requests.parameter_flush) << 5)
        | (u32::from(requests.latency_changed) << 6)
        | (u32::from(requests.state_dirty) << 7)
        | (u32::from(requests.note_ports_changed) << 8)
}

/// # Safety
/// All strings must be terminated and readable for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_worker_open(
    executable: *const c_char,
    path: *const c_char,
    identifier: *const c_char,
    sample_rate: f64,
    max_frames: u32,
) -> *mut ClapWorker {
    if executable.is_null() || path.is_null() || identifier.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: The caller supplies terminated strings.
    let (Ok(executable), Ok(path), Ok(identifier)) = (unsafe {
        (
            CStr::from_ptr(executable).to_str(),
            CStr::from_ptr(path).to_str(),
            CStr::from_ptr(identifier).to_str(),
        )
    }) else {
        return std::ptr::null_mut();
    };
    ClapWorker::spawn(
        executable,
        std::path::Path::new(path),
        identifier,
        sample_rate,
        max_frames as usize,
    )
    .map(Box::new)
    .map_or(std::ptr::null_mut(), Box::into_raw)
}

/// # Safety
/// The handle must be null or returned by `nylon_clap_worker_open` and not freed yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_worker_free(worker: *mut ClapWorker) {
    if !worker.is_null() {
        // SAFETY: Ownership transfers back exactly once.
        drop(unsafe { Box::from_raw(worker) });
    }
}

/// # Safety
/// The handle and buffers must remain live and exclusive. Audio regions contain
/// `frames` values. Event regions contain the corresponding number of records.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_worker_process_stereo(
    worker: *mut ClapWorker,
    input_left: *const f32,
    input_right: *const f32,
    output_left: *mut f32,
    output_right: *mut f32,
    frames: u32,
    parameter_events: *const ClapParameterEvent,
    parameter_event_count: u32,
    note_events: *const ClapNoteEvent,
    note_event_count: u32,
) -> i32 {
    // SAFETY: Handle validity is required by the interface.
    let Some(worker) = (unsafe { worker.as_mut() }) else {
        return 0;
    };
    if output_left.is_null()
        || output_right.is_null()
        || frames == 0
        || (input_left.is_null() != input_right.is_null())
        || (parameter_events.is_null() && parameter_event_count != 0)
        || (note_events.is_null() && note_event_count != 0)
    {
        return 0;
    }
    let frames = frames as usize;
    let input = if input_left.is_null() {
        None
    } else {
        // SAFETY: The caller supplies two readable regions containing frames samples.
        Some(unsafe {
            (
                std::slice::from_raw_parts(input_left, frames),
                std::slice::from_raw_parts(input_right, frames),
            )
        })
    };
    let parameter_events = if parameter_event_count == 0 {
        &[]
    } else {
        // SAFETY: The caller supplies parameter_event_count readable records.
        unsafe { std::slice::from_raw_parts(parameter_events, parameter_event_count as usize) }
    };
    let note_events = if note_event_count == 0 {
        &[]
    } else {
        // SAFETY: The caller supplies note_event_count readable records.
        unsafe { std::slice::from_raw_parts(note_events, note_event_count as usize) }
    };
    // SAFETY: The caller supplies two writable regions containing frames samples.
    let (output_left, output_right) = unsafe {
        (
            std::slice::from_raw_parts_mut(output_left, frames),
            std::slice::from_raw_parts_mut(output_right, frames),
        )
    };
    worker
        .process_stereo(
            input,
            output_left,
            output_right,
            parameter_events,
            note_events,
        )
        .is_ok()
        .into()
}

/// # Safety
/// The handle must remain live for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_worker_input_note_ports(worker: *const ClapWorker) -> u32 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { worker.as_ref() }.map_or(0, ClapWorker::input_note_ports)
}

/// # Safety
/// The handle must remain live for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_worker_input_audio_ports(worker: *const ClapWorker) -> u32 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { worker.as_ref() }.map_or(0, ClapWorker::input_audio_ports)
}

/// # Safety
/// The handle must remain live for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_worker_parameter_count(worker: *const ClapWorker) -> u64 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { worker.as_ref() }.map_or(0, |worker| worker.parameters().len() as u64)
}

/// # Safety
/// The handle must remain live and info must point to one writable record.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_worker_parameter_info(
    worker: *const ClapWorker,
    index: u64,
    info: *mut NylonClapParameterInfo,
) -> i32 {
    // SAFETY: Handle and output validity are required by the interface.
    let (Some(worker), Some(info)) = (unsafe { worker.as_ref() }, unsafe { info.as_mut() }) else {
        return 0;
    };
    let Some(parameter) = usize::try_from(index)
        .ok()
        .and_then(|index| worker.parameters().get(index))
    else {
        return 0;
    };
    *info = NylonClapParameterInfo::default();
    info.identifier = parameter.identifier;
    info.flags = parameter.flags;
    info.minimum = parameter.minimum;
    info.maximum = parameter.maximum;
    info.default_value = parameter.default_value;
    write_fixed_text(&parameter.name, &mut info.name);
    write_fixed_text(&parameter.module, &mut info.module);
    1
}

/// # Safety
/// The handle must remain live and frames must point to writable storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_worker_latency(
    worker: *const ClapWorker,
    frames: *mut u32,
) -> i32 {
    // SAFETY: Handle and output validity are required by the interface.
    let (Some(worker), Some(frames)) = (unsafe { worker.as_ref() }, unsafe { frames.as_mut() })
    else {
        return 0;
    };
    *frames = worker.latency_frames();
    1
}

/// # Safety
/// The handle must remain live and exclusive. The returned state belongs to
/// the caller and must be released with `nylon_clap_state_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_worker_save_state(worker: *mut ClapWorker) -> *mut Vec<u8> {
    // SAFETY: Handle validity is required by the interface.
    unsafe { worker.as_mut() }
        .and_then(|worker| worker.save_state().ok())
        .map(Box::new)
        .map_or(std::ptr::null_mut(), Box::into_raw)
}

/// # Safety
/// The handle must remain live and exclusive. Bytes must be null when length
/// is zero or point to length readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_worker_load_state(
    worker: *mut ClapWorker,
    bytes: *const u8,
    length: u64,
) -> i32 {
    // SAFETY: Handle validity is required by the interface.
    let Some(worker) = (unsafe { worker.as_mut() }) else {
        return 0;
    };
    let Ok(length) = usize::try_from(length) else {
        return 0;
    };
    if bytes.is_null() && length != 0 {
        return 0;
    }
    let bytes = if length == 0 {
        &[]
    } else {
        // SAFETY: The caller supplies length readable bytes.
        unsafe { std::slice::from_raw_parts(bytes, length) }
    };
    worker.load_state(bytes).is_ok().into()
}

/// # Safety
/// All strings must be terminated and readable for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_bridge_open(
    executable: *const c_char,
    path: *const c_char,
    identifier: *const c_char,
    sample_rate: f64,
    frames: u32,
    queue_depth: u32,
) -> *mut ClapBridge {
    if executable.is_null() || path.is_null() || identifier.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: The caller supplies terminated strings.
    let (Ok(executable), Ok(path), Ok(identifier)) = (unsafe {
        (
            CStr::from_ptr(executable).to_str(),
            CStr::from_ptr(path).to_str(),
            CStr::from_ptr(identifier).to_str(),
        )
    }) else {
        return std::ptr::null_mut();
    };
    let Ok(client) = ClapWorker::spawn(
        executable,
        std::path::Path::new(path),
        identifier,
        sample_rate,
        frames as usize,
    ) else {
        return std::ptr::null_mut();
    };
    ClapBridge::from_client(client, frames as usize, queue_depth as usize)
        .map(Box::new)
        .map_or(std::ptr::null_mut(), Box::into_raw)
}

/// # Safety
/// The handle must be null or returned by `nylon_clap_bridge_open` and not freed yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_bridge_free(bridge: *mut ClapBridge) {
    if !bridge.is_null() {
        // SAFETY: Ownership transfers back exactly once.
        drop(unsafe { Box::from_raw(bridge) });
    }
}

/// # Safety
/// The handle and buffers must remain live and exclusive. Audio regions contain
/// `frames` values. Event regions contain the corresponding number of records.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_bridge_process_stereo(
    bridge: *mut ClapBridge,
    input_left: *const f32,
    input_right: *const f32,
    output_left: *mut f32,
    output_right: *mut f32,
    frames: u32,
    parameter_events: *const ClapParameterEvent,
    parameter_event_count: u32,
    note_events: *const ClapNoteEvent,
    note_event_count: u32,
) -> i32 {
    // SAFETY: Handle validity is required by the interface.
    let Some(bridge) = (unsafe { bridge.as_mut() }) else {
        return 0;
    };
    if output_left.is_null()
        || output_right.is_null()
        || frames == 0
        || (input_left.is_null() != input_right.is_null())
        || (parameter_events.is_null() && parameter_event_count != 0)
        || (note_events.is_null() && note_event_count != 0)
    {
        return 0;
    }
    let frames = frames as usize;
    let input = if input_left.is_null() {
        None
    } else {
        // SAFETY: The caller supplies two readable regions containing frames samples.
        Some(unsafe {
            (
                std::slice::from_raw_parts(input_left, frames),
                std::slice::from_raw_parts(input_right, frames),
            )
        })
    };
    let parameter_events = if parameter_event_count == 0 {
        &[]
    } else {
        // SAFETY: The caller supplies parameter_event_count readable records.
        unsafe { std::slice::from_raw_parts(parameter_events, parameter_event_count as usize) }
    };
    let note_events = if note_event_count == 0 {
        &[]
    } else {
        // SAFETY: The caller supplies note_event_count readable records.
        unsafe { std::slice::from_raw_parts(note_events, note_event_count as usize) }
    };
    // SAFETY: The caller supplies two writable regions containing frames samples.
    let (output_left, output_right) = unsafe {
        (
            std::slice::from_raw_parts_mut(output_left, frames),
            std::slice::from_raw_parts_mut(output_right, frames),
        )
    };
    bridge
        .process_stereo(
            input,
            output_left,
            output_right,
            parameter_events,
            note_events,
        )
        .is_ok()
        .into()
}

/// # Safety
/// The handle must remain live for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_bridge_latency(bridge: *const ClapBridge) -> u32 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { bridge.as_ref() }.map_or(0, ClapBridge::latency_frames)
}

/// # Safety
/// The handle must remain live for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nylon_clap_bridge_is_running(bridge: *const ClapBridge) -> i32 {
    // SAFETY: Handle validity is required by the interface.
    unsafe { bridge.as_ref() }
        .is_some_and(ClapBridge::is_running)
        .into()
}

macro_rules! bridge_counter {
    ($name:ident, $method:ident) => {
        /// Returns one bridge health counter, or zero for a null handle.
        ///
        /// # Safety
        /// The handle must remain live for this call.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(bridge: *const ClapBridge) -> u64 {
            // SAFETY: Handle validity is required by the interface.
            unsafe { bridge.as_ref() }.map_or(0, ClapBridge::$method)
        }
    };
}

bridge_counter!(nylon_clap_bridge_submitted_blocks, submitted_blocks);
bridge_counter!(nylon_clap_bridge_completed_blocks, completed_blocks);
bridge_counter!(nylon_clap_bridge_underruns, underruns);
bridge_counter!(nylon_clap_bridge_queue_drops, queue_drops);
bridge_counter!(nylon_clap_bridge_worker_failures, worker_failures);
