use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::oneshot;

use crate::errors::report_error_sync;

/// Used by the database to signal it has encountered a fatal error.
#[derive(Clone)]
pub struct ShutdownSignal {
    mode: Mode,
}

/// Indicates what to do when an error is reported.
#[derive(Clone)]
enum Mode {
    Panic,
    /// If the `Option` inside the mutex is `Some`, the next fatal error will be
    /// sent to that sender. Otherwise, signalling will do nothing (under the
    /// presumption that an earlier error was reported and removed the sender).
    Notify(Arc<Mutex<Option<oneshot::Sender<anyhow::Error>>>>),
}

impl ShutdownSignal {
    /// Creates a new ShutdownSignal that sends the first encountered error to
    /// the provided oneshot sender.
    pub fn new(shutdown_tx: oneshot::Sender<anyhow::Error>) -> Self {
        Self {
            mode: Mode::Notify(Arc::new(Mutex::new(Some(shutdown_tx)))),
        }
    }

    /// Signals that an instance has encountered a fatal error and needs to be
    /// shut down.
    pub fn signal(&self, mut fatal_error: anyhow::Error) {
        report_error_sync(&mut fatal_error);
        match &self.mode {
            Mode::Notify(shutdown_tx_mutex) => {
                let Some(shutdown_tx) = shutdown_tx_mutex.lock().take() else {
                    // A shutdown message has already been sent for this instance. Do nothing.
                    return;
                };
                _ = shutdown_tx.send(fatal_error);
            },
            Mode::Panic => {
                // We don't have the shutdown signal configured. Just panic.
                panic!("Shutting down due to fatal error: {fatal_error}");
            },
        }
    }

    /// Creates a new ShutdownSignal that panics when signaled.
    pub fn panic() -> Self {
        Self { mode: Mode::Panic }
    }

}
