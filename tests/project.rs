use nylon::project::{Command, Project, ProjectError, TrackKind};

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
