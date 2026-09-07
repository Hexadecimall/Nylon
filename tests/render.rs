use nylon::engine::{Engine, GainEvent, MAX_EVENTS, MAX_FRAMES, RenderError};

#[test]
fn changes_split_at_exact_sample_and_persist() {
    let mut engine = Engine::new();
    engine.transport().play();
    let mut output = [[0.0; 2]; 5];
    engine
        .render(
            &[[1.0, -1.0]; 5],
            &mut output,
            &[
                GainEvent {
                    offset: 0,
                    gain: 0.5,
                },
                GainEvent {
                    offset: 2,
                    gain: 0.25,
                },
                GainEvent {
                    offset: 2,
                    gain: 0.125,
                },
                GainEvent {
                    offset: 5,
                    gain: 0.0,
                },
            ],
        )
        .unwrap();
    assert_eq!(
        output,
        [
            [0.5, -0.5],
            [0.5, -0.5],
            [0.125, -0.125],
            [0.125, -0.125],
            [0.125, -0.125]
        ]
    );
    assert_eq!(engine.transport().position(), 5);
    engine.render(&[[1.0; 2]; 5], &mut output, &[]).unwrap();
    assert_eq!(output, [[0.0; 2]; 5]);
}

#[test]
fn invalid_blocks_preserve_output_gain_and_clock() {
    let cases = [
        (
            vec![GainEvent {
                offset: 9,
                gain: 1.0,
            }],
            RenderError::EventOffset,
        ),
        (
            vec![
                GainEvent {
                    offset: 1,
                    gain: 0.5,
                },
                GainEvent {
                    offset: 0,
                    gain: 1.0,
                },
            ],
            RenderError::EventOrder,
        ),
        (
            vec![GainEvent {
                offset: 0,
                gain: f32::NAN,
            }],
            RenderError::InvalidGain,
        ),
        (
            vec![GainEvent {
                offset: 0,
                gain: f32::INFINITY,
            }],
            RenderError::InvalidGain,
        ),
        (
            vec![GainEvent {
                offset: 0,
                gain: -1.0,
            }],
            RenderError::InvalidGain,
        ),
        (
            vec![
                GainEvent {
                    offset: 0,
                    gain: 1.0
                };
                MAX_EVENTS + 1
            ],
            RenderError::EventCapacity,
        ),
    ];
    for (events, error) in cases {
        let mut engine = Engine::new();
        engine.transport().play();
        let mut output = [[42.0; 2]; 2];
        assert_eq!(
            engine.render(&[[1.0; 2]; 2], &mut output, &events),
            Err(error)
        );
        assert_eq!(output, [[42.0; 2]; 2]);
        assert_eq!(engine.transport().position(), 0);
        engine.render(&[[1.0; 2]; 2], &mut output, &[]).unwrap();
        assert_eq!(output, [[1.0; 2]; 2]);
    }
}

#[test]
fn transport_stop_locate_and_overflow() {
    let mut engine = Engine::new();
    let mut output = [[0.0; 2]; 32];
    engine.transport().locate(123);
    engine.render(&[[1.0; 2]; 32], &mut output, &[]).unwrap();
    assert_eq!(engine.transport().position(), 123);
    engine.transport().play();
    engine.render(&[[1.0; 2]; 32], &mut output, &[]).unwrap();
    assert_eq!(engine.transport().position(), 155);
    engine.transport().stop();
    assert!(!engine.transport().is_playing());
    engine.transport().locate(u64::MAX);
    engine.transport().play();
    assert_eq!(
        engine.render(&[[0.0; 2]; 32], &mut output, &[]),
        Err(RenderError::ClockOverflow)
    );
    assert_eq!(output, [[1.0; 2]; 32]);
}

#[test]
fn buffer_limits_and_empty_blocks() {
    let mut engine = Engine::new();
    assert_eq!(
        engine.render(&[[0.0; 2]], &mut [], &[]),
        Err(RenderError::BufferSize)
    );
    assert_eq!(
        engine.render(
            &[[0.0; 2]; MAX_FRAMES + 1],
            &mut [[0.0; 2]; MAX_FRAMES + 1],
            &[]
        ),
        Err(RenderError::BufferSize)
    );
    engine.render(&[], &mut [], &[]).unwrap();
    engine
        .render(&[[0.0; 2]; MAX_FRAMES], &mut [[0.0; 2]; MAX_FRAMES], &[])
        .unwrap();
}

#[test]
fn partitioning_preserves_bit_identical_output() {
    let input: Vec<_> = (0..1024)
        .map(|i| [i as f32 / 1024.0, -(i as f32)])
        .collect();
    let events = [
        GainEvent {
            offset: 127,
            gain: 0.33,
        },
        GainEvent {
            offset: 513,
            gain: 0.81,
        },
    ];
    let mut whole = vec![[0.0; 2]; input.len()];
    Engine::new().render(&input, &mut whole, &events).unwrap();
    for size in [1, 7, 32, 127, 128, 256, 512] {
        let mut engine = Engine::new();
        let mut split = vec![[0.0; 2]; input.len()];
        for start in (0..input.len()).step_by(size) {
            let end = (start + size).min(input.len());
            let local: Vec<_> = events
                .iter()
                .filter(|e| e.offset >= start && e.offset < end)
                .map(|e| GainEvent {
                    offset: e.offset - start,
                    gain: e.gain,
                })
                .collect();
            engine
                .render(&input[start..end], &mut split[start..end], &local)
                .unwrap();
        }
        assert_eq!(whole, split);
    }
}
