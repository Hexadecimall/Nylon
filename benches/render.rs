use nylon::engine::{Engine, GainEvent};
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let mut engine = Engine::new();
    let input = [[0.25, -0.25]; 256];
    let mut output = [[0.0; 2]; 256];
    let events = [GainEvent {
        offset: 128,
        gain: 0.5,
    }];
    for _ in 0..10_000 {
        engine
            .render(
                black_box(&input),
                black_box(&mut output),
                black_box(&events),
            )
            .unwrap();
    }
    let begin = Instant::now();
    for _ in 0..1_000_000 {
        engine
            .render(
                black_box(&input),
                black_box(&mut output),
                black_box(&events),
            )
            .unwrap();
        black_box(&output);
    }
    println!(
        "render_ns_per_block={:.2}",
        begin.elapsed().as_nanos() as f64 / 1_000_000.0
    );
}
