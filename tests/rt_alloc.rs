use nylon::engine::{Engine, GainEvent, MAX_FRAMES};
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
