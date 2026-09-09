use nylon::dsp::auto_filter::{AutoFilter, Parameters};
use std::hint::black_box;
use std::time::Instant;

const RATE: f32 = 48_000.0;
const FRAMES: usize = 256;

fn main() {
    let mut filter = AutoFilter::new(
        RATE,
        Parameters {
            envelope_amount_octaves: 3.0,
            lfo_rate_hz: 1.5,
            lfo_amount_octaves: 1.0,
            drive_db: 6.0,
            ..Parameters::default()
        },
    );
    let mut audio = [[0.1_f32, -0.08_f32]; FRAMES];
    let sidechain = [[0.4_f32, 0.3_f32]; FRAMES];
    for _ in 0..1_000 {
        filter.process_block(black_box(&mut audio), black_box(Some(&sidechain)));
    }
    let begin = Instant::now();
    for _ in 0..100_000 {
        filter.process_block(black_box(&mut audio), black_box(Some(&sidechain)));
        black_box(&audio);
    }
    let elapsed = begin.elapsed().as_nanos() as f64 / 100_000.0;
    let budget = FRAMES as f64 / RATE as f64 * 1_000_000_000.0;
    println!("auto_filter_ns_per_block={elapsed:.2}");
    println!(
        "auto_filter_percent_of_budget={:.3}",
        elapsed / budget * 100.0
    );
}
