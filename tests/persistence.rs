use nylon::persistence::PersistenceError;
use nylon::project::{Command, Project, TrackKind};

fn session() -> Project {
    let mut project = Project::new();
    project
        .apply(&[Command::CreateTrack {
            name: "Synth".into(),
            kind: TrackKind::Midi,
        }])
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
    future[4] = 2;
    assert!(matches!(
        Project::from_bytes(&future),
        Err(PersistenceError::UnsupportedVersion)
    ));
    let mut trailing = bytes;
    trailing.push(0);
    assert!(Project::from_bytes(&trailing).is_err());
}

#[test]
fn bundle_save_replaces_the_document_without_leaving_temporary_files() {
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::path::PathBuf::from("target").join(format!("save-{tick}"));
    let mut project = session();
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
