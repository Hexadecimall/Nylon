use nylon::persistence::PersistenceError;
use nylon::project::{Command, MidiNote, Project, TrackKind};

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
    future[4] = 4;
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
