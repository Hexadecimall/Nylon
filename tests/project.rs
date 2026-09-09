use nylon::engine::device::{DeviceConfig, DeviceKind};
use nylon::engine::voice::Patch;
use nylon::plugin::Format as PluginFormat;
use nylon::project::{
    AutomationCurve, AutomationParameter, AutomationPoint, Command, MidiNote, PluginDevice,
    PluginParameterValue, Project, ProjectError, TrackKind,
};
use nylon::routing::EdgeKind;

#[test]
fn grouped_commands_undo_as_one_and_snapshots_stay_immutable() {
    let mut project = Project::new();
    let original = project.snapshot();
    project
        .apply(&[
            Command::CreateTrack {
                name: "Bass".into(),
                kind: TrackKind::Midi,
            },
            Command::SetTempo(128.0),
        ])
        .unwrap();
    let changed = project.snapshot();
    assert!(original.tracks().is_empty());
    assert_eq!(changed.tracks()[0].name(), "Bass");
    assert_eq!(changed.tempo(), 128.0);
    assert!(project.undo());
    assert!(project.snapshot().tracks().is_empty());
    assert_eq!(project.snapshot().tempo(), 120.0);
    assert!(project.redo());
    assert_eq!(*project.snapshot(), *changed);
}

#[test]
fn automation_lanes_are_validated_replaced_and_undoable() {
    let mut project = Project::new();
    project
        .apply(&[Command::CreateTrack {
            name: "Automation".into(),
            kind: TrackKind::Audio,
        }])
        .unwrap();
    let track = project.snapshot().tracks()[0].id();
    let points = vec![
        AutomationPoint {
            beat: 0.0,
            value: -12.0,
            curve: AutomationCurve::Linear,
        },
        AutomationPoint {
            beat: 4.0,
            value: 0.0,
            curve: AutomationCurve::Smooth,
        },
    ];
    project
        .apply(&[Command::SetAutomation {
            track,
            parameter: AutomationParameter::Volume,
            points: points.clone(),
        }])
        .unwrap();
    let snapshot = project.snapshot();
    assert_eq!(snapshot.tracks()[0].automation()[0].points(), points);

    let replacement = vec![AutomationPoint {
        beat: 2.0,
        value: -3.0,
        curve: AutomationCurve::Step,
    }];
    project
        .apply(&[Command::SetAutomation {
            track,
            parameter: AutomationParameter::Volume,
            points: replacement.clone(),
        }])
        .unwrap();
    assert_eq!(
        project.snapshot().tracks()[0].automation()[0].points(),
        replacement
    );
    assert!(project.undo());
    assert_eq!(
        project.snapshot().tracks()[0].automation()[0].points(),
        points
    );

    for invalid in [
        vec![],
        vec![AutomationPoint {
            beat: -1.0,
            value: 0.0,
            curve: AutomationCurve::Linear,
        }],
        vec![
            AutomationPoint {
                beat: 1.0,
                value: 0.0,
                curve: AutomationCurve::Linear,
            },
            AutomationPoint {
                beat: 1.0,
                value: 1.0,
                curve: AutomationCurve::Linear,
            },
        ],
    ] {
        assert_eq!(
            project.apply(&[Command::SetAutomation {
                track,
                parameter: AutomationParameter::Pan,
                points: invalid,
            }]),
            Err(ProjectError::InvalidAutomation)
        );
    }
    assert_eq!(
        project.apply(&[Command::SetAutomation {
            track,
            parameter: AutomationParameter::Mute,
            points: vec![AutomationPoint {
                beat: 0.0,
                value: 1.0,
                curve: AutomationCurve::Linear,
            }],
        }]),
        Err(ProjectError::InvalidAutomation)
    );
    project
        .apply(&[Command::ClearAutomation {
            track,
            parameter: AutomationParameter::Volume,
        }])
        .unwrap();
    assert!(project.snapshot().tracks()[0].automation().is_empty());
}

#[test]
fn failed_transaction_does_not_publish_or_destroy_redo() {
    let mut project = Project::new();
    project.apply(&[Command::SetTempo(150.0)]).unwrap();
    project.undo();
    assert_eq!(
        project.apply(&[
            Command::CreateTrack {
                name: "Audio".into(),
                kind: TrackKind::Audio
            },
            Command::SetTempo(f64::NAN),
        ]),
        Err(ProjectError::InvalidTempo)
    );
    assert!(project.snapshot().tracks().is_empty());
    assert!(project.redo());
    assert_eq!(project.snapshot().tempo(), 150.0);
}

#[test]
fn identifiers_are_never_reused_after_undo_or_delete() {
    let mut project = Project::new();
    let create = Command::CreateTrack {
        name: "Track".into(),
        kind: TrackKind::Audio,
    };
    project.apply(std::slice::from_ref(&create)).unwrap();
    let first = project.snapshot().tracks()[0].id();
    project.undo();
    project.apply(&[create]).unwrap();
    let second = project.snapshot().tracks()[0].id();
    assert_ne!(first, second);
    assert!(!project.redo());
    assert_eq!(
        project.apply(&[Command::DeleteTrack(first)]),
        Err(ProjectError::MissingTrack)
    );
    project
        .apply(&[Command::RenameTrack {
            id: second,
            name: "Lead".into(),
        }])
        .unwrap();
    assert_eq!(project.snapshot().tracks()[0].name(), "Lead");
    project.apply(&[Command::DeleteTrack(second)]).unwrap();
    assert!(project.snapshot().tracks().is_empty());
}

#[test]
fn empty_transaction_preserves_history_and_names_are_validated() {
    let mut project = Project::new();
    project.apply(&[]).unwrap();
    assert!(!project.undo());
    for name in ["", " ", "line\nbreak"] {
        assert_eq!(
            project.apply(&[Command::CreateTrack {
                name: name.into(),
                kind: TrackKind::Audio
            }]),
            Err(ProjectError::InvalidName)
        );
    }
}

#[test]
fn audio_clips_are_undoable_and_reject_unsafe_media_paths() {
    let mut project = Project::new();
    project
        .apply(&[Command::CreateTrack {
            name: "Audio".into(),
            kind: TrackKind::Audio,
        }])
        .unwrap();
    let snapshot = project.snapshot();
    let track = snapshot.tracks()[0].id();
    let scene = snapshot.scenes()[0].id();
    project
        .apply(&[Command::CreateAudioClip {
            track,
            scene,
            name: "Take".into(),
            media_path: "Media/take.wav".into(),
            length_beats: 8.0,
            source_tempo: 120.0,
        }])
        .unwrap();
    let clip = project.snapshot().audio_clip_at(0, 0).unwrap().id();
    project
        .apply(&[
            Command::SetClipName {
                id: clip,
                name: "Edited".into(),
            },
            Command::SetClipColor { id: clip, index: 7 },
            Command::SetClipLoop {
                id: clip,
                start_beats: 1.0,
                length_beats: 3.0,
            },
            Command::SetAudioClipGain { id: clip, db: -6.0 },
            Command::SetAudioClipReverse {
                id: clip,
                enabled: true,
            },
            Command::SetAudioClipWarp {
                id: clip,
                enabled: true,
                source_tempo: 128.0,
            },
            Command::PlaceClip {
                track,
                clip,
                start_beats: 4.0,
                length_beats: 8.0,
            },
        ])
        .unwrap();
    let snapshot = project.snapshot();
    let audio = snapshot.audio_clip_at(0, 0).unwrap();
    assert_eq!(audio.name(), "Edited");
    assert_eq!(audio.media_path(), "Media/take.wav");
    assert_eq!(audio.color_index(), 7);
    assert_eq!(audio.loop_range(), (1.0, 3.0));
    assert_eq!(audio.gain_db(), -6.0);
    assert!(audio.reversed());
    assert!(audio.warped());
    assert_eq!(audio.source_tempo(), 128.0);
    assert_eq!(snapshot.tracks()[0].arrangement().len(), 1);

    assert!(project.undo());
    let snapshot = project.snapshot();
    let audio = snapshot.audio_clip_at(0, 0).unwrap();
    assert_eq!(audio.name(), "Take");
    assert!(!audio.reversed());
    assert!(project.redo());
    assert!(project.snapshot().audio_clip_at(0, 0).unwrap().reversed());

    for media_path in [
        "",
        "../take.wav",
        "Media/../take.wav",
        "/Media/take.wav",
        "Media\\take.wav",
    ] {
        let result = project.apply(&[Command::CreateAudioClip {
            track,
            scene,
            name: "Bad".into(),
            media_path: media_path.into(),
            length_beats: 1.0,
            source_tempo: 120.0,
        }]);
        assert_eq!(result, Err(ProjectError::InvalidMediaPath));
    }
}

#[test]
fn long_history_round_trips() {
    let mut project = Project::new();
    for index in 0..2000 {
        project
            .apply(&[Command::SetTempo(40.0 + f64::from(index % 200))])
            .unwrap();
    }
    let final_state = project.snapshot();
    for _ in 0..2000 {
        assert!(project.undo());
    }
    assert!(!project.undo());
    assert_eq!(project.snapshot().tempo(), 120.0);
    for _ in 0..2000 {
        assert!(project.redo());
    }
    assert!(!project.redo());
    assert_eq!(*final_state, *project.snapshot());
}

#[test]
fn mixer_edits_are_validated_and_undoable() {
    let mut project = Project::new();
    project
        .apply(&[Command::CreateTrack {
            name: "Bus".into(),
            kind: TrackKind::Audio,
        }])
        .unwrap();
    let id = project.snapshot().tracks()[0].id();
    project
        .apply(&[
            Command::SetTrackVolume { id, db: -12.0 },
            Command::SetTrackPan { id, pan: -0.5 },
            Command::SetTrackMute { id, enabled: true },
            Command::SetTrackSolo { id, enabled: true },
            Command::SetTrackArm { id, enabled: true },
            Command::SetTrackColor { id, index: 7 },
        ])
        .unwrap();
    let snapshot = project.snapshot();
    let track = &snapshot.tracks()[0];
    assert_eq!(track.volume_db(), -12.0);
    assert_eq!(track.pan(), -0.5);
    assert!(track.muted() && track.solo() && track.armed());
    assert_eq!(track.color_index(), 7);
    assert!(project.undo());
    assert_eq!(project.snapshot().tracks()[0].volume_db(), 0.0);
    assert!(!project.snapshot().tracks()[0].muted());
    assert!(project.redo());
    for db in [f64::NAN, f64::INFINITY, -121.0, 6.01] {
        assert!(
            project
                .apply(&[Command::SetTrackVolume { id, db }])
                .is_err()
        );
    }
    for pan in [f64::NAN, f64::INFINITY, -1.01, 1.01] {
        assert!(project.apply(&[Command::SetTrackPan { id, pan }]).is_err());
    }
    assert!(
        project
            .apply(&[Command::SetTrackColor { id, index: 16 }])
            .is_err()
    );
    assert_eq!(*project.snapshot(), *snapshot);
    project
        .apply(&[Command::SetTrackVolume {
            id,
            db: f64::NEG_INFINITY,
        }])
        .unwrap();
    assert_eq!(
        project.snapshot().tracks()[0].volume_db(),
        f64::NEG_INFINITY
    );
}

#[test]
fn instrument_patches_are_typed_validated_and_undoable() {
    let mut project = Project::new();
    project
        .apply(&[
            Command::CreateTrack {
                name: "Lead".into(),
                kind: TrackKind::Midi,
            },
            Command::CreateTrack {
                name: "Audio".into(),
                kind: TrackKind::Audio,
            },
        ])
        .unwrap();
    let snapshot = project.snapshot();
    let midi = snapshot.tracks()[0].id();
    let audio = snapshot.tracks()[1].id();
    let patch = Patch {
        oscillator_mix: 0.65,
        oscillator_b_detune_cents: -12.0,
        sub_level: 0.4,
        noise_level: 0.08,
        unison_voices: 4,
        unison_detune_cents: 18.0,
        cutoff: 2_400.0,
        resonance: 1.4,
        level_db: -9.0,
        ..Patch::default()
    };
    project
        .apply(&[Command::SetInstrumentPatch { id: midi, patch }])
        .unwrap();
    assert_eq!(project.snapshot().tracks()[0].instrument_patch(), patch);
    assert!(project.undo());
    assert_eq!(
        project.snapshot().tracks()[0].instrument_patch(),
        Patch::default()
    );
    assert!(project.redo());

    let before = project.snapshot();
    let invalid = Patch {
        unison_voices: 0,
        ..patch
    };
    assert_eq!(
        project.apply(&[Command::SetInstrumentPatch {
            id: midi,
            patch: invalid,
        }]),
        Err(ProjectError::InvalidInstrument)
    );
    assert_eq!(
        project.apply(&[Command::SetInstrumentPatch { id: audio, patch }]),
        Err(ProjectError::InvalidInstrument)
    );
    assert_eq!(*project.snapshot(), *before);
}

#[test]
fn session_settings_validate_before_publishing() {
    let mut project = Project::new();
    project
        .apply(&[
            Command::SetTimeSignature {
                numerator: 7,
                denominator: 8,
            },
            Command::SetSampleRate(96000),
        ])
        .unwrap();
    assert_eq!(project.snapshot().time_signature(), (7, 8));
    assert_eq!(project.snapshot().sample_rate(), 96000);
    for (numerator, denominator) in [(0, 4), (65, 4), (4, 0), (4, 3), (4, 128)] {
        assert!(
            project
                .apply(&[Command::SetTimeSignature {
                    numerator,
                    denominator
                }])
                .is_err()
        );
    }
    assert!(project.apply(&[Command::SetSampleRate(0)]).is_err());
    assert!(project.undo());
    assert_eq!(project.snapshot().time_signature(), (4, 4));
    assert_eq!(project.snapshot().sample_rate(), 48000);
}

#[test]
fn scenes_slots_notes_and_placements_share_one_undoable_model() {
    let mut project = Project::new();
    project
        .apply(&[
            Command::CreateTrack {
                name: "Keys".into(),
                kind: TrackKind::Midi,
            },
            Command::CreateScene {
                name: "Verse".into(),
            },
        ])
        .unwrap();
    let snapshot = project.snapshot();
    let track = snapshot.tracks()[0].id();
    let scene = snapshot.scenes()[0].id();
    project
        .apply(&[Command::CreateMidiClip {
            track,
            scene,
            name: "Chord".into(),
            length_beats: 4.0,
        }])
        .unwrap();
    let clip = project.snapshot().clip_at(0, 0).unwrap().id();
    project
        .apply(&[
            Command::AddNote {
                id: clip,
                note: MidiNote {
                    pitch: 64,
                    velocity: 96,
                    start_beats: 1.0,
                    length_beats: 0.5,
                },
            },
            Command::AddNote {
                id: clip,
                note: MidiNote {
                    pitch: 60,
                    velocity: 110,
                    start_beats: 0.0,
                    length_beats: 1.0,
                },
            },
            Command::PlaceClip {
                track,
                clip,
                start_beats: 8.0,
                length_beats: 4.0,
            },
        ])
        .unwrap();
    let snapshot = project.snapshot();
    let clip = snapshot.clip_at(0, 0).unwrap();
    assert_eq!(clip.notes()[0].pitch, 60);
    assert_eq!(clip.notes()[1].pitch, 64);
    assert_eq!(snapshot.tracks()[0].arrangement()[0].start_beats(), 8.0);
    assert!(project.undo());
    assert!(project.snapshot().clip_at(0, 0).unwrap().notes().is_empty());
    assert!(project.redo());
    assert_eq!(project.snapshot().clip_at(0, 0).unwrap().notes().len(), 2);
}

#[test]
fn midi_transforms_are_atomic_deterministic_and_undoable() {
    fn populated_project() -> (Project, nylon::project::ClipId) {
        let mut project = Project::new();
        project
            .apply(&[Command::CreateTrack {
                name: "Notes".into(),
                kind: TrackKind::Midi,
            }])
            .unwrap();
        let snapshot = project.snapshot();
        let track = snapshot.tracks()[0].id();
        let scene = snapshot.scenes()[0].id();
        project
            .apply(&[Command::CreateMidiClip {
                track,
                scene,
                name: "Phrase".into(),
                length_beats: 4.0,
            }])
            .unwrap();
        let clip = project.snapshot().clip_at(0, 0).unwrap().id();
        project
            .apply(&[
                Command::AddNote {
                    id: clip,
                    note: MidiNote {
                        pitch: 60,
                        velocity: 80,
                        start_beats: 0.22,
                        length_beats: 0.5,
                    },
                },
                Command::AddNote {
                    id: clip,
                    note: MidiNote {
                        pitch: 124,
                        velocity: 100,
                        start_beats: 0.81,
                        length_beats: 0.5,
                    },
                },
            ])
            .unwrap();
        (project, clip)
    }

    let (mut project, clip) = populated_project();
    let original = project.snapshot();
    project
        .apply(&[
            Command::QuantizeNotes {
                id: clip,
                grid_beats: 0.25,
                strength: 1.0,
            },
            Command::TransposeNotes {
                id: clip,
                semitones: 3,
            },
            Command::SetNoteVelocity {
                id: clip,
                velocity: 96,
            },
            Command::HumanizeNotes {
                id: clip,
                timing_beats: 0.02,
                velocity_range: 4,
                seed: 42,
            },
        ])
        .unwrap();
    let transformed = project.snapshot();
    assert_ne!(
        transformed.clip_at(0, 0).unwrap().notes(),
        original.clip_at(0, 0).unwrap().notes()
    );
    assert!(
        transformed
            .clip_at(0, 0)
            .unwrap()
            .notes()
            .iter()
            .all(|note| { (1..=127).contains(&note.velocity) && note.start_beats >= 0.0 })
    );
    assert!(project.undo());
    assert_eq!(*project.snapshot(), *original);

    let (mut repeated, repeated_clip) = populated_project();
    repeated
        .apply(&[
            Command::QuantizeNotes {
                id: repeated_clip,
                grid_beats: 0.25,
                strength: 1.0,
            },
            Command::TransposeNotes {
                id: repeated_clip,
                semitones: 3,
            },
            Command::SetNoteVelocity {
                id: repeated_clip,
                velocity: 96,
            },
            Command::HumanizeNotes {
                id: repeated_clip,
                timing_beats: 0.02,
                velocity_range: 4,
                seed: 42,
            },
        ])
        .unwrap();
    assert_eq!(
        transformed.clip_at(0, 0).unwrap().notes(),
        repeated.snapshot().clip_at(0, 0).unwrap().notes()
    );

    let before = repeated.snapshot();
    for command in [
        Command::QuantizeNotes {
            id: repeated_clip,
            grid_beats: 0.0,
            strength: 1.0,
        },
        Command::QuantizeNotes {
            id: repeated_clip,
            grid_beats: f64::from_bits(1),
            strength: 1.0,
        },
        Command::TransposeNotes {
            id: repeated_clip,
            semitones: 12,
        },
        Command::SetNoteVelocity {
            id: repeated_clip,
            velocity: 0,
        },
        Command::HumanizeNotes {
            id: repeated_clip,
            timing_beats: -0.1,
            velocity_range: 4,
            seed: 1,
        },
    ] {
        assert_eq!(repeated.apply(&[command]), Err(ProjectError::InvalidNote));
        assert_eq!(*repeated.snapshot(), *before);
    }
}

#[test]
fn invalid_clip_edits_leave_the_snapshot_unchanged() {
    let mut project = Project::new();
    project
        .apply(&[
            Command::CreateTrack {
                name: "Keys".into(),
                kind: TrackKind::Midi,
            },
            Command::CreateScene {
                name: "Scene".into(),
            },
        ])
        .unwrap();
    let before = project.snapshot();
    let track = before.tracks()[0].id();
    let scene = before.scenes()[0].id();
    assert_eq!(
        project.apply(&[Command::CreateMidiClip {
            track,
            scene,
            name: "Bad".into(),
            length_beats: f64::NAN,
        }]),
        Err(ProjectError::InvalidClipLength)
    );
    assert_eq!(*project.snapshot(), *before);
}

#[test]
fn routing_edits_are_validated_undoable_and_use_stable_track_ids() {
    let mut project = Project::new();
    for name in ["Source A", "Source B", "Bus"] {
        project
            .apply(&[Command::CreateTrack {
                name: name.into(),
                kind: TrackKind::Audio,
            }])
            .unwrap();
    }
    let snapshot = project.snapshot();
    let a = snapshot.tracks()[0].id();
    let b = snapshot.tracks()[1].id();
    let bus = snapshot.tracks()[2].id();
    project
        .apply(&[
            Command::SetTrackLatency { id: a, frames: 128 },
            Command::SetTrackLatency { id: b, frames: 32 },
            Command::CreateRoute {
                source: a,
                destination: bus,
                kind: EdgeKind::Main,
                gain: 1.0,
            },
            Command::CreateRoute {
                source: b,
                destination: bus,
                kind: EdgeKind::SendPostFader,
                gain: 0.5,
            },
        ])
        .unwrap();
    let routed = project.snapshot();
    assert_eq!(routed.routes().len(), 2);
    assert_eq!(routed.tracks()[0].latency_frames(), 128);
    assert_eq!(routed.compiled_routing().unwrap().edge_delay(1), Some(96));

    let before = project.snapshot();
    assert_eq!(
        project.apply(&[Command::CreateRoute {
            source: bus,
            destination: a,
            kind: EdgeKind::Main,
            gain: 1.0,
        }]),
        Err(ProjectError::InvalidRouting)
    );
    assert_eq!(*project.snapshot(), *before);

    project.apply(&[Command::DeleteTrack(a)]).unwrap();
    assert_eq!(project.snapshot().routes().len(), 1);
    assert!(project.undo());
    assert_eq!(project.snapshot().routes().len(), 2);
}

#[test]
fn device_chains_are_validated_ordered_and_undoable() {
    let mut project = Project::new();
    project
        .apply(&[Command::CreateTrack {
            name: "Bus".into(),
            kind: TrackKind::Audio,
        }])
        .unwrap();
    let track = project.snapshot().tracks()[0].id();
    project
        .apply(&[
            Command::AddDevice {
                track,
                config: DeviceConfig {
                    enabled: true,
                    kind: DeviceKind::Utility {
                        gain_db: -6.0,
                        width: 1.0,
                        balance: 0.0,
                    },
                },
            },
            Command::AddDevice {
                track,
                config: DeviceConfig {
                    enabled: true,
                    kind: DeviceKind::StereoDelay {
                        delay_seconds: 0.25,
                        feedback: 0.4,
                        mix: 0.3,
                    },
                },
            },
        ])
        .unwrap();
    let snapshot = project.snapshot();
    let utility = snapshot.tracks()[0].devices()[0].id();
    let delay = snapshot.tracks()[0].devices()[1].id();
    project
        .apply(&[
            Command::MoveDevice {
                id: delay,
                index: 0,
            },
            Command::SetDeviceEnabled {
                id: utility,
                enabled: false,
            },
        ])
        .unwrap();
    assert_eq!(project.snapshot().tracks()[0].devices()[0].id(), delay);
    assert!(!project.snapshot().tracks()[0].devices()[1].enabled());
    assert!(project.undo());
    assert_eq!(project.snapshot().tracks()[0].devices()[0].id(), utility);

    let before = project.snapshot();
    assert_eq!(
        project.apply(&[Command::SetDeviceKind {
            id: utility,
            kind: DeviceKind::StereoDelay {
                delay_seconds: 1.0,
                feedback: 1.0,
                mix: 0.5,
            },
        }]),
        Err(ProjectError::InvalidDevice)
    );
    assert_eq!(*project.snapshot(), *before);
}

#[test]
fn plugin_devices_share_order_history_and_latency_with_native_devices() {
    let mut project = Project::new();
    project
        .apply(&[Command::CreateTrack {
            name: "Bus".into(),
            kind: TrackKind::Audio,
        }])
        .unwrap();
    let track = project.snapshot().tracks()[0].id();
    let plugin = PluginDevice::new(
        PluginFormat::Clap,
        "Effect.clap",
        "app.nylon.effect",
        96,
        vec![1, 2, 3],
    )
    .unwrap();
    project
        .apply(&[
            Command::AddDevice {
                track,
                config: DeviceConfig {
                    enabled: true,
                    kind: DeviceKind::Utility {
                        gain_db: 0.0,
                        width: 1.0,
                        balance: 0.0,
                    },
                },
            },
            Command::AddPluginDevice {
                track,
                enabled: true,
                plugin,
            },
        ])
        .unwrap();
    let snapshot = project.snapshot();
    let plugin_id = snapshot.tracks()[0].devices()[1].id();
    assert_eq!(
        snapshot.compiled_routing().unwrap().output_latency(0),
        Some(96)
    );
    assert_eq!(
        snapshot.tracks()[0].devices()[1].plugin().unwrap().state(),
        &[1, 2, 3]
    );
    project
        .apply(&[
            Command::MoveDevice {
                id: plugin_id,
                index: 0,
            },
            Command::SetPluginState {
                id: plugin_id,
                state: vec![9, 8],
            },
            Command::SetPluginParameter {
                id: plugin_id,
                identifier: 42,
                value: 0.75,
            },
        ])
        .unwrap();
    let changed = project.snapshot();
    assert!(changed.tracks()[0].devices()[0].plugin().is_some());
    assert_eq!(
        changed.tracks()[0].devices()[0].plugin().unwrap().state(),
        &[9, 8]
    );
    assert_eq!(
        changed.tracks()[0].devices()[0]
            .plugin()
            .unwrap()
            .parameters(),
        &[PluginParameterValue {
            identifier: 42,
            value: 0.75,
        }]
    );
    assert!(project.undo());
    assert_eq!(
        project.snapshot().tracks()[0].devices()[1]
            .plugin()
            .unwrap()
            .state(),
        &[1, 2, 3]
    );
    assert!(
        project.snapshot().tracks()[0].devices()[1]
            .plugin()
            .unwrap()
            .parameters()
            .is_empty()
    );

    assert_eq!(
        project.apply(&[Command::SetPluginParameter {
            id: plugin_id,
            identifier: 42,
            value: f64::NAN,
        }]),
        Err(ProjectError::InvalidDevice)
    );
}
