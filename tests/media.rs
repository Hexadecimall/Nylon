use nylon::audio::StreamConfig;
use nylon::audio::offline::{INPUT_DEVICE, OfflineBackend, OfflineCapture};
use nylon::audio::recording::RecordingSession;
use nylon::bounce::{Options, render_wave};
use nylon::media::{MediaError, import_wave, prepare_recording, timeline_from_project};
use nylon::project::{Command, Project, TrackKind};
use nylon::wave::{Format, WaveWriter};

fn test_directory(label: &str) -> std::path::PathBuf {
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::path::PathBuf::from("target").join(format!("{label}-{tick}"))
}

fn write_source(path: &std::path::Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut writer = WaveWriter::new(file, Format::stereo(48_000)).unwrap();
    let frames: Vec<_> = (0..480)
        .map(|index| {
            let sample = if index % 2 == 0 { 0.25 } else { -0.25 };
            [sample, -sample]
        })
        .collect();
    writer.write_stereo(&frames).unwrap();
    writer.finish().unwrap();
}

#[test]
fn imported_wave_is_copied_persisted_and_prepared_for_playback() {
    let directory = test_directory("media-import");
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("source.wav");
    write_source(&source);

    let bundle = directory.join("Session.nylonproject");
    let mut project = Project::new();
    project.save_bundle(&bundle).unwrap();
    project
        .apply(&[Command::CreateTrack {
            name: "Audio".into(),
            kind: TrackKind::Audio,
        }])
        .unwrap();
    let report = import_wave(&mut project, &source, 0, 0, 120.0).unwrap();
    assert_eq!(report.frames, 480);
    assert_eq!(report.sample_rate, 48_000);
    assert!((report.length_beats - 0.02).abs() < f64::EPSILON);
    assert!(bundle.join(&report.media_path).is_file());

    let snapshot = project.snapshot();
    let track = snapshot.tracks()[0].id();
    let clip = snapshot.audio_clip_at(0, 0).unwrap().id();
    project
        .apply(&[Command::PlaceClip {
            track,
            clip,
            start_beats: 1.0,
            length_beats: 1.0,
        }])
        .unwrap();
    let timeline = timeline_from_project(&project).unwrap();
    assert_eq!(timeline.media_count(), 1);
    assert_eq!(timeline.region_count(), 1);

    project.save_bundle(&bundle).unwrap();
    let loaded = Project::load_bundle(&bundle).unwrap();
    assert_eq!(timeline_from_project(&loaded).unwrap().region_count(), 1);
    let (_, bounce) = render_wave(
        &loaded,
        std::io::Cursor::new(Vec::new()),
        Options::stereo(1.1, 48_000),
    )
    .unwrap();
    assert!(bounce.peak_left > 0.2);
    assert!(bounce.peak_right > 0.2);
    let collected = directory.join("Collected.nylonproject");
    project.save_bundle(&collected).unwrap();
    let collected_project = Project::load_bundle(&collected).unwrap();
    assert_eq!(
        timeline_from_project(&collected_project)
            .unwrap()
            .region_count(),
        1
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn invalid_media_does_not_change_the_project() {
    let directory = test_directory("media-invalid");
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("broken.wav");
    std::fs::write(&source, b"not audio").unwrap();
    let bundle = directory.join("Session.nylonproject");
    let mut project = Project::new();
    project.save_bundle(&bundle).unwrap();
    project
        .apply(&[Command::CreateTrack {
            name: "Audio".into(),
            kind: TrackKind::Audio,
        }])
        .unwrap();
    let before = project.snapshot();
    assert!(matches!(
        import_wave(&mut project, &source, 0, 0, 120.0),
        Err(MediaError::Wave(_))
    ));
    assert_eq!(*project.snapshot(), *before);
    assert!(!bundle.join("Media").exists());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_finished_recording_becomes_one_undoable_project_clip() {
    let directory = test_directory("media-recording");
    let bundle = directory.join("Session.nylonproject");
    let mut project = Project::new();
    project.save_bundle(&bundle).unwrap();
    project
        .apply(&[Command::CreateTrack {
            name: "Audio".into(),
            kind: TrackKind::Audio,
        }])
        .unwrap();
    let mut target = prepare_recording(&project, 0, 0).unwrap();
    let destination = target.destination().to_path_buf();
    let file = target.take_file().unwrap();
    let config = StreamConfig {
        device: INPUT_DEVICE,
        sample_rate: 48_000,
        block_frames: 16,
        channels: 2,
    };
    let mut session = RecordingSession::<OfflineCapture>::open_file(
        &OfflineBackend::new(),
        config,
        file,
        &destination,
        4,
    )
    .unwrap();
    session.start().unwrap();
    session
        .stream_mut()
        .unwrap()
        .capture_from(&[[0.4, -0.4]; 16])
        .unwrap();
    let captured = session.finish().unwrap();
    let imported = target.commit(&mut project, &captured).unwrap();
    assert_eq!(imported.frames, 16);
    assert_eq!(
        project.snapshot().audio_clip_at(0, 0).unwrap().media_path(),
        imported.media_path
    );
    assert!(project.undo());
    assert!(project.snapshot().audio_clip_at(0, 0).is_none());
    assert!(destination.is_file());
    assert!(project.redo());
    assert!(project.snapshot().audio_clip_at(0, 0).is_some());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn an_abandoned_recording_removes_its_reserved_file() {
    let directory = test_directory("media-abandoned-recording");
    let bundle = directory.join("Session.nylonproject");
    let mut project = Project::new();
    project.save_bundle(&bundle).unwrap();
    project
        .apply(&[Command::CreateTrack {
            name: "Audio".into(),
            kind: TrackKind::Audio,
        }])
        .unwrap();
    let target = prepare_recording(&project, 0, 0).unwrap();
    let destination = target.destination().to_path_buf();
    assert!(destination.is_file());
    drop(target);
    assert!(!destination.exists());
    std::fs::remove_dir_all(directory).unwrap();
}
