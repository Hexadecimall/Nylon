use nylon::exchange::exchange;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct State {
    value: usize,
    drops: Arc<AtomicUsize>,
}
impl Drop for State {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn replaced_storage_is_reclaimed_on_the_control_side() {
    let drops = Arc::new(AtomicUsize::new(0));
    let state = |value| {
        Box::new(State {
            value,
            drops: drops.clone(),
        })
    };
    let (mut control, mut audio) = exchange(state(1));
    assert!(control.publish(state(2)).is_ok());
    assert!(audio.apply_pending());
    assert_eq!(audio.current().value, 2);
    assert_eq!(drops.load(Ordering::Relaxed), 0);
    drop(control.reclaim());
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    drop(audio);
    drop(control);
    assert_eq!(drops.load(Ordering::Relaxed), 2);
}

#[test]
fn backpressure_defers_swapping_without_destroying_state() {
    let (mut control, mut audio) = exchange(1);
    assert!(!audio.apply_pending());
    control.publish(2).unwrap();
    assert_eq!(control.publish(3), Err(3));
    assert!(audio.apply_pending());
    control.publish(3).unwrap();
    assert!(!audio.apply_pending());
    assert_eq!(*audio.current(), 2);
    assert_eq!(control.reclaim(), Some(1));
    assert!(audio.apply_pending());
    assert_eq!(*audio.current(), 3);
    assert_eq!(control.reclaim(), Some(2));
    assert_eq!(control.reclaim(), None);
}
