use nylon::dsp::reverb::{Parameters, Reverb};
use std::hint::black_box;
use std::time::Instant;

const RATE: f32 = 48_000.0;
const FRAMES: usize = 256;

fn main() {
    let mut reverb = Reverb::new(RATE, Parameters::default());
    let mut storage = vec![0.0_f32; reverb.required_storage_frames()];
    let mut audio = [[0.1_f32, -0.08_f32]; FRAMES];
    for _ in 0..1_000 {
        reverb.process_block(black_box(&mut storage), black_box(&mut audio));
    }
    let begin = Instant::now();
    for _ in 0..100_000 {
        reverb.process_block(black_box(&mut storage), black_box(&mut audio));
        black_box(&audio);
    }
    let elapsed = begin.elapsed().as_nanos() as f64 / 100_000.0;
    let budget = FRAMES as f64 / RATE as f64 * 1_000_000_000.0;
    println!("reverb_ns_per_block={elapsed:.2}");
    println!("reverb_percent_of_budget={:.3}", elapsed / budget * 100.0);
}
