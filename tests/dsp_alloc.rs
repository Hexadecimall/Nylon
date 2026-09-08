//! Every processing path must run without touching the allocator.
//!
//! The counter below records allocator operations on the calling thread
//! while a flag is set, the same technique the render kernel test uses.
//! Storage that outlives a call is allocated before the flag goes up.

use nylon::dsp::biquad::{Biquad, Coefficients, Kind};
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
