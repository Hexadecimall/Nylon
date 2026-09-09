use nylon::dsp::chorus::Parameters as ChorusParameters;
use nylon::dsp::gate::Parameters as GateParameters;
use nylon::dsp::limiter::Parameters as LimiterParameters;
use nylon::dsp::reverb::Parameters as ReverbParameters;
use nylon::dsp::saturator::{
    Curve as SaturatorCurve, Oversampling as SaturatorOversampling,
    Parameters as SaturatorParameters,
};
use nylon::engine::device::{DeviceConfig, DeviceKind};
use nylon::engine::voice::Patch;
use nylon::persistence::PersistenceError;
use nylon::project::{
    AutomationCurve, AutomationParameter, AutomationPoint, Command, MidiNote, Project, TrackKind,
};
use nylon::routing::EdgeKind;

fn session() -> Project {
    let mut project = Project::new();
    project
        .apply(&[
            Command::CreateTrack {
                name: "Synth".into(),
                kind: TrackKind::Midi,
            },
            Command::CreateTrack {
                name: "Recording".into(),
                kind: TrackKind::Audio,
            },
        ])
        .unwrap();
    let id = project.snapshot().tracks()[0].id();
    project
        .apply(&[
            Command::SetTrackVolume { id, db: -8.0 },
            Command::SetTrackPan { id, pan: 0.25 },
            Command::SetTrackArm { id, enabled: true },
            Command::SetInstrumentPatch {
                id,
                patch: Patch {
                    oscillator_mix: 0.7,
                    sub_level: 0.35,
                    noise_level: 0.05,
                    unison_voices: 3,
                    unison_detune_cents: 16.0,
                    cutoff: 3_200.0,
                    resonance: 1.2,
                    level_db: -10.0,
                    ..Patch::default()
                },
            },
            Command::SetAutomation {
                track: id,
                parameter: AutomationParameter::Volume,
                points: vec![
                    AutomationPoint {
                        beat: 0.0,
                        value: -12.0,
                        curve: AutomationCurve::Linear,
                    },
                    AutomationPoint {
                        beat: 8.0,
                        value: -3.0,
                        curve: AutomationCurve::Smooth,
                    },
                ],
            },
            Command::AddDevice {
                track: id,
                config: DeviceConfig {
                    enabled: true,
                    kind: DeviceKind::StereoDelay {
                        delay_seconds: 0.375,
                        feedback: 0.45,
                        mix: 0.2,
                    },
                },
            },
            Command::AddDevice {
                track: id,
                config: DeviceConfig {
                    enabled: true,
                    kind: DeviceKind::Gate {
                        parameters: GateParameters {
                            threshold_db: -32.0,
                            hysteresis_db: 8.0,
                            attack_seconds: 0.002,
                            hold_seconds: 0.04,
                            release_seconds: 0.15,
                            external_sidechain: true,
                        },
                    },
                },
            },
            Command::AddDevice {
                track: id,
                config: DeviceConfig {
                    enabled: true,
                    kind: DeviceKind::Saturator {
                        parameters: SaturatorParameters {
                            drive_db: 9.0,
                            output_db: -4.0,
                            mix: 0.75,
                            curve: SaturatorCurve::Diode,
                            oversampling: SaturatorOversampling::Four,
                            dc_filter: true,
                        },
                    },
                },
            },
            Command::AddDevice {
                track: id,
                config: DeviceConfig {
                    enabled: true,
                    kind: DeviceKind::Limiter {
                        parameters: LimiterParameters {
                            ceiling_db: -0.5,
                            release_seconds: 0.125,
                            lookahead_seconds: 0.004,
                        },
                    },
                },
            },
            Command::AddDevice {
                track: id,
                config: DeviceConfig {
                    enabled: true,
                    kind: DeviceKind::Chorus {
                        parameters: ChorusParameters::default(),
                    },
                },
            },
            Command::AddDevice {
                track: id,
                config: DeviceConfig {
                    enabled: true,
                    kind: DeviceKind::Reverb {
                        parameters: ReverbParameters::default(),
                    },
                },
            },
            Command::SetTimeSignature {
                numerator: 7,
                denominator: 8,
            },
        ])
        .unwrap();
    project
        .apply(&[Command::CreateScene {
            name: "Verse".into(),
        }])
        .unwrap();
    let snapshot = project.snapshot();
    let scene = snapshot.scenes()[0].id();
    project
        .apply(&[Command::CreateMidiClip {
            track: id,
            scene,
            name: "Part".into(),
            length_beats: 7.0,
        }])
        .unwrap();
    let clip = project.snapshot().clip_at(0, 0).unwrap().id();
    project
        .apply(&[
            Command::AddNote {
                id: clip,
                note: MidiNote {
                    pitch: 64,
                    velocity: 105,
                    start_beats: 1.5,
                    length_beats: 0.5,
                },
            },
            Command::PlaceClip {
                track: id,
                clip,
                start_beats: 14.0,
                length_beats: 7.0,
            },
        ])
        .unwrap();
    let audio_track = project.snapshot().tracks()[1].id();
    project
        .apply(&[
            Command::SetTrackLatency { id, frames: 384 },
            Command::CreateRoute {
                source: id,
                destination: audio_track,
                kind: EdgeKind::Sidechain,
                gain: 0.75,
            },
        ])
        .unwrap();
    project
        .apply(&[Command::CreateAudioClip {
            track: audio_track,
            scene,
            name: "Take".into(),
            media_path: "Media/take.wav".into(),
            length_beats: 7.0,
            source_tempo: 128.0,
        }])
        .unwrap();
    let audio = project.snapshot().audio_clip_at(1, 0).unwrap().id();
    project
        .apply(&[
            Command::SetAudioClipGain {
                id: audio,
                db: -3.0,
            },
            Command::SetAudioClipReverse {
                id: audio,
                enabled: true,
            },
            Command::SetAudioClipWarp {
                id: audio,
                enabled: true,
                source_tempo: 126.0,
            },
            Command::PlaceClip {
                track: audio_track,
                clip: audio,
                start_beats: 21.0,
                length_beats: 7.0,
            },
        ])
        .unwrap();
    project.apply(&[Command::SetTempo(135.0)]).unwrap();
    project.undo();
    project
}

#[test]
fn project_and_both_history_branches_round_trip_exactly() {
    let mut original = session();
    let bytes = original.to_bytes().unwrap();
    let mut loaded = Project::from_bytes(&bytes).unwrap();
    assert_eq!(bytes, loaded.to_bytes().unwrap());
    assert_eq!(*original.snapshot(), *loaded.snapshot());
    assert!(original.redo() && loaded.redo());
    assert_eq!(*original.snapshot(), *loaded.snapshot());
    while original.can_undo() {
        assert!(original.undo() && loaded.undo());
        assert_eq!(*original.snapshot(), *loaded.snapshot());
    }
    assert!(!loaded.can_undo());
}

#[test]
fn every_truncation_and_single_bit_corruption_is_rejected() {
    let bytes = session().to_bytes().unwrap();
    for end in 0..bytes.len() {
        assert!(Project::from_bytes(&bytes[..end]).is_err());
    }
    for index in 0..bytes.len() {
        for bit in 0..8 {
            let mut broken = bytes.clone();
            broken[index] ^= 1 << bit;
            assert!(Project::from_bytes(&broken).is_err());
        }
    }
    let mut future = bytes.clone();
    future[4] = 13;
    assert!(matches!(
        Project::from_bytes(&future),
        Err(PersistenceError::UnsupportedVersion)
    ));
    let mut trailing = bytes;
    trailing.push(0);
    assert!(Project::from_bytes(&trailing).is_err());
}

#[test]
fn version_one_empty_project_migrates_without_data_loss() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"NYLN");
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&120_f64.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&48000_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    let mut checksum = !0_u32;
    for byte in &bytes {
        checksum ^= u32::from(*byte);
        for _ in 0..8 {
            checksum = (checksum >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(checksum & 1));
        }
    }
    bytes.extend_from_slice(&(!checksum).to_le_bytes());
    let loaded = Project::from_bytes(&bytes).unwrap();
    assert_eq!(loaded.snapshot().tempo(), 120.0);
    assert!(loaded.snapshot().tracks().is_empty());
    assert!(loaded.snapshot().scenes().is_empty());
}

#[test]
fn version_two_empty_project_migrates_without_data_loss() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"NYLN");
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&120_f64.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&48000_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    let mut checksum = !0_u32;
    for byte in &bytes {
        checksum ^= u32::from(*byte);
        for _ in 0..8 {
            checksum = (checksum >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(checksum & 1));
        }
    }
    bytes.extend_from_slice(&(!checksum).to_le_bytes());
    let loaded = Project::from_bytes(&bytes).unwrap();
    assert_eq!(loaded.snapshot().tempo(), 120.0);
    assert!(loaded.snapshot().tracks().is_empty());
    assert!(loaded.snapshot().scenes().is_empty());
    assert!(loaded.snapshot().clips().is_empty());
    assert!(loaded.snapshot().audio_clips().is_empty());
}

#[test]
fn version_nine_midi_tracks_receive_the_default_instrument() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"NYLN");
    bytes.extend_from_slice(&9_u32.to_le_bytes());
    bytes.extend_from_slice(&10_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&120_f64.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&9_u64.to_le_bytes());
    bytes.extend_from_slice(&[1, 0, 0]);
    bytes.extend_from_slice(&0_f64.to_le_bytes());
    bytes.extend_from_slice(&0_f64.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&5_u32.to_le_bytes());
    bytes.extend_from_slice(b"Synth");
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    let mut checksum = !0_u32;
    for byte in &bytes {
        checksum ^= u32::from(*byte);
        for _ in 0..8 {
            checksum = (checksum >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(checksum & 1));
        }
    }
    bytes.extend_from_slice(&(!checksum).to_le_bytes());
    let loaded = Project::from_bytes(&bytes).unwrap();
    assert_eq!(
        loaded.snapshot().tracks()[0].instrument_patch(),
        Patch::default()
    );
}

#[test]
fn bundle_save_replaces_the_document_without_leaving_temporary_files() {
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::path::PathBuf::from("target").join(format!("save-{tick}"));
    let mut project = Project::new();
    project.apply(&[Command::SetTempo(129.0)]).unwrap();
    project.save_bundle(&directory).unwrap();
    assert_eq!(
        *Project::load_bundle(&directory).unwrap().snapshot(),
        *project.snapshot()
    );
    project.apply(&[Command::SetTempo(111.0)]).unwrap();
    project.save_bundle(&directory).unwrap();
    assert_eq!(
        Project::load_bundle(&directory).unwrap().snapshot().tempo(),
        111.0
    );
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn autosave_preserves_the_saved_document_until_recovery_is_selected() {
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::path::PathBuf::from("target").join(format!("recovery-{tick}"));
    let mut project = Project::new();
    project.save_bundle(&directory).unwrap();
    assert!(!project.is_modified());
    assert!(!Project::recovery_available(&directory));

    project.apply(&[Command::SetTempo(147.0)]).unwrap();
    assert!(project.is_modified());
    project.autosave().unwrap();
    assert!(Project::recovery_available(&directory));
    assert_eq!(
        Project::load_bundle(&directory).unwrap().snapshot().tempo(),
        120.0
    );

    let mut recovered = Project::recover_bundle(&directory).unwrap();
    assert_eq!(recovered.snapshot().tempo(), 147.0);
    assert!(recovered.is_modified());
    recovered.save_bundle(&directory).unwrap();
    assert!(!recovered.is_modified());
    assert!(!Project::recovery_available(&directory));
    assert_eq!(
        Project::load_bundle(&directory).unwrap().snapshot().tempo(),
        147.0
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn recovery_rejects_corruption_and_unsaved_projects_cannot_autosave() {
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::path::PathBuf::from("target").join(format!("bad-recovery-{tick}"));
    let project = Project::new();
    assert_eq!(project.autosave(), Err(PersistenceError::Io));

    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(".autosave.nylon"), b"damaged").unwrap();
    assert!(!Project::recovery_available(&directory));
    assert!(Project::recover_bundle(&directory).is_err());
    Project::discard_recovery(&directory).unwrap();
    assert!(!directory.join(".autosave.nylon").exists());
    std::fs::remove_dir_all(directory).unwrap();
}
