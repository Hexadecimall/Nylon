//! Mixing cost per block, reported the same way the render benchmark is.
//!
//! The figure is nanoseconds to mix one 256-frame block of 32 tracks with
//! automation, which is the shape of a busy session. At 48 kHz a block is
//! 5.33 ms of audio, so the budget for the whole callback is that; the
//! mixer should use a small fraction of it.

use nylon::mixer::{MASTER, MixEvent, Mixer, Parameter, TrackInput};
use std::hint::black_box;
use std::time::Instant;

const TRACKS: usize = 32;
const FRAMES: usize = 256;
const RATE: f32 = 48_000.0;

fn main() {
    let mut mixer = Mixer::new(TRACKS, RATE);
    let buffers: Vec<Vec<[f32; 2]>> = (0..TRACKS)
        .map(|track| {
            (0..FRAMES)
                .map(|index| {
                    let phase = (index + track * 7) as f32 * 0.01;
                    [phase.sin() * 0.2, phase.cos() * 0.2]
                })
                .collect()
        })
        .collect();
    let inputs: Vec<TrackInput<'_>> = buffers
        .iter()
        .enumerate()
        .map(|(track, samples)| TrackInput {
            track: track as u16,
            samples,
        })
        .collect();
    let mut output = vec![[0.0_f32; 2]; FRAMES];
    // A handful of automation points spread through the block, which is
    // what a moving fader and a pan sweep produce.
    let events = [
        MixEvent {
            offset: 0,
            track: 0,
            parameter: Parameter::Volume,
            value: -6.0,
        },
        MixEvent {
            offset: 64,
            track: 5,
            parameter: Parameter::Pan,
            value: -0.5,
        },
        MixEvent {
            offset: 128,
            track: 11,
            parameter: Parameter::Volume,
            value: -3.0,
        },
        MixEvent {
            offset: 192,
            track: MASTER,
            parameter: Parameter::Volume,
            value: -1.0,
        },
    ];

    for _ in 0..2_000 {
        mixer
            .render(
                black_box(&inputs),
                black_box(&mut output),
                black_box(&events),
            )
            .unwrap();
        black_box(&output);
    }

    let iterations = 20_000;
    let begin = Instant::now();
    for _ in 0..iterations {
        mixer
            .render(
                black_box(&inputs),
                black_box(&mut output),
                black_box(&events),
            )
            .unwrap();
        black_box(&output);
    }
    let elapsed = begin.elapsed().as_nanos() as f64 / iterations as f64;
    println!("mixer_ns_per_block={elapsed:.2}");
    // Share of one block's real-time budget, for a sense of headroom.
    let budget = f64::from(FRAMES as u32) / f64::from(RATE) * 1.0e9;
    println!("mixer_percent_of_budget={:.3}", elapsed / budget * 100.0);
}
