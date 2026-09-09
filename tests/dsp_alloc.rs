//! Every processing path must run without touching the allocator.
//!
//! The counter below records allocator operations on the calling thread
//! while a flag is set, the same technique the render kernel test uses.
//! Storage that outlives a call is allocated before the flag goes up.

use nylon::dsp::biquad::{Biquad, Coefficients, Kind};
use nylon::dsp::compressor::{Compressor, Parameters as CompressorParameters};
use nylon::dsp::delay::DelayLine;
use nylon::dsp::env::{Envelope, Settings};
use nylon::dsp::meter::Meter;
use nylon::dsp::osc::{Oscillator, Shape};
use nylon::dsp::smooth::{OnePole, Ramp};
use nylon::dsp::{db, pan};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counter;
thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    if ENABLED.try_with(Cell::get).unwrap_or(false) {
        COUNT.with(|value| value.set(value.get() + 1));
    }
}

// SAFETY: Every allocation operation forwards its original layout and pointer.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: The caller supplies a valid allocation layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count();
        // SAFETY: The pointer and layout come from the matching allocation.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        // SAFETY: The caller supplies the allocation and its valid new size.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counter = Counter;

/// Runs `body` with allocation counting on and returns the count.
fn measure(body: impl FnOnce()) -> usize {
    COUNT.with(|value| value.set(0));
    ENABLED.with(|value| value.set(true));
    body();
    ENABLED.with(|value| value.set(false));
    COUNT.with(Cell::get)
}

const RATE: f32 = 48_000.0;
const BLOCK: usize = 512;

#[test]
fn processing_a_block_performs_no_allocator_operations() {
    // Everything the callback touches is built first.
    let mut oscillator = Oscillator::new(Shape::Saw, 220.0, RATE);
    let mut envelope = Envelope::new(Settings::default(), RATE);
    let mut filter = Biquad::new(Coefficients::design(Kind::LowPass, 2_000.0, 0.9, 0.0, RATE));
    let mut smoother = OnePole::new(0.0, 0.01, RATE);
    let mut ramp = Ramp::new(0.0);
    let mut meter = Meter::with_defaults(RATE);
    let mut compressor = Compressor::new(RATE, CompressorParameters::default());
    let mut line = DelayLine::new();
    let mut delay_buffer = vec![0.0_f32; 4_800];
    let mut block = vec![0.0_f32; BLOCK];

    envelope.note_on();
    smoother.set_target(0.8);
    ramp.ramp_to(1.0, BLOCK as u32);

    let operations = measure(|| {
        for sample in block.iter_mut().take(BLOCK) {
            let level = envelope.process() * smoother.process() * ramp.process();
            let gains = pan::constant_power(0.25);
            let dry = oscillator.process() * level * gains.left;
            let wet = line.tick(&mut delay_buffer, dry, 1_000.5);
            let mixed = filter.process(dry + wet * 0.4);
            *sample = mixed;
        }
        meter.push_block(&block);
        filter.process_block(&mut block);
        for frame in block.chunks_exact_mut(2) {
            (frame[0], frame[1]) = compressor.process_stereo(frame[0], frame[1]);
        }
        // Redesigning coefficients mid-block is a normal automation step.
        filter.set_coefficients(Coefficients::design(Kind::Peaking, 800.0, 1.5, 3.0, RATE));
        let _ = db::to_linear(-6.0);
        let _ = db::from_linear(0.5);
    });

    assert_eq!(
        operations, 0,
        "{operations} allocator operations during processing"
    );
    assert!(block.iter().all(|sample| sample.is_finite()));
    assert!(meter.peak() > 0.0);
}

#[test]
fn mixing_a_block_performs_no_allocator_operations() {
    use nylon::mixer::{MASTER, MixEvent, Mixer, Parameter, TrackInput};

    const TRACKS: usize = 32;
    let mut mixer = Mixer::new(TRACKS, RATE);
    let track_buffers: Vec<Vec<[f32; 2]>> = (0..TRACKS)
        .map(|track| {
            (0..BLOCK)
                .map(|index| {
                    let phase = (index + track) as f32 * 0.01;
                    [phase.sin() * 0.2, phase.cos() * 0.2]
                })
                .collect()
        })
        .collect();
    let inputs: Vec<TrackInput<'_>> = track_buffers
        .iter()
        .enumerate()
        .map(|(track, samples)| TrackInput {
            track: track as u16,
            samples,
        })
        .collect();
    let mut output = vec![[0.0_f32; 2]; BLOCK];
    let events = [
        MixEvent {
            offset: 0,
            track: 0,
            parameter: Parameter::Volume,
            value: -6.0,
        },
        MixEvent {
            offset: 128,
            track: 3,
            parameter: Parameter::Pan,
            value: -0.5,
        },
        MixEvent {
            offset: 256,
            track: 7,
            parameter: Parameter::Solo,
            value: 1.0,
        },
        MixEvent {
            offset: 384,
            track: MASTER,
            parameter: Parameter::Volume,
            value: -3.0,
        },
    ];
    let mut levels = vec![nylon::mixer::Levels::default(); TRACKS + 1];

    // One render before counting, so any lazy initialization is done.
    mixer.render(&inputs, &mut output, &events).unwrap();

    let operations = measure(|| {
        for _ in 0..8 {
            mixer.render(&inputs, &mut output, &events).unwrap();
            mixer.copy_levels(&mut levels);
        }
        mixer.clear_clipping();
        mixer.set_volume_db(1, -12.0);
        mixer.set_pan(2, 0.75);
        mixer.set_muted(4, true);
        mixer.set_soloed(5, true);
        mixer.reset();
    });

    assert_eq!(
        operations, 0,
        "{operations} allocator operations while mixing"
    );
    assert!(
        output
            .iter()
            .all(|frame| frame[0].is_finite() && frame[1].is_finite())
    );
}

#[test]
fn the_playback_engine_renders_without_allocating() {
    use nylon::engine::playback::{MixSettings, PlaybackEngine, TrackSettings};
    use nylon::engine::sample::Sample;
    use nylon::engine::timeline::{AudioRegion, AudioTimeline};
    use nylon::routing::{Edge, EdgeKind, RoutingGraph};

    let (mut engine, mut publisher) = PlaybackEngine::new(48_000.0);
    let mut settings = MixSettings::new();
    settings.set_track_count(16);
    for index in 0..16 {
        settings.set_track(
            index,
            TrackSettings {
                volume_db: -3.0,
                pan: 0.25,
                muted: index % 5 == 0,
                soloed: false,
            },
        );
    }
    // The transport is started through the settings, since once the
    // engine is handed to a stream it cannot be reached directly.
    settings.set_playing(true);
    assert!(publisher.publish(&settings));
    let mut timeline = AudioTimeline::new();
    let media = timeline
        .add_sample(Sample::new(48_000, vec![[0.1, -0.1]; 4_096]).unwrap())
        .unwrap();
    let mut region = AudioRegion::new(media, 9, 0.0, 8.0, 0.0, 24_000.0).unwrap();
    assert!(region.set_loop(Some(0..4_096)));
    timeline.add_region(region).unwrap();
    assert!(publisher.publish_audio(timeline));
    let mut graph = RoutingGraph::new(17).unwrap();
    for track in 0..16 {
        graph
            .add_edge(Edge {
                source: track,
                destination: 16,
                kind: EdgeKind::Main,
                gain: 1.0,
            })
            .unwrap();
    }
    let compiled = graph.compile().unwrap();
    assert!(publisher.publish_routing(&compiled, 16).unwrap());
    let mut output = vec![[0.0_f32; 2]; BLOCK];
    // One block before counting so the settings are taken up.
    engine.render_block(&mut output, &[]);
    // Prepare another revision so the measured callback also exercises
    // graph replacement and deferred reclamation.
    assert!(publisher.publish_routing(&compiled, 16).unwrap());

    let operations = measure(|| {
        for index in 0..64 {
            // Publishing from the control thread and rendering from the
            // audio thread both have to stay clear of the allocator.
            if index % 8 == 0 {
                let _ = publisher.publish(&settings);
            }
            engine.render_block(&mut output, &[]);
            let _ = publisher.state();
        }
    });

    assert_eq!(
        operations, 0,
        "{operations} allocator operations while the engine ran"
    );
    assert!(engine.transport().position_frames() > 0);
}

#[test]
fn routing_a_block_performs_no_allocator_operations() {
    use nylon::engine::graph::{GraphRenderer, NodeInput};
    use nylon::routing::{Edge, EdgeKind, RoutingGraph};

    let mut graph = RoutingGraph::new(3).unwrap();
    graph.set_node_latency(0, 128).unwrap();
    graph
        .add_edge(Edge {
            source: 0,
            destination: 2,
            kind: EdgeKind::Main,
            gain: 1.0,
        })
        .unwrap();
    graph
        .add_edge(Edge {
            source: 1,
            destination: 2,
            kind: EdgeKind::Sidechain,
            gain: 0.5,
        })
        .unwrap();
    let compiled = graph.compile().unwrap();
    let mut renderer = GraphRenderer::new(&compiled, BLOCK).unwrap();
    let source = vec![[0.25, -0.25]; BLOCK];
    let inputs = [NodeInput {
        node: 0,
        samples: &source,
    }];
    let mut output = vec![[0.0; 2]; BLOCK];

    let operations = measure(|| {
        renderer
            .render(&inputs, 2, &mut output, |_, main, sidechain, pre, post| {
                pre.copy_from_slice(main);
                for ((post, main), sidechain) in post.iter_mut().zip(main).zip(sidechain) {
                    post[0] = main[0] + sidechain[0];
                    post[1] = main[1] + sidechain[1];
                }
            })
            .unwrap();
    });

    assert_eq!(
        operations, 0,
        "{operations} allocator operations while routing"
    );
}

#[test]
fn a_device_chain_processes_without_allocating() {
    use nylon::dsp::biquad::Kind as FilterKind;
    use nylon::dsp::compressor::Parameters as CompressorParameters;
    use nylon::engine::device::{DeviceChain, DeviceConfig, DeviceKind};

    let configs = [
        DeviceConfig {
            enabled: true,
            kind: DeviceKind::Utility {
                gain_db: -3.0,
                width: 1.2,
                balance: 0.1,
            },
        },
        DeviceConfig {
            enabled: true,
            kind: DeviceKind::Equalizer {
                kind: FilterKind::Peaking,
                frequency: 1_200.0,
                q: 0.8,
                gain_db: 2.0,
            },
        },
        DeviceConfig {
            enabled: true,
            kind: DeviceKind::Compressor {
                parameters: CompressorParameters::default(),
                external_sidechain: true,
            },
        },
        DeviceConfig {
            enabled: true,
            kind: DeviceKind::StereoDelay {
                delay_seconds: 0.01,
                feedback: 0.25,
                mix: 0.2,
            },
        },
    ];
    let mut chain = DeviceChain::new(&configs, RATE).unwrap();
    let input = vec![[0.2, -0.1]; BLOCK];
    let sidechain = vec![[0.8, 0.4]; BLOCK];
    let mut output = vec![[0.0; 2]; BLOCK];
    chain.process(&input, &sidechain, &mut output).unwrap();

    let operations = measure(|| {
        for _ in 0..32 {
            chain.process(&input, &sidechain, &mut output).unwrap();
        }
        chain.reset();
    });
    assert_eq!(
        operations, 0,
        "{operations} allocator operations in device chain"
    );
    assert!(output.iter().flatten().all(|sample| sample.is_finite()));
}

#[test]
fn a_voice_bank_sounds_notes_without_allocating() {
    use nylon::engine::voice::{MAX_VOICES, Patch, VoiceBank};

    let mut bank = VoiceBank::new(Patch::default(), RATE);
    let mut output = vec![[0.0_f32; 2]; BLOCK];
    // One pass before counting so anything lazy is already done.
    bank.note_on(60, 100);
    bank.render(&mut output, 0.0);

    let operations = measure(|| {
        for round in 0..16 {
            // More notes than voices, so voice stealing runs too.
            for offset in 0..MAX_VOICES as u8 + 8 {
                bank.note_on(40 + offset, 90);
            }
            bank.render(&mut output, if round % 2 == 0 { -0.5 } else { 0.5 });
            for offset in 0..MAX_VOICES as u8 + 8 {
                bank.note_off(40 + offset);
            }
            bank.render_additive(&mut output, 0.0);
            bank.set_patch(Patch {
                cutoff: 1_000.0 + round as f32 * 100.0,
                ..Patch::default()
            });
            bank.set_polyphony(8 + round);
        }
        bank.all_notes_off();
        bank.reset();
    });

    assert_eq!(
        operations, 0,
        "{operations} allocator operations while voices sounded"
    );
    assert!(output.iter().all(|frame| frame[0].is_finite()));
}

#[test]
fn envelope_and_oscillator_state_changes_do_not_allocate() {
    let mut oscillator = Oscillator::new(Shape::Sine, 440.0, RATE);
    let mut envelope = Envelope::new(Settings::default(), RATE);
    let operations = measure(|| {
        for index in 0..1_000 {
            oscillator.set_frequency(440.0 + index as f32);
            oscillator.set_shape(if index % 2 == 0 {
                Shape::Square
            } else {
                Shape::Triangle
            });
            oscillator.process();
            if index % 100 == 0 {
                envelope.note_on();
            }
            if index % 100 == 50 {
                envelope.note_off();
            }
            envelope.set_settings(Settings {
                attack: 0.001,
                decay: 0.05,
                sustain: 0.5,
                release: 0.1,
            });
            envelope.process();
        }
    });
    assert_eq!(operations, 0);
}

#[test]
fn sample_playback_does_not_allocate() {
    use nylon::engine::sample::{Interpolation, Player, Sample};

    let frames = (0..4_096)
        .map(|index| {
            let value = (index as f32 * 0.01).sin();
            [value, -value]
        })
        .collect();
    let sample = Sample::new(48_000, frames).unwrap();
    let mut player = Player::new(&sample, 44_100);
    player.set_interpolation(Interpolation::Cubic);
    assert!(player.set_loop(Some(128..4_000)));
    player.trigger();
    let mut output = vec![[0.0; 2]; BLOCK];

    let operations = measure(|| {
        for _ in 0..32 {
            output.fill([0.0; 2]);
            player.render_additive(&mut output);
        }
    });

    assert_eq!(
        operations, 0,
        "{operations} allocator operations during sample playback"
    );
    assert!(output.iter().flatten().all(|sample| sample.is_finite()));
}
