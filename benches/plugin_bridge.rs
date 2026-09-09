use nylon::plugin::bridge::{BlockProcessor, Bridge};
use nylon::plugin::clap::{NoteEvent, ParameterEvent};
use std::hint::black_box;
use std::time::Instant;

const RATE: f64 = 48_000.0;
const FRAMES: usize = 256;

struct Pass;

impl BlockProcessor for Pass {
    fn process_block(
        &mut self,
        input: Option<(&[f32], &[f32])>,
        output_left: &mut [f32],
        output_right: &mut [f32],
        _: &[ParameterEvent],
        _: &[NoteEvent],
    ) -> bool {
        if let Some((left, right)) = input {
            output_left.copy_from_slice(left);
            output_right.copy_from_slice(right);
        }
        true
    }
}

fn main() {
    let mut bridge = Bridge::new(Pass, FRAMES, 64, 0).unwrap();
    let left = [0.25_f32; FRAMES];
    let right = [-0.25_f32; FRAMES];
    let mut output_left = [0.0_f32; FRAMES];
    let mut output_right = [0.0_f32; FRAMES];
    for _ in 0..10_000 {
        bridge
            .process_stereo(
                Some((&left, &right)),
                &mut output_left,
                &mut output_right,
                &[],
                &[],
            )
            .unwrap();
    }
    let begin = Instant::now();
    for _ in 0..100_000 {
        bridge
            .process_stereo(
                Some((black_box(&left), black_box(&right))),
                black_box(&mut output_left),
                black_box(&mut output_right),
                &[],
                &[],
            )
            .unwrap();
    }
    let elapsed = begin.elapsed().as_nanos() as f64 / 100_000.0;
    let budget = FRAMES as f64 / RATE * 1_000_000_000.0;
    println!("plugin_bridge_ns_per_block={elapsed:.2}");
    println!(
        "plugin_bridge_percent_of_budget={:.3}",
        elapsed / budget * 100.0
    );
    println!("plugin_bridge_queue_drops={}", bridge.queue_drops());
}
