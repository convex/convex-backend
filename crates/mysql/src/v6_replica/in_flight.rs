//! Sequence-numbered tracking of mutating source calls, so a comparison waits
//! only for the calls that could have committed before its scan ended.
use std::collections::BTreeSet;

use parking_lot::Mutex;
use tokio::{
    sync::Notify,
    time::Instant,
};

#[derive(Default)]
pub(super) struct InFlightCalls {
    state: Mutex<State>,
    finished: Notify,
}

#[derive(Default)]
struct State {
    next: u64,
    in_flight: BTreeSet<u64>,
}

impl InFlightCalls {
    /// Tracks a call until the guard drops, which must happen after the call
    /// has queued its replay.
    pub(super) fn begin(&self) -> InFlightCall<'_> {
        let mut state = self.state.lock();
        let seq = state.next;
        state.next += 1;
        state.in_flight.insert(seq);
        InFlightCall { calls: self, seq }
    }

    /// The most recent call begun so far, if any.
    pub(super) fn latest(&self) -> Option<u64> {
        self.state.lock().next.checked_sub(1)
    }

    fn finished_through(&self, seq: u64) -> bool {
        self.state
            .lock()
            .in_flight
            .first()
            .is_none_or(|&oldest| oldest > seq)
    }

    /// Waits until every call begun at or before `seq` has finished.
    pub(super) async fn wait_finished_through(&self, seq: u64, deadline: Instant) -> bool {
        loop {
            let notified = self.finished.notified();
            tokio::pin!(notified);
            // Register before checking so a completion between the check and
            // the await still wakes this waiter.
            notified.as_mut().enable();
            if self.finished_through(seq) {
                return true;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return false;
            }
        }
    }
}

pub(super) struct InFlightCall<'a> {
    calls: &'a InFlightCalls,
    seq: u64,
}

impl Drop for InFlightCall<'_> {
    fn drop(&mut self) {
        self.calls.state.lock().in_flight.remove(&self.seq);
        self.calls.finished.notify_waiters();
    }
}
