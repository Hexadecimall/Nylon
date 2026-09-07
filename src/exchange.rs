//! Bounded state publication with deferred reclamation.

use crate::spsc::{Consumer, Producer, SpscQueue};

pub struct ControlSlot<T> {
    pending: Producer<T>,
    retired: Consumer<T>,
}

/// Destroy this endpoint only after stopping the callback, on the control thread.
pub struct AudioSlot<T> {
    current: T,
    pending: Consumer<T>,
    retired: Producer<T>,
}

/// Allocate communication storage before playback. Published values transfer
/// ownership; neither endpoint clones their payload during publication.
pub fn exchange<T>(initial: T) -> (ControlSlot<T>, AudioSlot<T>) {
    let (pending_tx, pending_rx) = SpscQueue::with_capacity(1);
    let (retired_tx, retired_rx) = SpscQueue::with_capacity(1);
    (
        ControlSlot {
            pending: pending_tx,
            retired: retired_rx,
        },
        AudioSlot {
            current: initial,
            pending: pending_rx,
            retired: retired_tx,
        },
    )
}

impl<T> ControlSlot<T> {
    /// Return ownership to the caller when publication storage is occupied.
    pub fn publish(&mut self, value: T) -> Result<(), T> {
        self.pending.push(value)
    }
    /// Reclaim obsolete storage on the control thread.
    pub fn reclaim(&mut self) -> Option<T> {
        self.retired.pop()
    }
}

impl<T> AudioSlot<T> {
    pub fn current(&self) -> &T {
        &self.current
    }
    pub fn current_mut(&mut self) -> &mut T {
        &mut self.current
    }

    /// Swap at a block boundary. Defer publication if reclamation is stalled.
    /// No payload is allocated, cloned, or destroyed by a successful swap.
    pub fn apply_pending(&mut self) -> bool {
        if self.retired.is_full() {
            return false;
        }
        let Some(next) = self.pending.pop() else {
            return false;
        };
        let previous = std::mem::replace(&mut self.current, next);
        // Only this endpoint can occupy the checked slot. The other endpoint
        // can only increase free capacity between the check and this write.
        assert!(self.retired.push(previous).is_ok());
        true
    }
}
