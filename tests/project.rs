use nylon::project::{Command, MidiNote, Project, ProjectError, TrackKind};

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
