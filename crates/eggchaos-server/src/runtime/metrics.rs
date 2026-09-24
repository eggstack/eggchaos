use eggchaos_core::FAULT_TYPE_NAMES;
use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, Mutex as StdMutex},
};

/// Low-cardinality native metrics counters.
#[derive(Debug, Default)]
pub struct MetricsCounters {
    /// Accepted connection total.
    pub accepted: AtomicU64,
    /// Completed connection total.
    pub completed: AtomicU64,
    /// Rejected connection total (admission limits).
    pub rejected: AtomicU64,
    /// Final outcomes by coarse class (`OUTCOME_CLASS_NAMES` order).
    pub outcomes: [AtomicU64; 8],
    /// Injected graceful termination requests observed at close.
    pub graceful_requests: AtomicU64,
    /// Injected hard-reset requests observed at close.
    pub hard_reset_requests: AtomicU64,
    /// Abortive closes applied at the TCP edge.
    pub reset_applied: AtomicU64,
    /// Abortive closes with no transport to act on.
    pub reset_unsupported: AtomicU64,
    /// Abortive closes attempted but failed.
    pub reset_failed: AtomicU64,
    /// Accepted bytes across all closed connections.
    pub bytes_accepted: AtomicU64,
    /// Forwarded bytes across all closed connections.
    pub bytes_forwarded: AtomicU64,
    /// Discarded bytes across all closed connections.
    pub bytes_discarded: AtomicU64,
    /// Completed live-policy transitions across all closed connections.
    pub transitions: AtomicU64,
    /// V2 schedule runs started (coarse total, no run/fingerprint labels).
    pub schedule_v2_runs: AtomicU64,
    /// V2 schedule events applied (coarse total).
    pub schedule_v2_events: AtomicU64,
    /// V2 schedule events applied after their deadline (`late_by_ns > 0`).
    pub schedule_v2_late_events: AtomicU64,
    /// Bounded per-proxy and activation tables.
    pub tables: StdMutex<MetricTables>,
}

/// Coarse final-outcome classes in counter order.
pub const OUTCOME_CLASS_NAMES: [&str; 8] = [
    "relay_completed",
    "connect_failed",
    "killed_by_operator",
    "service_shutdown",
    "proxy_removed",
    "graceful_termination",
    "hard_reset",
    "relay_error",
];

/// Maximum proxy names retained in metrics tables.
pub const MAX_METRIC_PROXIES: usize = 1024;
/// Maximum (proxy, direction, fault-type) activation series retained.
pub const MAX_METRIC_ACTIVATIONS: usize = 8192;

/// Per-proxy reconciling counters.
#[derive(Debug, Default)]
pub struct PerProxyMetrics {
    /// Connections accepted on this proxy.
    pub accepted: u64,
    /// Connections completed on this proxy.
    pub completed: u64,
    /// Byte counters per direction (0 = upstream, 1 = downstream) as
    /// (accepted, forwarded, discarded).
    pub bytes: [[u64; 3]; 2],
}

/// Bounded low-cardinality metric tables with overflow buckets, so metric
/// memory stays bounded even if proxy names are created in a loop.
#[derive(Debug, Default)]
pub struct MetricTables {
    pub(super) proxies: HashMap<String, PerProxyMetrics>,
    pub(super) overflow_proxy: PerProxyMetrics,
    pub(super) activations: HashMap<(String, String, String), u64>,
    pub(super) overflow_activations: u64,
}

impl MetricTables {
    fn proxy_entry(&mut self, proxy: &str) -> &mut PerProxyMetrics {
        if self.proxies.contains_key(proxy) {
            return self.proxies.get_mut(proxy).expect("checked");
        }
        if self.proxies.len() >= MAX_METRIC_PROXIES {
            return &mut self.overflow_proxy;
        }
        self.proxies.entry(proxy.to_owned()).or_default()
    }
    /// Record an accepted connection on a proxy.
    pub fn record_accept(&mut self, proxy: &str) {
        self.proxy_entry(proxy).accepted += 1;
    }
    /// Record a completed connection with final byte counters.
    pub fn record_complete(&mut self, proxy: &str, upstream: [u64; 3], downstream: [u64; 3]) {
        let entry = self.proxy_entry(proxy);
        entry.completed += 1;
        for (slot, value) in entry.bytes[0].iter_mut().zip(upstream) {
            *slot = slot.saturating_add(value);
        }
        for (slot, value) in entry.bytes[1].iter_mut().zip(downstream) {
            *slot = slot.saturating_add(value);
        }
    }
    /// Record fault-type activations for one direction.
    pub fn record_activations(&mut self, proxy: &str, direction: &str, activations: [u64; 7]) {
        for (index, count) in activations.iter().enumerate() {
            if *count == 0 {
                continue;
            }
            let key = (
                proxy.to_owned(),
                direction.to_owned(),
                FAULT_TYPE_NAMES[index].to_owned(),
            );
            if let Some(slot) = self.activations.get_mut(&key) {
                *slot = slot.saturating_add(*count);
                continue;
            }
            if self.activations.len() >= MAX_METRIC_ACTIVATIONS {
                self.overflow_activations = self.overflow_activations.saturating_add(*count);
                continue;
            }
            self.activations.insert(key, *count);
        }
    }
}
