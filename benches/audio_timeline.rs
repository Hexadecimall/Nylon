//! Full callback cost for a dense audio arrangement.

use nylon::engine::playback::{MixSettings, PlaybackEngine};
use nylon::engine::sample::Sample;
use nylon::engine::timeline::{AudioRegion, AudioTimeline};
use std::hint::black_box;
use std::time::Instant;

const TRACKS: usize = 32;
const REGIONS_PER_TRACK: usize = 2;
const FRAMES: usize = 256;
const RATE: u32 = 48_000;

fn main() {
    let (mut engine, mut publisher) = PlaybackEngine::new(f64::from(RATE));
    let mut settings = MixSettings::new();
    settings.set_track_count(TRACKS);
    settings.set_playing(true);
    assert!(publisher.publish(&settings));

    let frames = (0..16_384)
        .map(|index| {
            let phase = index as f32 * 0.013;
            [phase.sin() * 0.1, phase.cos() * 0.1]
        })
        .collect();
    let mut timeline = AudioTimeline::new();
    let media = timeline
        .add_sample(Sample::new(RATE, frames).unwrap())
        .unwrap();
    for track in 0..TRACKS {
        for region_index in 0..REGIONS_PER_TRACK {
            let start = region_index as f64 * 32.0;
            let mut region = AudioRegion::new(media, track, start, 64.0, 0.0, 24_000.0).unwrap();
            assert!(region.set_loop(Some(0..16_384)));
            timeline.add_region(region).unwrap();
        }
    }
    assert!(publisher.publish_audio(timeline));
    let mut output = [[0.0_f32; 2]; FRAMES];

    for _ in 0..500 {
        engine.render_block(black_box(&mut output), &[]);
    }
    let iterations = 5_000;
    let begin = Instant::now();
    for _ in 0..iterations {
        engine.render_block(black_box(&mut output), &[]);
        black_box(&output);
    }
    let elapsed = begin.elapsed().as_nanos() as f64 / iterations as f64;
    println!("audio_timeline_ns_per_block={elapsed:.2}");
    let budget = FRAMES as f64 / f64::from(RATE) * 1.0e9;
    println!(
        "audio_timeline_percent_of_budget={:.3}",
        elapsed / budget * 100.0
    );
}
