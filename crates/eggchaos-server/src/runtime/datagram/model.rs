//! Datagram configuration model: validated limits, proxy definitions, runtime
//! views, association evidence records, and lifecycle errors. No listener,
//! socket, or task state lives here.
use std::{
    io,
    net::{IpAddr, SocketAddr},
    sync::atomic::AtomicU64,
    time::Duration,
};

use eggchaos_core::{DatagramEvidence, DatagramLivePolicy, DatagramPlan, DatagramQueueLimits};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::time::Instant;

use super::{
    MAX_ASSOCIATION_HISTORY, MAX_DATAGRAM_ASSOCIATIONS, MAX_DATAGRAM_PROXIES, MAX_IDLE_TIMEOUT,
    MAX_INGRESS_BUFFER_BYTES, MAX_INGRESS_QUEUE,
};

/// Global resource bounds for the datagram runtime.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct DatagramRuntimeLimits {
    /// Maximum proxy definitions in this runtime.
    pub max_proxies: usize,
    /// Maximum simultaneously active associations across proxies.
    pub max_associations: usize,
    /// Retained completed association summaries.
    pub history: usize,
    /// Bounded pre-engine ingress queue per association.
    pub ingress_per_association: usize,
    /// Total bytes allowed in pre-engine association ingress queues.
    pub max_ingress_queue_bytes: usize,
}

impl Default for DatagramRuntimeLimits {
    fn default() -> Self {
        Self {
            max_proxies: 128,
            max_associations: 4096,
            history: 1024,
            ingress_per_association: 16,
            max_ingress_queue_bytes: 64 * 1024 * 1024,
        }
    }
}

impl DatagramRuntimeLimits {
    pub fn validate(self) -> Result<Self, DatagramRuntimeError> {
        if self.max_proxies == 0 || self.max_proxies > MAX_DATAGRAM_PROXIES {
            return Err(DatagramRuntimeError::Invalid(
                "max_proxies must be 1..=1024".into(),
            ));
        }
        if self.max_associations == 0 || self.max_associations > MAX_DATAGRAM_ASSOCIATIONS {
            return Err(DatagramRuntimeError::Invalid(
                "max_associations must be 1..=65536".into(),
            ));
        }
        if self.history > MAX_ASSOCIATION_HISTORY {
            return Err(DatagramRuntimeError::Invalid(
                "history exceeds 65536 entries".into(),
            ));
        }
        if self.ingress_per_association == 0 || self.ingress_per_association > MAX_INGRESS_QUEUE {
            return Err(DatagramRuntimeError::Invalid(
                "ingress_per_association must be 1..=1024".into(),
            ));
        }
        if self.max_ingress_queue_bytes == 0
            || self.max_ingress_queue_bytes > MAX_INGRESS_BUFFER_BYTES
        {
            return Err(DatagramRuntimeError::Invalid(
                "max_ingress_queue_bytes must be 1..=1073741824".into(),
            ));
        }
        Ok(self)
    }
}

/// Fixed-target datagram listener definition.
#[derive(Debug, Clone)]
pub struct DatagramProxySpec {
    /// Stable proxy name.
    pub name: String,
    /// Local UDP listener address.
    pub listen: SocketAddr,
    /// Fixed upstream UDP target.
    pub upstream: SocketAddr,
    /// Per-proxy association cap.
    pub max_associations: usize,
    /// Idle expiry, deferred while either engine owns queued datagrams.
    pub association_idle_timeout: Duration,
    /// Shared directional engine queue bounds.
    pub queue_limits: DatagramQueueLimits,
    /// Upstream client-to-target policy.
    pub upstream_policy: DatagramLivePolicy,
    /// Downstream target-to-client policy.
    pub downstream_policy: DatagramLivePolicy,
    /// Stable initial policy seed namespace.
    pub seed: u64,
}

impl DatagramProxySpec {
    /// Create an enabled fixed-target definition with empty directional plans.
    pub fn new(
        name: impl Into<String>,
        listen: SocketAddr,
        upstream: SocketAddr,
        queue_limits: DatagramQueueLimits,
    ) -> Result<Self, DatagramRuntimeError> {
        let seed = 0;
        Ok(Self {
            name: name.into(),
            listen,
            upstream,
            max_associations: 256,
            association_idle_timeout: Duration::from_secs(60),
            queue_limits: queue_limits
                .validate()
                .map_err(|e| DatagramRuntimeError::Invalid(e.into()))?,
            upstream_policy: DatagramLivePolicy::new(DatagramPlan::empty(), seed)
                .map_err(|e| DatagramRuntimeError::Invalid(e.into()))?,
            downstream_policy: DatagramLivePolicy::new(DatagramPlan::empty(), seed)
                .map_err(|e| DatagramRuntimeError::Invalid(e.into()))?,
            seed,
        })
    }

    pub(crate) fn validate(&self, global_associations: usize) -> Result<(), DatagramRuntimeError> {
        if self.name.is_empty()
            || self.name.len() > 128
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return Err(DatagramRuntimeError::Invalid(
                "proxy name must match [A-Za-z0-9._-]+ and be 1..=128 bytes".into(),
            ));
        }
        if self.max_associations == 0 || self.max_associations > global_associations {
            return Err(DatagramRuntimeError::Invalid(
                "per-proxy associations must be nonzero and no greater than the global limit"
                    .into(),
            ));
        }
        if self.association_idle_timeout < Duration::from_millis(1)
            || self.association_idle_timeout > MAX_IDLE_TIMEOUT
        {
            return Err(DatagramRuntimeError::Invalid(
                "association idle timeout must be 1 ms..=24 hours".into(),
            ));
        }
        self.queue_limits
            .validate()
            .map_err(|e| DatagramRuntimeError::Invalid(e.into()))?;
        self.upstream_policy
            .snapshot()
            .plan
            .validate()
            .map_err(|e| DatagramRuntimeError::Invalid(e.into()))?;
        self.downstream_policy
            .snapshot()
            .plan
            .validate()
            .map_err(|e| DatagramRuntimeError::Invalid(e.into()))?;
        let forbidden_target = match self.upstream.ip() {
            IpAddr::V4(ip) => ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast(),
            IpAddr::V6(ip) => ip.is_unspecified() || ip.is_multicast(),
        };
        if self.upstream.port() == 0 || forbidden_target {
            return Err(DatagramRuntimeError::Invalid(
                "upstream must be a concrete unicast socket address with a nonzero port".into(),
            ));
        }
        let forbidden_listener = match self.listen.ip() {
            IpAddr::V4(ip) => ip.is_multicast() || ip.is_broadcast(),
            IpAddr::V6(ip) => ip.is_multicast(),
        };
        if forbidden_listener {
            return Err(DatagramRuntimeError::Invalid(
                "listen address must be unicast or unspecified".into(),
            ));
        }
        Ok(())
    }
}

/// Current runtime status for one datagram proxy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatagramProxyView {
    pub name: String,
    pub listen: SocketAddr,
    pub bound_addr: Option<SocketAddr>,
    pub upstream: SocketAddr,
    pub running: bool,
    pub max_associations: usize,
    pub association_idle_timeout_ms: u64,
    pub max_datagram_size: u64,
    pub max_queued_datagrams: u64,
    pub max_queued_bytes: u64,
    pub seed: u64,
    pub active_associations: usize,
    pub upstream_generation: u64,
    pub downstream_generation: u64,
    pub oversize_datagrams: u64,
    pub association_capacity_rejections: u64,
    pub ingress_queue_overflow: u64,
    pub association_setup_failures: u64,
}

/// Retained or live association evidence. No payload is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatagramAssociationSnapshot {
    pub id: u64,
    pub proxy: String,
    pub client: SocketAddr,
    pub upstream: SocketAddr,
    pub age_ms: u64,
    pub idle_ms: u64,
    pub ingress_datagrams: u64,
    pub ingress_bytes: u64,
    pub egress_datagrams: u64,
    pub egress_bytes: u64,
    pub ingress_queue_overflow: u64,
    pub oversize_datagrams: u64,
    pub administrative_discards: u64,
    pub send_errors: u64,
    pub upstream_evidence: DatagramEvidence,
    pub downstream_evidence: DatagramEvidence,
}

/// Datagram runtime lifecycle and validation failures.
#[derive(Debug, Error)]
pub enum DatagramRuntimeError {
    #[error("invalid datagram proxy: {0}")]
    Invalid(String),
    #[error("datagram proxy already exists: {0}")]
    Duplicate(String),
    #[error("datagram proxy not found: {0}")]
    NotFound(String),
    #[error("datagram proxy listener bind failed: {0}")]
    Bind(#[source] io::Error),
    #[error("datagram runtime has reached its association limit")]
    AssociationLimit,
    #[error("datagram policy conflict: {0}")]
    Conflict(String),
}
#[derive(Default)]
pub(crate) struct ProxyCounters {
    pub(crate) oversize_datagrams: AtomicU64,
    pub(crate) capacity_rejections: AtomicU64,
    pub(crate) ingress_overflow: AtomicU64,
    pub(crate) association_setup_failures: AtomicU64,
}
pub(crate) struct AssociationRecord {
    pub(crate) id: u64,
    pub(crate) proxy: String,
    pub(crate) client: SocketAddr,
    pub(crate) upstream: SocketAddr,
    pub(crate) created: Instant,
    pub(crate) last_activity: Instant,
    pub(crate) ingress_datagrams: u64,
    pub(crate) ingress_bytes: u64,
    pub(crate) egress_datagrams: u64,
    pub(crate) egress_bytes: u64,
    pub(crate) ingress_queue_overflow: u64,
    pub(crate) oversize_datagrams: u64,
    pub(crate) administrative_discards: u64,
    pub(crate) send_errors: u64,
    pub(crate) upstream_evidence: DatagramEvidence,
    pub(crate) downstream_evidence: DatagramEvidence,
}
impl AssociationRecord {
    pub(crate) fn snapshot(&self, now: Instant) -> DatagramAssociationSnapshot {
        DatagramAssociationSnapshot {
            id: self.id,
            proxy: self.proxy.clone(),
            client: self.client,
            upstream: self.upstream,
            age_ms: now
                .saturating_duration_since(self.created)
                .as_millis()
                .min(u64::MAX as u128) as u64,
            idle_ms: now
                .saturating_duration_since(self.last_activity)
                .as_millis()
                .min(u64::MAX as u128) as u64,
            ingress_datagrams: self.ingress_datagrams,
            ingress_bytes: self.ingress_bytes,
            egress_datagrams: self.egress_datagrams,
            egress_bytes: self.egress_bytes,
            ingress_queue_overflow: self.ingress_queue_overflow,
            oversize_datagrams: self.oversize_datagrams,
            administrative_discards: self.administrative_discards,
            send_errors: self.send_errors,
            upstream_evidence: self.upstream_evidence.clone(),
            downstream_evidence: self.downstream_evidence.clone(),
        }
    }
}
