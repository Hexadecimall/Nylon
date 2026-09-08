//! Publishing the newest value across threads.
//!
//! A queue is the wrong shape for state that only matters when it is
//! current: meters, the playhead, a level readout. If the reader falls
//! behind, a queue keeps the oldest entries and drops the new ones, so the
//! reader sees stale numbers exactly when it needs fresh ones.
//!
//! This is a triple buffer. The writer always has a slot to write into and
//! never waits; the reader always sees the most recently completed value.
//! Both operations are wait-free and neither allocates, so the writer can
//! be the audio callback.
//!
//! Values in flight are overwritten rather than queued. A reader that
//! misses an update has not lost anything it could have used.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Marks the shared slot as holding a value the reader has not taken.
const FRESH: usize = 0b100;
/// Selects the slot index from the shared word.
const INDEX: usize = 0b011;

struct Inner<T> {
    // Three slots: one the writer owns, one the reader owns, and one held
    // in `shared` as the handoff point.
    slots: [UnsafeCell<T>; 3],
    shared: AtomicUsize,
}

// SAFETY: A slot is only ever touched by the endpoint that owns its index.
// Ownership moves between the endpoints through the atomic swap in
// `publish` and `read`, which also orders the writes to the slot against
// the reads of it.
unsafe impl<T: Send> Sync for Inner<T> {}
// SAFETY: Sending the whole buffer moves ownership of every slot together.
unsafe impl<T: Send> Send for Inner<T> {}

/// Writing half of a triple buffer.
pub struct Writer<T> {
    inner: Arc<Inner<T>>,
    /// Slot this endpoint owns and writes into.
    slot: usize,
}

/// Reading half of a triple buffer.
pub struct Reader<T> {
    inner: Arc<Inner<T>>,
    /// Slot this endpoint owns and reads from.
    slot: usize,
}

// SAFETY: Each endpoint is the sole owner of its slot, so moving one to
// another thread keeps the invariant as long as `T` may cross threads.
unsafe impl<T: Send> Send for Writer<T> {}
// SAFETY: As for `Writer`.
unsafe impl<T: Send> Send for Reader<T> {}

/// Builds a triple buffer holding `initial`, returning its two ends.
///
/// All three slots start holding a copy of `initial`, so a reader that
/// runs before the first publication still sees a usable value.
#[must_use]
pub fn latest<T: Clone>(initial: T) -> (Writer<T>, Reader<T>) {
    let inner = Arc::new(Inner {
        slots: [
            UnsafeCell::new(initial.clone()),
            UnsafeCell::new(initial.clone()),
            UnsafeCell::new(initial),
        ],
        // Slot 0 belongs to the writer, slot 1 to the reader, slot 2 is
        // the handoff point and carries nothing new yet.
        shared: AtomicUsize::new(2),
    });
    (
        Writer {
            inner: Arc::clone(&inner),
            slot: 0,
        },
        Reader { inner, slot: 1 },
    )
}

impl<T> Writer<T> {
    /// Publishes `value`, replacing anything the reader has not taken.
    ///
    /// Never waits and never fails: the writer always owns a free slot.
    #[inline]
    pub fn publish(&mut self, value: T) {
        // SAFETY: `slot` is owned by this endpoint until the swap below
        // hands it over, so nothing else reads or writes it here.
        unsafe { *self.inner.slots[self.slot].get() = value };
        // Releasing orders the write above against the reader's acquire.
        let previous = self.inner.shared.swap(self.slot | FRESH, Ordering::AcqRel);
        self.slot = previous & INDEX;
    }

    /// True when the reader has taken the last published value.
    #[inline]
    #[must_use]
    pub fn is_taken(&self) -> bool {
        self.inner.shared.load(Ordering::Acquire) & FRESH == 0
    }
}

impl<T> Reader<T> {
    /// Takes the newest published value, or `None` when nothing new has
    /// been published since the last call.
    ///
    /// Never waits. The reference stays valid until the next call.
    #[inline]
    pub fn read(&mut self) -> Option<&T> {
        if self.inner.shared.load(Ordering::Acquire) & FRESH == 0 {
            return None;
        }
        let previous = self.inner.shared.swap(self.slot, Ordering::AcqRel);
        self.slot = previous & INDEX;
        // SAFETY: The swap transferred ownership of `previous`'s slot to
        // this endpoint and paired with the writer's release, so the value
        // is fully written and nothing else touches it now.
        Some(unsafe { &*self.inner.slots[self.slot].get() })
    }

    /// The value this endpoint holds, taking a newer one first when there
    /// is one. Unlike [`read`](Self::read) this always returns a value.
    #[inline]
    pub fn current(&mut self) -> &T {
        self.read();
        // SAFETY: This endpoint owns `slot`; `read` only ever leaves it
        // owning a slot that holds a completely written value.
        unsafe { &*self.inner.slots[self.slot].get() }
    }

    /// True when a value has been published that this endpoint has not
    /// taken.
    #[inline]
    #[must_use]
    pub fn has_new(&self) -> bool {
        self.inner.shared.load(Ordering::Acquire) & FRESH != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::thread;

    #[test]
    fn a_reader_starts_with_the_initial_value() {
        let (writer, mut reader) = latest(7_u32);
        assert!(!reader.has_new());
        assert_eq!(reader.read(), None);
        assert_eq!(*reader.current(), 7);
        assert!(writer.is_taken());
    }

    #[test]
    fn a_published_value_is_read_once() {
        let (mut writer, mut reader) = latest(0_u32);
        writer.publish(42);
        assert!(reader.has_new());
        assert_eq!(reader.read(), Some(&42));
        assert!(!reader.has_new());
        assert_eq!(reader.read(), None);
        // The value stays readable through `current`.
        assert_eq!(*reader.current(), 42);
    }

    #[test]
    fn only_the_newest_value_survives() {
        let (mut writer, mut reader) = latest(0_u32);
        for value in 1..=100 {
            writer.publish(value);
        }
        assert_eq!(reader.read(), Some(&100));
        assert_eq!(reader.read(), None);
    }

    #[test]
    fn the_writer_never_blocks_on_an_unread_value() {
        let (mut writer, mut reader) = latest(0_u32);
        writer.publish(1);
        assert!(!writer.is_taken());
        // Publishing again with the previous value still unread works.
        writer.publish(2);
        writer.publish(3);
        assert_eq!(reader.read(), Some(&3));
        assert!(writer.is_taken());
    }

    #[test]
    fn reading_and_writing_alternate_cleanly() {
        let (mut writer, mut reader) = latest(0_u32);
        for value in 1..=1_000 {
            writer.publish(value);
            assert_eq!(reader.read(), Some(&value));
        }
    }

    #[test]
    fn large_values_are_carried_whole() {
        // A value bigger than a word, to catch a partially visible write.
        #[derive(Clone, PartialEq, Debug)]
        struct Big {
            values: [u64; 64],
        }
        let (mut writer, mut reader) = latest(Big { values: [0; 64] });
        writer.publish(Big { values: [9; 64] });
        assert_eq!(reader.read(), Some(&Big { values: [9; 64] }));
    }

    #[test]
    fn a_concurrent_reader_never_sees_a_torn_value() {
        // Every published value has all its words equal. A reader that saw
        // a half-written slot would find words that disagree.
        const WORDS: usize = 32;
        #[derive(Clone)]
        struct Block {
            values: [u64; WORDS],
        }
        let (mut writer, mut reader) = latest(Block { values: [0; WORDS] });
        let stop = Arc::new(AtomicBool::new(false));
        let writer_stop = Arc::clone(&stop);

        let producer = thread::spawn(move || {
            for counter in 1..=200_000_u64 {
                writer.publish(Block {
                    values: [counter; WORDS],
                });
            }
            writer_stop.store(true, Ordering::Release);
        });

        let mut seen = 0_u64;
        let mut reads = 0_u64;
        while !stop.load(Ordering::Acquire) {
            if let Some(block) = reader.read() {
                let first = block.values[0];
                assert!(
                    block.values.iter().all(|value| *value == first),
                    "a torn value was read"
                );
                // Values never go backwards.
                assert!(first >= seen, "{first} came after {seen}");
                seen = first;
                reads += 1;
            }
        }
        producer.join().unwrap();
        // Draining afterwards leaves the reader on the final value.
        while reader.read().is_some() {}
        assert_eq!(reader.current().values[0], 200_000);
        assert!(reads > 0, "the reader never saw a value");
    }

    #[test]
    fn the_ends_can_move_between_threads() {
        let (mut writer, mut reader) = latest(0_u32);
        let producer = thread::spawn(move || {
            writer.publish(5);
            writer
        });
        let writer = producer.join().unwrap();
        let consumer = thread::spawn(move || {
            let value = *reader.current();
            (reader, value)
        });
        let (_reader, value) = consumer.join().unwrap();
        assert_eq!(value, 5);
        drop(writer);
    }

    #[test]
    fn values_are_dropped_exactly_once() {
        use std::sync::atomic::AtomicUsize as Counter;
        static DROPS: Counter = Counter::new(0);

        #[derive(Clone)]
        struct Counted;
        impl Drop for Counted {
            fn drop(&mut self) {
                DROPS.fetch_add(1, Ordering::SeqCst);
            }
        }

        DROPS.store(0, Ordering::SeqCst);
        {
            let (mut writer, mut reader) = latest(Counted);
            // Three slots hold the initial value's clones.
            writer.publish(Counted);
            writer.publish(Counted);
            reader.read();
            // Two published values replaced two slot contents.
            assert_eq!(DROPS.load(Ordering::SeqCst), 2);
        }
        // The three slots are dropped with the buffer.
        assert_eq!(DROPS.load(Ordering::SeqCst), 5);
    }
}
