use nylon::bounce::{Options, render_wave};
use nylon::media::{MediaError, import_wave, timeline_from_project};
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
