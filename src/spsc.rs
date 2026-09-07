//! Bounded single-producer single-consumer queue.
//!
//! The queue allocates its storage once at construction and never again.
//! `push` and `pop` are wait-free, perform no syscalls, and never block, so
//! either end may live on the real-time audio thread.
//!
//! One `Producer` and one `Consumer` are created per queue. Every operation
//! that moves data takes `&mut self`, so the borrow checker enforces the
//! single-producer, single-consumer invariant: a given end can be driven by
//! only one thread at a time. Both ends are `Send`, and they are also `Sync`
//! because the `&self` methods (`len`, `capacity`, ...) only read.
//!
//! Real-time safety covers the queue itself. Code the queue calls back
//! into is the caller's responsibility: `T::clone` in [`Producer::push_slice`],
//! the iterator in [`Producer::push_iter`], the closure in
//! [`Consumer::drain_with`], and `T::drop` when the last end is dropped or
//! a rejected value is discarded. Use `Copy` payloads or hand-written
//! `&mut self` loops over `push`/`pop` on the audio thread.
//!
//! Capacity is rounded up to a power of two. Positions are monotonically
//! increasing counters masked on access, so the full/empty distinction does
//! not cost a slot.

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Pads a value to its own cache line so the producer and consumer indices
/// do not share a line and invalidate each other on every write.
#[repr(align(128))]
struct CachePadded<T>(T);

struct Inner<T> {
    buffer: Box<[UnsafeCell<MaybeUninit<T>>]>,
    mask: usize,
    /// Next position the consumer will read. Written only by the consumer.
    head: CachePadded<AtomicUsize>,
    /// Next position the producer will write. Written only by the producer.
    tail: CachePadded<AtomicUsize>,
}

// SAFETY: Slots are accessed by at most one thread at a time. The producer
// only touches slots in `[tail, head + capacity)` and the consumer only touches
// slots in `[head, tail)`; the two ranges are disjoint and the atomics publish
// each side's writes before the other side may read them.
unsafe impl<T: Send> Sync for Inner<T> {}
// SAFETY: Ownership of stored `T` values moves with the queue as a whole.
unsafe impl<T: Send> Send for Inner<T> {}

impl<T> Drop for Inner<T> {
    fn drop(&mut self) {
        // Both ends have been dropped, so plain loads are sufficient.
        let head = self.head.0.load(Ordering::Relaxed);
        let tail = self.tail.0.load(Ordering::Relaxed);
        // Counters are free-running and may have wrapped, so iterate by
        // distance rather than over the range `head..tail`.
        for i in 0..tail.wrapping_sub(head) {
            let slot = self.buffer[head.wrapping_add(i) & self.mask].get();
            // SAFETY: Every slot within `tail - head` of `head` holds an
            // initialized value that has not been popped.
            unsafe { (*slot).assume_init_drop() };
        }
    }
}

/// Sending half of a [`SpscQueue`].
pub struct Producer<T> {
    inner: Arc<Inner<T>>,
    /// Local copy of `tail`; the producer is the only writer.
    tail: usize,
    /// Cached snapshot of `head`, refreshed only when the queue looks full.
    head_cache: usize,
}

/// Receiving half of a [`SpscQueue`].
pub struct Consumer<T> {
    inner: Arc<Inner<T>>,
    /// Local copy of `head`; the consumer is the only writer.
    head: usize,
    /// Cached snapshot of `tail`, refreshed only when the queue looks empty.
    tail_cache: usize,
}

/// Constructor namespace for the queue. Instances are never held directly;
/// [`SpscQueue::with_capacity`] returns the two ends.
pub struct SpscQueue<T>(core::marker::PhantomData<T>);

impl<T> SpscQueue<T> {
    /// Allocates a queue able to hold at least `capacity` elements and
    /// returns its two ends. `capacity` is rounded up to a power of two and
    /// must be non-zero.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is zero or would overflow when rounded.
    pub fn with_capacity(capacity: usize) -> (Producer<T>, Consumer<T>) {
        assert!(capacity > 0, "spsc capacity must be non-zero");
        let capacity = capacity
            .checked_next_power_of_two()
            .expect("spsc capacity overflows when rounded to a power of two");
        // A counter difference of `capacity` marks a full queue, so the
        // counters must be able to exceed the capacity without wrapping into
        // the mask.
        assert!(capacity <= usize::MAX / 2, "spsc capacity too large");

        let buffer = (0..capacity)
            .map(|_| UnsafeCell::new(MaybeUninit::uninit()))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let inner = Arc::new(Inner {
            buffer,
            mask: capacity - 1,
            head: CachePadded(AtomicUsize::new(0)),
            tail: CachePadded(AtomicUsize::new(0)),
        });
        Self::split(inner, 0)
    }

    /// As [`with_capacity`](Self::with_capacity) but with both position
    /// counters starting at `start`, for exercising counter wraparound.
    #[cfg(test)]
    fn with_capacity_at(capacity: usize, start: usize) -> (Producer<T>, Consumer<T>) {
        let (p, c) = Self::with_capacity(capacity);
        p.inner.head.0.store(start, Ordering::Relaxed);
        p.inner.tail.0.store(start, Ordering::Relaxed);
        let inner = p.inner;
        drop(c);
        Self::split(inner, start)
    }

    fn split(inner: Arc<Inner<T>>, start: usize) -> (Producer<T>, Consumer<T>) {
        (
            Producer {
                inner: Arc::clone(&inner),
                tail: start,
                head_cache: start,
            },
            Consumer {
                inner,
                head: start,
                tail_cache: start,
            },
        )
    }
}

/// Publishes writes made to slots past `tail` when dropped, so a panic in a
/// caller-supplied `clone` cannot strand already-written values in
/// unpublished slots (they would never be dropped).
struct Publish<'a> {
    tail: &'a mut usize,
    shared: &'a AtomicUsize,
    written: usize,
}

impl Drop for Publish<'_> {
    fn drop(&mut self) {
        if self.written > 0 {
            *self.tail = self.tail.wrapping_add(self.written);
            self.shared.store(*self.tail, Ordering::Release);
        }
    }
}

impl<T> Producer<T> {
    /// Number of slots the queue can hold.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.inner.mask + 1
    }

    /// Number of elements currently queued, as seen from the producer.
    #[inline]
    pub fn len(&self) -> usize {
        let head = self.inner.head.0.load(Ordering::Acquire);
        self.tail.wrapping_sub(head)
    }

    /// True when no element is queued, as seen from the producer.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True when no further element can be pushed right now.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    /// Free slots available to the producer right now.
    #[inline]
    pub fn slots(&self) -> usize {
        self.capacity() - self.len()
    }

    /// True when the consumer has been dropped. Pushes still succeed but
    /// will never be observed.
    #[inline]
    pub fn is_abandoned(&self) -> bool {
        Arc::strong_count(&self.inner) == 1
    }

    /// Appends `value`. Returns it back as `Err` when the queue is full so
    /// the caller decides where it is dropped.
    #[inline]
    pub fn push(&mut self, value: T) -> Result<(), T> {
        if self.free_from_cache() == 0 {
            self.head_cache = self.inner.head.0.load(Ordering::Acquire);
            if self.free_from_cache() == 0 {
                return Err(value);
            }
        }
        let slot = self.inner.buffer[self.tail & self.inner.mask].get();
        // SAFETY: The slot is beyond `head` (checked above) so the consumer
        // is not reading it, and it is at or beyond a position the consumer
        // has already vacated, so it holds no live value.
        unsafe { (*slot).write(value) };
        self.tail = self.tail.wrapping_add(1);
        self.inner.tail.0.store(self.tail, Ordering::Release);
        Ok(())
    }

    /// Appends as many elements from `items` as fit and returns how many
    /// were consumed. Elements are cloned into the queue in order.
    ///
    /// `T::clone` runs on the calling thread; it must itself be real-time
    /// safe if this is called from the audio thread. If it panics, the
    /// elements cloned before the panic remain queued and are dropped with
    /// the queue.
    pub fn push_slice(&mut self, items: &[T]) -> usize
    where
        T: Clone,
    {
        let mut free = self.free_from_cache();
        if free < items.len() {
            self.head_cache = self.inner.head.0.load(Ordering::Acquire);
            free = self.free_from_cache();
        }
        let count = free.min(items.len());
        let mut publish = Publish {
            tail: &mut self.tail,
            shared: &self.inner.tail.0,
            written: 0,
        };
        for item in &items[..count] {
            let pos = publish.tail.wrapping_add(publish.written);
            let value = item.clone();
            let slot = self.inner.buffer[pos & self.inner.mask].get();
            // SAFETY: All `count` positions are free (see `push`).
            unsafe { (*slot).write(value) };
            publish.written += 1;
        }
        drop(publish);
        count
    }

    /// Appends elements drawn from `iter` until the queue is full or the
    /// iterator ends. Returns the number pushed.
    ///
    /// The iterator runs on the calling thread and is not made real-time
    /// safe by the queue.
    pub fn push_iter<I>(&mut self, iter: I) -> usize
    where
        I: IntoIterator<Item = T>,
    {
        let mut pushed = 0;
        for item in iter {
            if self.push(item).is_err() {
                break;
            }
            pushed += 1;
        }
        pushed
    }

    #[inline]
    fn free_from_cache(&self) -> usize {
        self.capacity() - self.tail.wrapping_sub(self.head_cache)
    }
}

impl<T> Consumer<T> {
    /// Number of slots the queue can hold.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.inner.mask + 1
    }

    /// Number of elements currently queued, as seen from the consumer.
    #[inline]
    pub fn len(&self) -> usize {
        let tail = self.inner.tail.0.load(Ordering::Acquire);
        tail.wrapping_sub(self.head)
    }

    /// True when no element is queued, as seen from the consumer.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True when the queue holds `capacity` elements.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    /// True when the producer has been dropped. Elements already queued can
    /// still be popped.
    #[inline]
    pub fn is_abandoned(&self) -> bool {
        Arc::strong_count(&self.inner) == 1
    }

    /// Removes and returns the oldest element, or `None` when empty.
    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        if self.available_from_cache() == 0 {
            self.tail_cache = self.inner.tail.0.load(Ordering::Acquire);
            if self.available_from_cache() == 0 {
                return None;
            }
        }
        let slot = self.inner.buffer[self.head & self.inner.mask].get();
        // SAFETY: `head < tail` so the slot was written by the producer and
        // published with a release store that the acquire load above paired
        // with. Reading moves the value out; the slot is then logically free.
        let value = unsafe { (*slot).assume_init_read() };
        self.head = self.head.wrapping_add(1);
        self.inner.head.0.store(self.head, Ordering::Release);
        Some(value)
    }

    /// Returns a reference to the oldest element without removing it.
    #[inline]
    pub fn peek(&mut self) -> Option<&T> {
        if self.available_from_cache() == 0 {
            self.tail_cache = self.inner.tail.0.load(Ordering::Acquire);
            if self.available_from_cache() == 0 {
                return None;
            }
        }
        let slot = self.inner.buffer[self.head & self.inner.mask].get();
        // SAFETY: As in `pop`; the value stays in place and the borrow is
        // tied to `&mut self`, so it cannot outlive a later `pop`.
        Some(unsafe { (*slot).assume_init_ref() })
    }

    /// Moves up to `out.len()` elements into `out` and returns how many were
    /// written. Elements are moved, not cloned.
    pub fn pop_slice(&mut self, out: &mut [T]) -> usize
    where
        T: Copy,
    {
        let mut avail = self.available_from_cache();
        if avail < out.len() {
            self.tail_cache = self.inner.tail.0.load(Ordering::Acquire);
            avail = self.available_from_cache();
        }
        let count = avail.min(out.len());
        for (i, dst) in out[..count].iter_mut().enumerate() {
            let pos = self.head.wrapping_add(i);
            let slot = self.inner.buffer[pos & self.inner.mask].get();
            // SAFETY: All `count` positions are initialized (see `pop`).
            // `T: Copy` so reading does not leave a dangling owner behind.
            *dst = unsafe { (*slot).assume_init_read() };
        }
        if count > 0 {
            self.head = self.head.wrapping_add(count);
            self.inner.head.0.store(self.head, Ordering::Release);
        }
        count
    }

    /// Drains every element currently visible, calling `f` on each in
    /// order. Returns the number drained. Elements pushed while draining are
    /// left for the next call.
    ///
    /// `f` runs on the calling thread and is not made real-time safe by the
    /// queue. Each element is unlinked from the queue before `f` sees it, so
    /// a panic inside `f` drops only that element.
    pub fn drain_with<F: FnMut(T)>(&mut self, mut f: F) -> usize {
        self.tail_cache = self.inner.tail.0.load(Ordering::Acquire);
        let count = self.available_from_cache();
        for _ in 0..count {
            let slot = self.inner.buffer[self.head & self.inner.mask].get();
            // SAFETY: `head` is in `[head, tail)`, so the slot is initialized.
            let value = unsafe { (*slot).assume_init_read() };
            // Publish progress before calling out so a panic in `f` cannot
            // leave a moved-out slot inside the live range.
            self.head = self.head.wrapping_add(1);
            self.inner.head.0.store(self.head, Ordering::Release);
            f(value);
        }
        count
    }

    #[inline]
    fn available_from_cache(&self) -> usize {
        self.tail_cache.wrapping_sub(self.head)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize as StdAtomic;
    use std::thread;

    #[test]
    fn capacity_rounds_to_power_of_two() {
        let (p, c) = SpscQueue::<u8>::with_capacity(5);
        assert_eq!(p.capacity(), 8);
        assert_eq!(c.capacity(), 8);
        let (p, _) = SpscQueue::<u8>::with_capacity(1);
        assert_eq!(p.capacity(), 1);
        let (p, _) = SpscQueue::<u8>::with_capacity(64);
        assert_eq!(p.capacity(), 64);
    }

    #[test]
    #[should_panic(expected = "non-zero")]
    fn zero_capacity_panics() {
        let _ = SpscQueue::<u8>::with_capacity(0);
    }

    #[test]
    fn push_pop_fifo() {
        let (mut p, mut c) = SpscQueue::with_capacity(4);
        assert!(c.pop().is_none());
        assert!(p.push(1).is_ok());
        assert!(p.push(2).is_ok());
        assert!(p.push(3).is_ok());
        assert_eq!(p.len(), 3);
        assert_eq!(c.len(), 3);
        assert_eq!(c.pop(), Some(1));
        assert_eq!(c.pop(), Some(2));
        assert_eq!(c.pop(), Some(3));
        assert_eq!(c.pop(), None);
        assert!(c.is_empty());
        assert!(p.is_empty());
    }

    #[test]
    fn push_fails_when_full_and_returns_value() {
        let (mut p, mut c) = SpscQueue::with_capacity(2);
        assert!(p.push(10).is_ok());
        assert!(p.push(20).is_ok());
        assert!(p.is_full());
        assert_eq!(p.push(30), Err(30));
        assert_eq!(c.pop(), Some(10));
        assert!(p.push(30).is_ok());
        assert_eq!(c.pop(), Some(20));
        assert_eq!(c.pop(), Some(30));
    }

    #[test]
    fn full_capacity_is_usable() {
        // No slot is sacrificed to distinguish full from empty.
        let (mut p, mut c) = SpscQueue::with_capacity(8);
        for i in 0..8 {
            assert!(p.push(i).is_ok(), "push {i}");
        }
        assert!(p.is_full());
        assert!(c.is_full());
        for i in 0..8 {
            assert_eq!(c.pop(), Some(i));
        }
        assert!(c.is_empty());
    }

    #[test]
    fn wraps_around_many_times() {
        let (mut p, mut c) = SpscQueue::with_capacity(4);
        let mut expected = 0u64;
        for round in 0..1000u64 {
            let n = (round % 4) + 1;
            for k in 0..n {
                assert!(p.push(round * 10 + k).is_ok());
            }
            for k in 0..n {
                assert_eq!(c.pop(), Some(round * 10 + k));
                expected += 1;
            }
        }
        assert_eq!(expected, (1..=4).sum::<u64>() * 250);
        assert!(c.pop().is_none());
    }

    #[test]
    fn peek_does_not_consume() {
        let (mut p, mut c) = SpscQueue::with_capacity(2);
        assert!(c.peek().is_none());
        p.push(7).unwrap();
        assert_eq!(c.peek(), Some(&7));
        assert_eq!(c.peek(), Some(&7));
        assert_eq!(c.len(), 1);
        assert_eq!(c.pop(), Some(7));
        assert!(c.peek().is_none());
    }

    #[test]
    fn push_slice_partial_when_short_on_space() {
        let (mut p, mut c) = SpscQueue::with_capacity(4);
        assert_eq!(p.push_slice(&[1, 2, 3]), 3);
        assert_eq!(p.push_slice(&[4, 5, 6]), 1);
        assert_eq!(p.push_slice(&[9]), 0);
        let mut out = [0; 8];
        assert_eq!(c.pop_slice(&mut out), 4);
        assert_eq!(&out[..4], &[1, 2, 3, 4]);
        assert_eq!(c.pop_slice(&mut out), 0);
    }

    #[test]
    fn pop_slice_respects_output_length() {
        let (mut p, mut c) = SpscQueue::with_capacity(8);
        assert_eq!(p.push_slice(&[1, 2, 3, 4, 5]), 5);
        let mut out = [0; 2];
        assert_eq!(c.pop_slice(&mut out), 2);
        assert_eq!(out, [1, 2]);
        assert_eq!(c.pop_slice(&mut out), 2);
        assert_eq!(out, [3, 4]);
        assert_eq!(c.pop_slice(&mut out), 1);
        assert_eq!(out[0], 5);
    }

    #[test]
    fn push_iter_stops_at_full() {
        let (mut p, mut c) = SpscQueue::with_capacity(4);
        assert_eq!(p.push_iter(0..100), 4);
        assert_eq!(c.pop(), Some(0));
        assert_eq!(p.push_iter(100..200), 1);
        let mut seen = Vec::new();
        c.drain_with(|v| seen.push(v));
        assert_eq!(seen, vec![1, 2, 3, 100]);
    }

    #[test]
    fn drain_with_returns_count_and_order() {
        let (mut p, mut c) = SpscQueue::with_capacity(8);
        assert_eq!(c.drain_with(|_| panic!("nothing to drain")), 0);
        for i in 0..6 {
            p.push(i).unwrap();
        }
        let mut got = Vec::new();
        assert_eq!(c.drain_with(|v| got.push(v)), 6);
        assert_eq!(got, (0..6).collect::<Vec<_>>());
        assert!(c.is_empty());
    }

    #[test]
    fn abandoned_flags() {
        let (p, c) = SpscQueue::<u8>::with_capacity(1);
        assert!(!p.is_abandoned());
        assert!(!c.is_abandoned());
        drop(c);
        assert!(p.is_abandoned());
        let (p, c) = SpscQueue::<u8>::with_capacity(1);
        drop(p);
        assert!(c.is_abandoned());
    }

    struct DropCounter<'a>(&'a StdAtomic);
    impl Drop for DropCounter<'_> {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn queued_values_dropped_exactly_once() {
        let drops = StdAtomic::new(0);
        {
            let (mut p, mut c) = SpscQueue::with_capacity(4);
            for _ in 0..4 {
                p.push(DropCounter(&drops)).ok().unwrap();
            }
            drop(c.pop());
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            p.push(DropCounter(&drops)).ok().unwrap();
            // 4 values remain queued across a wrap boundary.
            drop(p);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            drop(c);
        }
        assert_eq!(drops.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn rejected_push_does_not_leak_or_double_drop() {
        let drops = StdAtomic::new(0);
        let (mut p, c) = SpscQueue::with_capacity(1);
        p.push(DropCounter(&drops)).ok().unwrap();
        let rejected = p.push(DropCounter(&drops));
        assert!(rejected.is_err());
        drop(rejected);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        drop(p);
        drop(c);
        assert_eq!(drops.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn concurrent_sequence_is_lossless_and_ordered() {
        const N: u64 = 200_000;
        let (mut p, mut c) = SpscQueue::with_capacity(64);
        let producer = thread::spawn(move || {
            let mut i = 0u64;
            while i < N {
                if p.push(i).is_ok() {
                    i += 1;
                } else {
                    thread::yield_now();
                }
            }
        });
        let consumer = thread::spawn(move || {
            let mut expect = 0u64;
            while expect < N {
                match c.pop() {
                    Some(v) => {
                        assert_eq!(v, expect);
                        expect += 1;
                    }
                    None => thread::yield_now(),
                }
            }
            assert!(c.pop().is_none());
        });
        producer.join().unwrap();
        consumer.join().unwrap();
    }

    #[test]
    fn concurrent_slice_ops_are_lossless() {
        const N: usize = 100_000;
        let (mut p, mut c) = SpscQueue::with_capacity(32);
        let producer = thread::spawn(move || {
            let data: Vec<u32> = (0..N as u32).collect();
            let mut sent = 0;
            while sent < N {
                let end = (sent + 7).min(N);
                sent += p.push_slice(&data[sent..end]);
                if p.is_full() {
                    thread::yield_now();
                }
            }
        });
        let consumer = thread::spawn(move || {
            let mut buf = [0u32; 5];
            let mut expect = 0u32;
            while (expect as usize) < N {
                let n = c.pop_slice(&mut buf);
                if n == 0 {
                    thread::yield_now();
                    continue;
                }
                for &v in &buf[..n] {
                    assert_eq!(v, expect);
                    expect += 1;
                }
            }
        });
        producer.join().unwrap();
        consumer.join().unwrap();
    }

    #[test]
    fn concurrent_non_copy_payloads_move_intact() {
        const N: usize = 20_000;
        let (mut p, mut c) = SpscQueue::with_capacity(16);
        let producer = thread::spawn(move || {
            for i in 0..N {
                let mut v = Box::new(vec![i; 3]);
                loop {
                    match p.push(v) {
                        Ok(()) => break,
                        Err(back) => {
                            v = back;
                            thread::yield_now();
                        }
                    }
                }
            }
        });
        let consumer = thread::spawn(move || {
            let mut expect = 0;
            while expect < N {
                match c.pop() {
                    Some(v) => {
                        assert_eq!(*v, vec![expect; 3]);
                        expect += 1;
                    }
                    None => thread::yield_now(),
                }
            }
        });
        producer.join().unwrap();
        consumer.join().unwrap();
    }

    #[test]
    fn producer_dropped_mid_stream_leaves_remaining_readable() {
        let (mut p, mut c) = SpscQueue::with_capacity(8);
        for i in 0..5 {
            p.push(i).unwrap();
        }
        drop(p);
        assert!(c.is_abandoned());
        let mut got = Vec::new();
        while let Some(v) = c.pop() {
            got.push(v);
        }
        assert_eq!(got, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn ends_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Producer<Vec<u8>>>();
        assert_send_sync::<Consumer<Vec<u8>>>();
    }

    #[test]
    fn counters_wrap_through_usize_max() {
        let (mut p, mut c) = SpscQueue::with_capacity_at(4, usize::MAX - 1);
        assert!(p.is_empty());
        for i in 0..4 {
            assert!(p.push(i).is_ok(), "push {i}");
        }
        assert!(p.is_full());
        assert_eq!(c.len(), 4);
        assert_eq!(c.pop(), Some(0));
        assert_eq!(c.pop(), Some(1));
        assert!(p.push(4).is_ok());
        assert!(p.push(5).is_ok());
        assert_eq!(p.push(6), Err(6));
        let mut got = Vec::new();
        assert_eq!(c.drain_with(|v| got.push(v)), 4);
        assert_eq!(got, vec![2, 3, 4, 5]);
        assert!(c.is_empty());
    }

    #[test]
    fn drop_releases_values_across_counter_wrap() {
        let drops = StdAtomic::new(0);
        {
            let (mut p, mut c) = SpscQueue::with_capacity_at(4, usize::MAX - 1);
            for _ in 0..3 {
                p.push(DropCounter(&drops)).ok().unwrap();
            }
            drop(c.pop());
            // Two live values straddle the counter wrap: positions
            // usize::MAX and 0.
            drop(p);
            drop(c);
        }
        assert_eq!(drops.load(Ordering::SeqCst), 3);
    }

    struct PanicOnNthClone<'a> {
        counter: &'a StdAtomic,
        drops: &'a StdAtomic,
        panic_at: usize,
    }
    impl Clone for PanicOnNthClone<'_> {
        fn clone(&self) -> Self {
            let n = self.counter.fetch_add(1, Ordering::SeqCst);
            assert!(n != self.panic_at, "clone {n} panics on purpose");
            PanicOnNthClone {
                counter: self.counter,
                drops: self.drops,
                panic_at: self.panic_at,
            }
        }
    }
    impl Drop for PanicOnNthClone<'_> {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn push_slice_publishes_clones_made_before_a_clone_panics() {
        let clones = StdAtomic::new(0);
        let drops = StdAtomic::new(0);
        let items: Vec<PanicOnNthClone<'_>> = (0..4)
            .map(|_| PanicOnNthClone {
                counter: &clones,
                drops: &drops,
                panic_at: 2,
            })
            .collect();
        let (mut p, mut c) = SpscQueue::with_capacity(8);
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| p.push_slice(&items)));
        assert!(result.is_err());
        // Clones 0 and 1 succeeded and must be visible to the consumer.
        assert_eq!(clones.load(Ordering::SeqCst), 3);
        assert_eq!(c.len(), 2);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(c.pop());
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        drop(p);
        drop(c);
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        drop(items);
        assert_eq!(drops.load(Ordering::SeqCst), 6);
    }
}
