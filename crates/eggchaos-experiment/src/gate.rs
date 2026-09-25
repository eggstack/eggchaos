//! Shared monotonic experiment epoch gate.
//!
//! One [`EpochGate`] captures a single Tokio monotonic `Instant` and
//! releases it to both the schedule driver and the caller workload.
//! Every schedule deadline is `epoch + compiled_offset`, so the two
//! sides share one schedule clock without any wall-clock or
//! cross-process synchronization claim.
//!
//! Kernel packet transmission, task wake-up latency, and application
//! processing remain observational. The gate synchronizes the schedule
//! clock only.

use std::sync::{Arc, OnceLock};

use tokio::sync::Notify;
use tokio::time::Instant;

#[derive(Debug, Default)]
struct GateInner {
    epoch: OnceLock<Instant>,
    notify: Notify,
}

/// Owner of the experiment start epoch.
///
/// Created during `arm`; exactly one `start` call captures the epoch
/// and releases all waiters. Extra `start` calls return the already
/// captured epoch without recapturing.
#[derive(Debug, Clone, Default)]
pub struct EpochGate {
    inner: Arc<GateInner>,
}

/// Waiter observing the shared epoch.
///
/// Handed to the schedule driver (and any consumer task needing the
/// same clock) before `start` is called.
#[derive(Debug, Clone)]
pub struct EpochWaiter {
    inner: Arc<GateInner>,
}

impl EpochGate {
    /// Create an unstarted gate.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a gate whose epoch is already captured (`Instant::now`).
    /// Used when no caller coordination is needed (e.g. the standalone
    /// server path, which preserves run-entry epoch semantics).
    pub fn started() -> Self {
        let gate = Self::new();
        gate.start();
        gate
    }

    /// Borrow a waiter for the schedule driver or a consumer task.
    /// Waiters must be created before `start` only in the sense that
    /// they observe retained state either way: a waiter created after
    /// `start` resolves immediately.
    pub fn waiter(&self) -> EpochWaiter {
        EpochWaiter {
            inner: self.inner.clone(),
        }
    }

    /// Capture the epoch (exactly once) and release all waiters.
    /// Returns the shared epoch.
    pub fn start(&self) -> Instant {
        let epoch = *self.inner.epoch.get_or_init(Instant::now);
        self.inner.notify.notify_waiters();
        epoch
    }

    /// Return the captured epoch, if `start` has been called.
    pub fn epoch(&self) -> Option<Instant> {
        self.inner.epoch.get().copied()
    }
}

impl EpochWaiter {
    /// Await the shared epoch. Resolves immediately with retained
    /// state if `start` already won.
    pub async fn await_epoch(&self) -> Instant {
        loop {
            if let Some(epoch) = self.inner.epoch.get() {
                return *epoch;
            }
            self.inner.notify.notified().await;
        }
    }

    /// Return the shared epoch without waiting, if captured.
    pub fn try_epoch(&self) -> Option<Instant> {
        self.inner.epoch.get().copied()
    }
}
