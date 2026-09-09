use nylon::audio::{BlockTiming, Renderer};
use nylon::engine::automation::{Lane, Point, Timeline};
use nylon::engine::playback::{MixSettings, PlaybackEngine, TempoTimeline};
use nylon::engine::{Engine, GainEvent, MAX_FRAMES};
use nylon::mixer::{AutomationCurve, Parameter};
use nylon::transport::TempoChange;
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

#[test]
fn render_callback_performs_no_allocator_operations() {
    let mut engine = Engine::new();
    engine.transport().play();
    let input = [[0.25, -0.25]; MAX_FRAMES];
    let mut output = [[0.0; 2]; MAX_FRAMES];
    COUNT.with(|value| value.set(0));
    ENABLED.with(|value| value.set(true));
    for _ in 0..1000 {
        engine
            .render(
                &input,
                &mut output,
                &[GainEvent {
                    offset: 17,
                    gain: 0.5,
                }],
            )
            .unwrap();
    }
    ENABLED.with(|value| value.set(false));
    assert_eq!(COUNT.with(Cell::get), 0);
}

#[test]
fn state_swap_defers_all_deallocation() {
    let (mut control, mut audio) = nylon::exchange::exchange(Box::new([1.0_f32; 64]));
    control.publish(Box::new([2.0_f32; 64])).unwrap();
    COUNT.with(|value| value.set(0));
    ENABLED.with(|value| value.set(true));
    let changed = audio.apply_pending();
    let unchanged = audio.apply_pending();
    ENABLED.with(|value| value.set(false));
    assert!(changed);
    assert!(!unchanged);
    assert_eq!(COUNT.with(Cell::get), 0);
    assert_eq!(audio.current()[0], 2.0);
    drop(control.reclaim());
}

#[test]
fn automated_project_render_performs_no_allocator_operations() {
    let (mut engine, mut publisher) = PlaybackEngine::new(48_000.0);
    let mut settings = MixSettings::new();
    settings.set_track_count(1);
    settings.set_playing(true);
    assert!(publisher.publish(&settings));
    let mut timeline = Timeline::new();
    assert!(timeline.add_lane(Lane {
        track: 0,
        parameter: Parameter::Pan,
        points: vec![
            Point {
                beat: 0.0,
                value: -1.0,
                curve: AutomationCurve::Linear,
            },
            Point {
                beat: 8.0,
                value: 1.0,
                curve: AutomationCurve::Smooth,
            },
        ],
    }));
    assert!(publisher.publish_automation(timeline));
    assert!(
        publisher.publish_tempo(
            TempoTimeline::from_parts(
                120.0,
                &[TempoChange {
                    beat: 0.05,
                    tempo: 75.0,
                }],
            )
            .unwrap(),
        )
    );
    let mut output = [[0.0_f32; 2]; MAX_FRAMES];
    engine.render(&mut output, BlockTiming::default());

    COUNT.with(|value| value.set(0));
    ENABLED.with(|value| value.set(true));
    for _ in 0..100 {
        engine.render(&mut output, BlockTiming::default());
    }
    ENABLED.with(|value| value.set(false));
    assert_eq!(COUNT.with(Cell::get), 0);
}
