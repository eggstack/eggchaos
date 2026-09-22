use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    io,
    net::SocketAddr,
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::Duration,
};

use eggchaos_core::{
    ActiveFault, ChaosStream, Direction, FaultKind, FaultPlan, FaultSpec, LivePolicy,
    PolicyConflict, RngVersion, StreamEvidence, TerminationInfo, TerminationRequest,
    FAULT_TYPE_NAMES,
};
use egress_relay::{HalfClosePolicy, RelayOptions};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Mutex, Notify, RwLock},
    task::{JoinHandle, JoinSet},
    time::timeout,
};
use tokio_util::sync::CancellationToken;

/// Server crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A validated fixed-target proxy definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxySpec {
    /// Stable proxy name.
    pub name: String,
    /// Local listener address. Port zero requests an ephemeral port.
    pub listen: SocketAddr,
    /// Fixed TCP target.
    pub upstream: SocketAddr,
    /// Client-to-target fault plan.
    #[serde(default)]
    pub upstream_faults: FaultPlan,
    /// Target-to-client fault plan.
    #[serde(default)]
    pub downstream_faults: FaultPlan,
    /// Live upstream policy shared with control-plane snapshots.
    #[serde(skip, default)]
    pub upstream_policy: LivePolicy,
    /// Live downstream policy shared with control-plane snapshots.
    #[serde(skip, default)]
    pub downstream_policy: LivePolicy,
    /// Whether the listener is active at start.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Optional per-proxy active connection limit.
    pub max_connections: Option<usize>,
    /// Bounded direct-connect timeout in milliseconds.
    #[serde(with = "duration_millis")]
    pub connect_timeout: Duration,
    /// Deterministic seed namespace.
    #[serde(default)]
    pub seed: u64,
}

fn default_true() -> bool {
    true
}

mod duration_millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(value.as_millis().min(u64::MAX as u128) as u64)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        Ok(Duration::from_millis(u64::deserialize(deserializer)?))
    }
}

impl ProxySpec {
    /// Construct a basic enabled proxy with no faults.
    pub fn new(name: impl Into<String>, listen: SocketAddr, upstream: SocketAddr) -> Self {
        Self {
            name: name.into(),
            listen,
            upstream,
            upstream_faults: FaultPlan::empty(),
            downstream_faults: FaultPlan::empty(),
            upstream_policy: LivePolicy::default(),
            downstream_policy: LivePolicy::default(),
            enabled: true,
            max_connections: None,
            connect_timeout: Duration::from_secs(5),
            seed: 0,
        }
    }
    /// Set an optional per-proxy active connection limit.
    pub fn with_max_connections(mut self, limit: usize) -> Self {
        self.max_connections = Some(limit);
        self
    }
    pub(crate) fn validate(&self) -> Result<(), EggchaosError> {
        if self.name.is_empty() || self.name.len() > 128 {
            return Err(EggchaosError::InvalidProxy(
                "proxy name must be 1..=128 bytes".into(),
            ));
        }
        // Names travel as single URL path segments, so the charset is
        // restricted to unreserved characters.
        if !self
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(EggchaosError::InvalidProxy(
                "proxy name must match [A-Za-z0-9._-]+".into(),
            ));
        }
        self.upstream_faults
            .validate()
            .map_err(EggchaosError::InvalidPlan)?;
        self.downstream_faults
            .validate()
            .map_err(EggchaosError::InvalidPlan)?;
        if self.connect_timeout.is_zero() {
            return Err(EggchaosError::InvalidProxy(
                "connect timeout must be non-zero".into(),
            ));
        }
        Ok(())
    }
    /// Initialize live policies from the configured plans under the
    /// service-scoped seed namespace. Canonical runtime state is the
    /// policy snapshot; the plan fields remain config-origin mirrors that
    /// mutations refresh from published snapshots under the same lock.
    pub fn init_policies(&mut self, service_seed: u64) {
        let namespace = service_seed ^ self.seed;
        self.upstream_policy = LivePolicy::new(self.upstream_faults.clone(), namespace);
        self.downstream_policy = LivePolicy::new(self.downstream_faults.clone(), namespace);
    }
}

/// Global and per-proxy admission bounds.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AdmissionLimits {
    /// Maximum active connections across all proxies.
    pub global_connections: usize,
    /// Maximum retained closed connection summaries.
    pub history: usize,
}

impl Default for AdmissionLimits {
    fn default() -> Self {
        Self {
            global_connections: 1024,
            history: 256,
        }
    }
}

/// Active connection lifecycle state.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ConnectionState {
    /// Accepted but not yet connected to the target.
    Connecting,
    /// Bidirectional relay is active.
    Relaying,
    /// Connection has ended.
    Closed,
}

/// Safe active-connection operational summary.
///
/// Accept-time identity fields are frozen when the connection is
/// registered; `observed_*`/`pending_*`/counter/fault fields report live
/// stream state merged at read time (or final state in history records).
/// No payload bytes are ever recorded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionSnapshot {
    /// Globally unique connection ID.
    pub id: u64,
    /// Proxy name.
    pub proxy: String,
    /// Proxy-local accept ordinal.
    pub ordinal: u64,
    /// Client peer address.
    pub peer: SocketAddr,
    /// Fixed target address.
    pub upstream: SocketAddr,
    /// Current lifecycle state.
    pub state: ConnectionState,
    /// Run-local deterministic connection key.
    pub connection_key: u64,
    /// Seed namespace reference (service seed XOR proxy seed).
    pub seed: u64,
    /// Global configuration generation observed at accept time.
    pub generation: u64,
    /// Upstream policy generation accepted at connection start.
    pub accepted_upstream_generation: u64,
    /// Downstream policy generation accepted at connection start.
    pub accepted_downstream_generation: u64,
    /// Upstream seed namespace accepted at connection start.
    pub accepted_upstream_seed: u64,
    /// Downstream seed namespace accepted at connection start.
    pub accepted_downstream_seed: u64,
    /// Currently compiled upstream policy generation.
    pub observed_upstream_generation: u64,
    /// Currently compiled downstream policy generation.
    pub observed_downstream_generation: u64,
    /// Upstream transition target draining the old generation, if any.
    pub pending_upstream_generation: Option<u64>,
    /// Downstream transition target draining the old generation, if any.
    pub pending_downstream_generation: Option<u64>,
    /// Currently compiled upstream seed namespace.
    pub upstream_seed_namespace: u64,
    /// Currently compiled downstream seed namespace.
    pub downstream_seed_namespace: u64,
    /// Completed upstream live-policy transitions.
    pub upstream_transitions: u64,
    /// Completed downstream live-policy transitions.
    pub downstream_transitions: u64,
    /// Upstream byte counters.
    pub upstream_bytes: DirectionBytes,
    /// Downstream byte counters.
    pub downstream_bytes: DirectionBytes,
    /// Connection-active upstream fault identities (bounded, no payloads).
    pub upstream_faults: Vec<ActiveFault>,
    /// Whether the upstream fault list truncated at the evidence bound.
    pub upstream_faults_truncated: bool,
    /// Connection-active downstream fault identities.
    pub downstream_faults: Vec<ActiveFault>,
    /// Whether the downstream fault list truncated at the evidence bound.
    pub downstream_faults_truncated: bool,
    /// Deterministic RNG contract version.
    pub rng_version: RngVersion,
}

/// Accepted/forwarded/discarded byte counters for one direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectionBytes {
    /// Bytes accepted from the caller side.
    pub accepted: u64,
    /// Bytes forwarded to the inner transport.
    pub forwarded: u64,
    /// Bytes intentionally discarded by a configured fault.
    pub discarded: u64,
}

/// Lock-shared live evidence for one connection, updated by its streams.
#[derive(Debug, Clone)]
pub struct ConnectionEvidence {
    /// Target-to-client direction evidence.
    pub upstream: Arc<StreamEvidence>,
    /// Client-to-target direction evidence.
    pub downstream: Arc<StreamEvidence>,
}

/// How a hard-reset request resolved at the concrete transport edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResetResult {
    /// An abortive close was initiated through a supported socket API.
    /// Wire-level RST observation remains platform-dependent (M013).
    Applied,
    /// The reset was attempted but the socket operation failed.
    Failed(String),
    /// No resettable transport existed when the request resolved (for
    /// example the relay had already failed and the sockets were gone).
    Unsupported(String),
}

/// Final classification for a closed connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionOutcome {
    /// The relay completed without injected termination or operator action.
    RelayCompleted,
    /// Upstream dial failed or timed out.
    ConnectFailed(String),
    /// An operator killed this connection explicitly.
    KilledByOperator,
    /// Service shutdown terminated this connection.
    ServiceShutdown,
    /// Its proxy was deleted or disabled while active.
    ProxyRemoved,
    /// A graceful termination request resolved. `drained` is true when the
    /// accepted prefix was delivered before close; a grace-timeout abort
    /// records false conservatively.
    GracefulTermination {
        /// Whether delivery completed before close.
        drained: bool,
    },
    /// A hard-reset request resolved at the TCP edge.
    HardReset {
        /// Client-side socket result.
        client: ResetResult,
        /// Upstream-side socket result.
        upstream: ResetResult,
    },
    /// The relay failed with an I/O error unrelated to injected termination.
    RelayError(String),
}

/// A bounded retained final connection record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosedConnection {
    /// Final snapshot with `state == Closed`.
    pub snapshot: ConnectionSnapshot,
    /// Final classification.
    pub outcome: ConnectionOutcome,
    /// Upstream termination evidence, if any fault requested it.
    pub upstream_termination: Option<TerminationInfo>,
    /// Downstream termination evidence, if any fault requested it.
    pub downstream_termination: Option<TerminationInfo>,
    /// Short machine-safe detail (relay report, failure, truncation note).
    pub detail: Option<String>,
}

/// Shared abortive-close control for one TCP socket.
///
/// The relay owns the wrapped streams, so the connection task cannot reclaim
/// them to call socket options directly. Instead the wrapper carries shared
/// flags: on a hard-reset request the task sets the flag and drops the relay
/// future, and the wrapper's `Drop` applies `SO_LINGER=0` before close,
/// which produces an abortive close (RST) on supported platforms. The
/// outcome is recorded into the shared slot so evidence stays truthful.
#[derive(Debug, Default)]
pub struct TcpResetHandle {
    reset_on_close: AtomicBool,
    outcome: StdMutex<Option<ResetResult>>,
}

impl TcpResetHandle {
    /// Request an abortive close when the socket is released.
    pub fn request_reset(&self) {
        self.reset_on_close.store(true, Ordering::Release);
    }
    /// Return the recorded close outcome, if the socket was released.
    pub fn outcome(&self) -> Option<ResetResult> {
        self.outcome.lock().ok()?.clone()
    }
}

/// A `TcpStream` wrapper that can apply an abortive close on drop when its
/// shared handle requests it. Ordinary drops close gracefully (FIN).
pub struct ResettableTcpStream {
    inner: Option<TcpStream>,
    handle: Arc<TcpResetHandle>,
}

impl ResettableTcpStream {
    /// Wrap a socket, returning the stream and its shared reset handle.
    pub fn new(stream: TcpStream) -> (Self, Arc<TcpResetHandle>) {
        let handle = Arc::new(TcpResetHandle::default());
        (
            Self {
                inner: Some(stream),
                handle: handle.clone(),
            },
            handle,
        )
    }
}

impl Drop for ResettableTcpStream {
    fn drop(&mut self) {
        let Some(stream) = self.inner.take() else {
            return;
        };
        if !self.reset_on_close_requested() {
            return;
        }
        // Abortive close through stable APIs only: converting to a
        // socket2 handle lets us set zero linger before close, which
        // discards buffers and emits RST on platforms that honor it.
        // `std::net::TcpStream::set_linger` is still unstable, so socket2
        // is used instead of unstable standard APIs or unsafe code.
        let result = stream
            .into_std()
            .map_err(|error| error.to_string())
            .and_then(|std_stream| {
                socket2::Socket::from(std_stream)
                    .set_linger(Some(Duration::ZERO))
                    .map_err(|error| error.to_string())
                // Dropping the socket here closes abortively (RST) on
                // platforms that honor zero linger.
            });
        let outcome = match result {
            Ok(()) => ResetResult::Applied,
            Err(reason) => ResetResult::Failed(reason),
        };
        if let Ok(mut slot) = self.handle.outcome.lock() {
            *slot = Some(outcome);
        }
    }
}

impl ResettableTcpStream {
    fn reset_on_close_requested(&self) -> bool {
        self.handle.reset_on_close.load(Ordering::Acquire)
    }
}

impl tokio::io::AsyncRead for ResettableTcpStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let stream = self
            .inner
            .as_mut()
            .expect("ResettableTcpStream is live while polled");
        std::pin::Pin::new(stream).poll_read(cx, buffer)
    }
}

impl tokio::io::AsyncWrite for ResettableTcpStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        let stream = self
            .inner
            .as_mut()
            .expect("ResettableTcpStream is live while polled");
        std::pin::Pin::new(stream).poll_write(cx, bytes)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let stream = self
            .inner
            .as_mut()
            .expect("ResettableTcpStream is live while polled");
        std::pin::Pin::new(stream).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let stream = self
            .inner
            .as_mut()
            .expect("ResettableTcpStream is live while polled");
        std::pin::Pin::new(stream).poll_shutdown(cx)
    }
    fn is_write_vectored(&self) -> bool {
        self.inner
            .as_ref()
            .map(tokio::io::AsyncWrite::is_write_vectored)
            .unwrap_or(false)
    }
}

/// Partial proxy update. Fault plans change only through fault CRUD; listen
/// and upstream changes are restart-class and go through stop/bind/swap
/// with documented rollback.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyPatch {
    /// Replacement listener address (restart-class).
    pub listen: Option<SocketAddr>,
    /// Replacement fixed target (restart-class).
    pub upstream: Option<SocketAddr>,
    /// Enable or disable the listener lifecycle.
    pub enabled: Option<bool>,
    /// Replacement per-proxy connection cap (`None` clears the cap).
    #[serde(default)]
    pub max_connections: Option<Option<usize>>,
    /// Replacement connect timeout in milliseconds.
    pub connect_timeout_ms: Option<u64>,
}

/// Fault creation body: direction plus a complete fault specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultUpsert {
    /// Direction the fault applies to.
    pub direction: Direction,
    /// Stable fault identity (1..=128 bytes).
    pub id: String,
    /// Activation probability, defaults to 1.0.
    #[serde(default = "default_probability")]
    pub probability: f64,
    /// Fault behavior.
    pub kind: FaultKind,
}

fn default_probability() -> f64 {
    1.0
}

/// Fault update body: only supplied fields change. The fault identity and
/// direction are fixed; moving a fault across directions is delete plus add.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FaultPatch {
    /// Replacement activation probability.
    pub probability: Option<f64>,
    /// Replacement fault behavior.
    pub kind: Option<FaultKind>,
}

/// Operator-visible proxy state: configured definition plus actual runtime
/// listener state. A successful mutation reflects an actual listener, never
/// a map entry alone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyView {
    /// Stable proxy name.
    pub name: String,
    /// Configured listener address.
    pub listen: SocketAddr,
    /// Configured fixed target.
    pub upstream: SocketAddr,
    /// Actual bound listener address, when running.
    pub bound_addr: Option<SocketAddr>,
    /// Whether a supervised listener is currently running.
    pub running: bool,
    /// Whether the proxy is enabled (desired state).
    pub enabled: bool,
    /// Client-to-target fault plan, read from the same atomic snapshot as
    /// `upstream_generation`.
    pub upstream_faults: FaultPlan,
    /// Target-to-client fault plan, read from the same atomic snapshot as
    /// `downstream_generation`.
    pub downstream_faults: FaultPlan,
    /// Live upstream policy generation matching `upstream_faults`.
    pub upstream_generation: u64,
    /// Live downstream policy generation matching `downstream_faults`.
    pub downstream_generation: u64,
    /// Live upstream seed namespace matching `upstream_faults`.
    pub upstream_seed_namespace: u64,
    /// Live downstream seed namespace matching `downstream_faults`.
    pub downstream_seed_namespace: u64,
    /// Per-proxy active connection limit.
    pub max_connections: Option<usize>,
    /// Direct-connect timeout in milliseconds.
    pub connect_timeout_ms: u64,
    /// Deterministic seed namespace.
    pub seed: u64,
}

/// Native reset report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetReport {
    /// Generation after the reset transaction committed.
    pub generation: u64,
    /// Always true when the transaction committed.
    pub reset: bool,
    /// Proxies that could not be re-enabled because their bind failed.
    /// Those proxies retain their definitions and stay disabled.
    pub failed_enables: Vec<String>,
}

/// Typed control-plane failures with stable machine codes.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ControlError {
    /// Named proxy does not exist.
    #[error("proxy not found: {0}")]
    NotFound(String),
    /// Name or fault identity already exists.
    #[error("conflict: {0}")]
    Conflict(String),
    /// Request or resulting state is invalid.
    #[error("invalid: {0}")]
    Invalid(String),
    /// Listener bind failed; no proxy was registered.
    #[error("bind failed for proxy {proxy}: {reason}")]
    BindFailed {
        /// Proxy name.
        proxy: String,
        /// Underlying bind error.
        reason: String,
    },
    /// Restart-class update failed; the proxy is left in the documented
    /// rollback state described in `reason`.
    #[error("restart failed for proxy {proxy}: {reason}")]
    RestartFailed {
        /// Proxy name.
        proxy: String,
        /// Rollback outcome and current state.
        reason: String,
    },
}

impl ControlError {
    /// Stable machine code for the JSON error envelope.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Invalid(_) => "invalid",
            Self::BindFailed { .. } => "bind_failed",
            Self::RestartFailed { .. } => "restart_failed",
        }
    }
}

/// Errors from service construction, binding, or lifecycle.
#[derive(Debug, Error)]
pub enum EggchaosError {
    /// Proxy definition is invalid.
    #[error("invalid proxy: {0}")]
    InvalidProxy(String),
    /// Fault plan is invalid.
    #[error(transparent)]
    InvalidPlan(#[from] eggchaos_core::ValidationError),
    /// A proxy name is duplicated.
    #[error("duplicate proxy name: {0}")]
    DuplicateProxy(String),
    /// Control-plane mutation failed.
    #[error(transparent)]
    Control(#[from] ControlError),
    /// Listener or upstream I/O error.
    #[error("{context}: {source}")]
    Io {
        /// Operation context.
        context: String,
        /// Underlying I/O error.
        source: io::Error,
    },
    /// A listener task failed.
    #[error("listener task failed: {0}")]
    Join(String),
}

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
    proxies: HashMap<String, PerProxyMetrics>,
    overflow_proxy: PerProxyMetrics,
    activations: HashMap<(String, String, String), u64>,
    overflow_activations: u64,
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

/// Coarse outcome class index for `MetricsCounters::outcomes`.
pub const fn outcome_class(outcome: &ConnectionOutcome) -> usize {
    match outcome {
        ConnectionOutcome::RelayCompleted => 0,
        ConnectionOutcome::ConnectFailed(_) => 1,
        ConnectionOutcome::KilledByOperator => 2,
        ConnectionOutcome::ServiceShutdown => 3,
        ConnectionOutcome::ProxyRemoved => 4,
        ConnectionOutcome::GracefulTermination { .. } => 5,
        ConnectionOutcome::HardReset { .. } => 6,
        ConnectionOutcome::RelayError(_) => 7,
    }
}

/// Runtime tuning shared by every proxy of a service.
#[derive(Debug, Clone)]
pub struct RuntimeParams {
    /// Run seed for deterministic connection keys.
    pub seed: u64,
    /// Admission and history bounds.
    pub limits: AdmissionLimits,
    /// Relay half-close policy.
    pub half_close: HalfClosePolicy,
    /// Bounded relay copy buffer.
    pub relay_buffer: NonZeroUsize,
    /// Grace period for a graceful termination to drain before the relay
    /// is aborted. Bounded; aborts record `drained: false`.
    pub term_grace: Duration,
}

impl Default for RuntimeParams {
    fn default() -> Self {
        Self {
            seed: 0,
            limits: AdmissionLimits::default(),
            half_close: HalfClosePolicy::Drain,
            relay_buffer: NonZeroUsize::new(64 * 1024).expect("non-zero buffer"),
            term_grace: Duration::from_secs(5),
        }
    }
}

/// Level-triggered supervisor completion signal. The flag is set through a
/// `Drop` guard so even a panicking supervisor releases waiters; control
/// operations join on the flag and then drop the finished `JoinHandle`, so
/// no supervisor task is ever detached while unfinished.
#[derive(Debug, Default)]
struct SupervisorDone {
    flag: AtomicBool,
    notify: Notify,
}

impl SupervisorDone {
    async fn join(&self) {
        loop {
            let notified = self.notify.notified();
            if self.flag.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }

    fn finish(&self) {
        self.flag.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }
}

struct DoneGuard(Arc<SupervisorDone>);

impl Drop for DoneGuard {
    fn drop(&mut self) {
        self.0.finish();
    }
}

struct ManagedProxy {
    spec: ProxySpec,
    running: bool,
    bound_addr: Option<SocketAddr>,
    cancel: CancellationToken,
    done: Arc<SupervisorDone>,
    active: Arc<AtomicUsize>,
    ordinal: Arc<AtomicU64>,
}

impl Clone for ManagedProxy {
    fn clone(&self) -> Self {
        Self {
            spec: self.spec.clone(),
            running: self.running,
            bound_addr: self.bound_addr,
            cancel: self.cancel.clone(),
            done: self.done.clone(),
            active: self.active.clone(),
            ordinal: self.ordinal.clone(),
        }
    }
}

/// The single runtime mutation authority: proxy definitions, listener
/// ownership, connection registries, bounded history, metrics, and task
/// supervision all live here. Native admin, CLI (via HTTP), compatibility
/// adapters, and scenarios mutate through these typed methods; no caller
/// maintains a parallel listener registry.
pub struct RuntimeInner {
    params: RuntimeParams,
    proxies: RwLock<BTreeMap<String, ManagedProxy>>,
    generation: AtomicU64,
    next_conn_id: AtomicU64,
    active: AtomicUsize,
    connections: RwLock<HashMap<u64, ConnectionSnapshot>>,
    cancellations: RwLock<HashMap<u64, CancellationToken>>,
    evidence: RwLock<HashMap<u64, ConnectionEvidence>>,
    history: RwLock<VecDeque<ClosedConnection>>,
    metrics: MetricsCounters,
    shutdown_initiated: AtomicBool,
    shutdown_token: CancellationToken,
    /// Owned supervisor tasks by proxy name. Entries are removed only after
    /// the supervisor's done-flag confirms completion, so dropping a
    /// `JoinHandle` here never detaches a running task.
    root: Mutex<Vec<(String, JoinHandle<()>)>>,
    /// Owned scenario run tasks. Shutdown cancels their tokens (children
    /// of the service token) and then joins every task, so no scenario
    /// outlives the service untracked.
    scenario_tasks: Mutex<JoinSet<()>>,
    /// Scenario run records by run ID, bounded (finished runs prune FIFO).
    scenario_runs: Mutex<BTreeMap<u64, crate::scenario::ScenarioRunRecord>>,
    /// Cancellation tokens for active scenario runs.
    scenario_tokens: Mutex<HashMap<u64, CancellationToken>>,
    /// Next scenario run ID.
    next_run_id: AtomicU64,
}

impl RuntimeInner {
    fn new(params: RuntimeParams) -> Self {
        let shutdown_token = CancellationToken::new();
        let _ = &shutdown_token;
        Self {
            params,
            proxies: RwLock::new(BTreeMap::new()),
            generation: AtomicU64::new(1),
            next_conn_id: AtomicU64::new(1),
            active: AtomicUsize::new(0),
            connections: RwLock::new(HashMap::new()),
            cancellations: RwLock::new(HashMap::new()),
            evidence: RwLock::new(HashMap::new()),
            history: RwLock::new(VecDeque::new()),
            metrics: MetricsCounters::default(),
            shutdown_initiated: AtomicBool::new(false),
            shutdown_token,
            root: Mutex::new(Vec::new()),
            scenario_tasks: Mutex::new(JoinSet::new()),
            scenario_runs: Mutex::new(BTreeMap::new()),
            scenario_tokens: Mutex::new(HashMap::new()),
            next_run_id: AtomicU64::new(1),
        }
    }

    fn view_of(entry: &ManagedProxy) -> ProxyView {
        // Both plans come from the same atomic snapshot load as the
        // reported generation, so GET can never pair a plan with an older
        // or newer generation than the one it was published with.
        let upstream = entry.spec.upstream_policy.snapshot();
        let downstream = entry.spec.downstream_policy.snapshot();
        ProxyView {
            name: entry.spec.name.clone(),
            listen: entry.spec.listen,
            upstream: entry.spec.upstream,
            bound_addr: entry.bound_addr,
            running: entry.running,
            enabled: entry.spec.enabled,
            upstream_faults: (*upstream.plan).clone(),
            downstream_faults: (*downstream.plan).clone(),
            upstream_generation: upstream.generation,
            downstream_generation: downstream.generation,
            upstream_seed_namespace: upstream.seed_namespace,
            downstream_seed_namespace: downstream.seed_namespace,
            max_connections: entry.spec.max_connections,
            connect_timeout_ms: entry.spec.connect_timeout.as_millis().min(u64::MAX as u128) as u64,
            seed: entry.spec.seed,
        }
    }
}

/// Shared typed mutation authority used by native and compatibility APIs.
#[derive(Clone)]
pub struct ControlState {
    runtime: Arc<RuntimeInner>,
}

/// Expected-generation publication bundle for
/// [`ControlState::publish_plans_expected`].
#[derive(Debug, Clone)]
pub struct ExpectedPublish {
    /// Replacement client-to-target plan.
    pub upstream: FaultPlan,
    /// Replacement target-to-client plan.
    pub downstream: FaultPlan,
    /// Seed namespace for the upstream publication.
    pub upstream_seed: u64,
    /// Seed namespace for the downstream publication.
    pub downstream_seed: u64,
    /// Upstream generation the publisher based on.
    pub expected_upstream: u64,
    /// Downstream generation the publisher based on.
    pub expected_downstream: u64,
}

impl Default for ControlState {
    fn default() -> Self {
        Self {
            runtime: Arc::new(RuntimeInner::new(RuntimeParams::default())),
        }
    }
}

impl ControlState {
    /// Create state holding validated but not yet started definitions.
    /// Definitions report `running: false` until `start_all` or
    /// `create_proxy` binds them, so reads never claim a listener that
    /// does not exist.
    pub fn new(proxies: impl IntoIterator<Item = ProxySpec>) -> Self {
        let state = Self::default();
        if let Ok(mut map) = state.runtime.proxies.try_write() {
            for mut proxy in proxies {
                if map.contains_key(&proxy.name) {
                    continue;
                }
                proxy.init_policies(state.runtime.params.seed);
                map.insert(
                    proxy.name.clone(),
                    ManagedProxy {
                        spec: proxy,
                        running: false,
                        bound_addr: None,
                        cancel: state.runtime.shutdown_token.child_token(),
                        done: Arc::new(SupervisorDone::default()),
                        active: Arc::new(AtomicUsize::new(0)),
                        ordinal: Arc::new(AtomicU64::new(1)),
                    },
                );
            }
        }
        state
    }

    /// Create state with explicit runtime tuning.
    pub fn with_params(params: RuntimeParams) -> Self {
        Self {
            runtime: Arc::new(RuntimeInner::new(params)),
        }
    }

    /// Render low-cardinality Prometheus text. Labels are bounded by
    /// construction: proxy names come from a capped table, directions and
    /// fault types are fixed vocabularies, and no connection ID, peer
    /// address, scenario run, or arbitrary fault ID ever becomes a label.
    pub async fn metrics_text(&self) -> String {
        let mut text = format!(
            "# HELP eggchaos_config_generation Current configuration generation\n# TYPE eggchaos_config_generation gauge\neggchaos_config_generation {}\n# HELP eggchaos_connections_accepted_total Accepted connections\n# TYPE eggchaos_connections_accepted_total counter\neggchaos_connections_accepted_total {}\n# HELP eggchaos_connections_completed_total Completed connections\n# TYPE eggchaos_connections_completed_total counter\neggchaos_connections_completed_total {}\n# HELP eggchaos_connections_rejected_total Rejected connections\n# TYPE eggchaos_connections_rejected_total counter\neggchaos_connections_rejected_total {}\n# HELP eggchaos_connections_active Active connections\n# TYPE eggchaos_connections_active gauge\neggchaos_connections_active {}\n",
            self.generation(),
            self.runtime.metrics.accepted.load(Ordering::Relaxed),
            self.runtime.metrics.completed.load(Ordering::Relaxed),
            self.runtime.metrics.rejected.load(Ordering::Relaxed),
            self.runtime.active.load(Ordering::Relaxed),
        );
        for (index, name) in OUTCOME_CLASS_NAMES.iter().enumerate() {
            text.push_str(&format!(
                "eggchaos_connection_outcomes_total{{outcome=\"{name}\"}} {}\n",
                self.runtime.metrics.outcomes[index].load(Ordering::Relaxed),
            ));
        }
        text.push_str(&format!(
            "# HELP eggchaos_termination_requests_total Injected termination requests observed at close\n# TYPE eggchaos_termination_requests_total counter\neggchaos_termination_requests_total{{request=\"graceful\"}} {}\neggchaos_termination_requests_total{{request=\"hard_reset\"}} {}\n# HELP eggchaos_reset_results_total Abortive-close outcomes at the TCP edge\n# TYPE eggchaos_reset_results_total counter\neggchaos_reset_results_total{{result=\"applied\"}} {}\neggchaos_reset_results_total{{result=\"unsupported\"}} {}\neggchaos_reset_results_total{{result=\"failed\"}} {}\n# HELP eggchaos_bytes_total Bytes by flow across closed connections\n# TYPE eggchaos_bytes_total counter\neggchaos_bytes_total{{flow=\"accepted\"}} {}\neggchaos_bytes_total{{flow=\"forwarded\"}} {}\neggchaos_bytes_total{{flow=\"discarded\"}} {}\n# HELP eggchaos_policy_transitions_total Completed live-policy transitions\n# TYPE eggchaos_policy_transitions_total counter\neggchaos_policy_transitions_total {}\n",
            self.runtime.metrics.graceful_requests.load(Ordering::Relaxed),
            self.runtime.metrics.hard_reset_requests.load(Ordering::Relaxed),
            self.runtime.metrics.reset_applied.load(Ordering::Relaxed),
            self.runtime.metrics.reset_unsupported.load(Ordering::Relaxed),
            self.runtime.metrics.reset_failed.load(Ordering::Relaxed),
            self.runtime.metrics.bytes_accepted.load(Ordering::Relaxed),
            self.runtime.metrics.bytes_forwarded.load(Ordering::Relaxed),
            self.runtime.metrics.bytes_discarded.load(Ordering::Relaxed),
            self.runtime.metrics.transitions.load(Ordering::Relaxed),
        ));
        {
            let tables = self.runtime.metrics.tables.lock().expect("metrics lock");
            let mut proxies: Vec<_> = tables.proxies.iter().collect();
            proxies.sort_by_key(|(name, _)| *name);
            for (name, entry) in proxies {
                text.push_str(&format!(
                    "eggchaos_proxy_connections_accepted_total{{proxy=\"{name}\"}} {}\neggchaos_proxy_connections_completed_total{{proxy=\"{name}\"}} {}\n",
                    entry.accepted, entry.completed,
                ));
                for (direction, index) in [("upstream", 0), ("downstream", 1)] {
                    for (flow, value) in ["accepted", "forwarded", "discarded"]
                        .iter()
                        .zip(entry.bytes[index])
                    {
                        text.push_str(&format!(
                            "eggchaos_proxy_bytes_total{{proxy=\"{name}\",direction=\"{direction}\",flow=\"{flow}\"}} {value}\n",
                        ));
                    }
                }
            }
            if tables.overflow_proxy.accepted + tables.overflow_proxy.completed > 0 {
                text.push_str(&format!(
                    "eggchaos_proxy_connections_accepted_total{{proxy=\"_overflow\"}} {}\neggchaos_proxy_connections_completed_total{{proxy=\"_overflow\"}} {}\n",
                    tables.overflow_proxy.accepted, tables.overflow_proxy.completed,
                ));
            }
            let mut activations: Vec<_> = tables.activations.iter().collect();
            activations.sort();
            for ((proxy, direction, fault_type), count) in activations {
                text.push_str(&format!(
                    "eggchaos_fault_activations_total{{proxy=\"{proxy}\",direction=\"{direction}\",fault_type=\"{fault_type}\"}} {count}\n",
                ));
            }
            if tables.overflow_activations > 0 {
                text.push_str(&format!(
                    "eggchaos_fault_activations_total{{proxy=\"_overflow\",direction=\"_overflow\",fault_type=\"_overflow\"}} {}\n",
                    tables.overflow_activations,
                ));
            }
        }
        // Live gauges read directly from policies: per-proxy active
        // connections, queued bytes summed from live evidence, and current
        // policy generations.
        let map = self.runtime.proxies.read().await;
        let evidence = self.runtime.evidence.read().await;
        let connections = self.runtime.connections.read().await;
        let mut names: Vec<_> = map.keys().collect();
        names.sort();
        // Queue bytes per proxy+direction from live evidence handles.
        let mut queued: HashMap<(&str, &str), u64> = HashMap::new();
        for snapshot in connections.values() {
            let Some(record) = evidence.get(&snapshot.id) else {
                continue;
            };
            for (handle, direction) in [
                (&record.upstream, "upstream"),
                (&record.downstream, "downstream"),
            ] {
                let (accepted, forwarded, discarded) = handle.byte_counts();
                let current = accepted.saturating_sub(forwarded.saturating_add(discarded));
                let slot = queued
                    .entry((snapshot.proxy.as_str(), direction))
                    .or_default();
                *slot = slot.saturating_add(current);
            }
        }
        for name in names {
            let entry = &map[name];
            let upstream = entry.spec.upstream_policy.snapshot();
            let downstream = entry.spec.downstream_policy.snapshot();
            text.push_str(&format!(
                "eggchaos_proxy_connections_active{{proxy=\"{name}\"}} {}\neggchaos_proxy_policy_generation{{proxy=\"{name}\",direction=\"upstream\"}} {}\neggchaos_proxy_policy_generation{{proxy=\"{name}\",direction=\"downstream\"}} {}\neggchaos_proxy_queue_bytes{{proxy=\"{name}\",direction=\"upstream\"}} {}\neggchaos_proxy_queue_bytes{{proxy=\"{name}\",direction=\"downstream\"}} {}\n",
                entry.active.load(Ordering::Relaxed),
                upstream.generation,
                downstream.generation,
                queued.get(&(name.as_str(), "upstream")).copied().unwrap_or(0),
                queued.get(&(name.as_str(), "downstream")).copied().unwrap_or(0),
            ));
        }
        text
    }

    /// Current configuration generation.
    pub fn generation(&self) -> u64 {
        self.runtime.generation.load(Ordering::Acquire)
    }

    fn next_generation(&self) -> u64 {
        self.runtime.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Snapshot all proxy views with actual listener state.
    pub async fn list(&self) -> Vec<ProxyView> {
        self.runtime
            .proxies
            .read()
            .await
            .values()
            .map(RuntimeInner::view_of)
            .collect()
    }

    /// Get one proxy view.
    pub async fn get(&self, name: &str) -> Option<ProxyView> {
        self.runtime
            .proxies
            .read()
            .await
            .get(name)
            .map(RuntimeInner::view_of)
    }

    /// Snapshot active runtime connections, merging live stream evidence
    /// (observed/pending generations, counters, active faults) at read
    /// time. Connections whose evidence is not yet registered report
    /// accept-time values.
    pub async fn connections(&self) -> Vec<ConnectionSnapshot> {
        let map = self.runtime.connections.read().await;
        let evidence = self.runtime.evidence.read().await;
        map.values()
            .map(|snapshot| merge_evidence(snapshot, evidence.get(&snapshot.id)))
            .collect()
    }

    /// Snapshot one active connection with live evidence merged.
    pub async fn get_connection(&self, id: u64) -> Option<ConnectionSnapshot> {
        let map = self.runtime.connections.read().await;
        let snapshot = map.get(&id)?.clone();
        let evidence = self.runtime.evidence.read().await;
        Some(merge_evidence(&snapshot, evidence.get(&id)))
    }

    /// Snapshot bounded closed-connection history, newest last.
    pub async fn history(&self) -> Vec<ClosedConnection> {
        self.runtime.history.read().await.iter().cloned().collect()
    }

    /// Terminate an active connection, if present. Cancellation is
    /// level-triggered: a kill issued before the relay begins waiting is
    /// still observed. Returns true only when the ID was active.
    pub async fn kill(&self, id: u64) -> bool {
        let token = self.runtime.cancellations.write().await.remove(&id);
        if let Some(token) = token {
            token.cancel();
            true
        } else {
            false
        }
    }

    /// Store a validated definition without binding. Live policies are
    /// initialized from the configured plans so reads never diverge from
    /// what streams will compile. The proxy reports `running: false`
    /// until started.
    pub async fn import_definition(&self, mut proxy: ProxySpec) -> Result<u64, ControlError> {
        proxy.validate().map_err(|error| match error {
            EggchaosError::InvalidProxy(message) => ControlError::Invalid(message),
            EggchaosError::InvalidPlan(error) => {
                ControlError::Invalid(format!("invalid fault plan: {error}"))
            }
            other => ControlError::Invalid(other.to_string()),
        })?;
        proxy.init_policies(self.runtime.params.seed);
        let mut map = self.runtime.proxies.write().await;
        if map.contains_key(&proxy.name) {
            return Err(ControlError::Conflict(format!(
                "proxy {} already exists",
                proxy.name
            )));
        }
        map.insert(
            proxy.name.clone(),
            ManagedProxy {
                spec: proxy,
                running: false,
                bound_addr: None,
                cancel: self.runtime.shutdown_token.child_token(),
                done: Arc::new(SupervisorDone::default()),
                active: Arc::new(AtomicUsize::new(0)),
                ordinal: Arc::new(AtomicU64::new(1)),
            },
        );
        Ok(self.next_generation())
    }

    /// Bind and supervise every enabled definition that is not running.
    pub async fn start_all(&self) -> Result<u64, ControlError> {
        let names: Vec<String> = self
            .runtime
            .proxies
            .read()
            .await
            .iter()
            .filter(|(_, entry)| entry.spec.enabled && !entry.running)
            .map(|(name, _)| name.clone())
            .collect();
        for name in names {
            let spec = self
                .runtime
                .proxies
                .read()
                .await
                .get(&name)
                .map(|entry| entry.spec.clone())
                .ok_or_else(|| ControlError::NotFound(name.clone()))?;
            self.start_stored(&name, &spec).await?;
        }
        Ok(self.generation())
    }

    /// Validate, bind, publish initial policies, and supervise a proxy.
    /// The listener is bound before visible success; a bind failure leaves
    /// no registered proxy. Returns the operator view (including the actual
    /// bound address, resolving port 0) and the new generation.
    pub async fn create_proxy(
        &self,
        mut proxy: ProxySpec,
    ) -> Result<(ProxyView, u64), ControlError> {
        proxy.validate().map_err(|error| match error {
            EggchaosError::InvalidProxy(message) => ControlError::Invalid(message),
            EggchaosError::InvalidPlan(error) => {
                ControlError::Invalid(format!("invalid fault plan: {error}"))
            }
            other => ControlError::Invalid(other.to_string()),
        })?;
        {
            let map = self.runtime.proxies.read().await;
            if map.contains_key(&proxy.name) {
                return Err(ControlError::Conflict(format!(
                    "proxy {} already exists",
                    proxy.name
                )));
            }
        }
        let listener =
            TcpListener::bind(proxy.listen)
                .await
                .map_err(|source| ControlError::BindFailed {
                    proxy: proxy.name.clone(),
                    reason: source.to_string(),
                })?;
        let bound_addr = listener
            .local_addr()
            .map_err(|source| ControlError::BindFailed {
                proxy: proxy.name.clone(),
                reason: source.to_string(),
            })?;
        // Canonical plans and live policies start together at generation
        // one: the stored spec carries the same policy snapshots the
        // streams will compile from.
        proxy.init_policies(self.runtime.params.seed);
        proxy.enabled = true;
        let name = proxy.name.clone();
        let cancel = self.runtime.shutdown_token.child_token();
        let mut entry = ManagedProxy {
            spec: proxy,
            running: true,
            bound_addr: Some(bound_addr),
            cancel: cancel.clone(),
            done: Arc::new(SupervisorDone::default()),
            active: Arc::new(AtomicUsize::new(0)),
            ordinal: Arc::new(AtomicU64::new(1)),
        };
        let done = self
            .spawn_supervisor(listener, name.clone(), cancel.clone())
            .await;
        entry.done = done;
        {
            let mut map = self.runtime.proxies.write().await;
            if map.contains_key(&name) {
                // Lost a creation race after binding: stop the orphan
                // supervisor through its own scope and release the
                // listener rather than leaking an untracked acceptor.
                drop(map);
                cancel.cancel();
                entry.done.join().await;
                self.untrack(&name).await;
                return Err(ControlError::Conflict(format!(
                    "proxy {name} already exists"
                )));
            }
            map.insert(name.clone(), entry);
            let view = RuntimeInner::view_of(map.get(&name).expect("proxy was just inserted"));
            drop(map);
            Ok((view, self.next_generation()))
        }
    }

    /// Drop the finished tracked supervisor handle for a proxy. Only call
    /// after the supervisor's done-flag confirms completion, so a running
    /// task is never detached.
    async fn untrack(&self, name: &str) {
        let mut root = self.runtime.root.lock().await;
        if let Some(position) = root.iter().position(|(tracked, _)| tracked == name) {
            let (_, handle) = root.swap_remove(position);
            drop(handle);
        }
    }

    async fn start_stored(&self, name: &str, spec: &ProxySpec) -> Result<SocketAddr, ControlError> {
        let listener =
            TcpListener::bind(spec.listen)
                .await
                .map_err(|source| ControlError::BindFailed {
                    proxy: name.to_owned(),
                    reason: source.to_string(),
                })?;
        let bound_addr = listener
            .local_addr()
            .map_err(|source| ControlError::BindFailed {
                proxy: name.to_owned(),
                reason: source.to_string(),
            })?;
        let cancel = self.runtime.shutdown_token.child_token();
        let done = self
            .spawn_supervisor(listener, name.to_owned(), cancel.clone())
            .await;
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(name) else {
            // Proxy vanished while binding: stop the orphan supervisor.
            drop(map);
            cancel.cancel();
            done.join().await;
            self.untrack(name).await;
            return Err(ControlError::NotFound(name.to_owned()));
        };
        entry.spec.enabled = true;
        entry.running = true;
        entry.bound_addr = Some(bound_addr);
        entry.cancel = cancel;
        entry.done = done;
        // Stored definitions already carry synchronized plans/policies.
        Ok(bound_addr)
    }

    async fn spawn_supervisor(
        &self,
        listener: TcpListener,
        name: String,
        cancel: CancellationToken,
    ) -> Arc<SupervisorDone> {
        let runtime = self.runtime.clone();
        let done = Arc::new(SupervisorDone::default());
        let task_done = done.clone();
        let task_name = name.clone();
        let handle = tokio::spawn(proxy_supervisor(
            listener, task_name, runtime, cancel, task_done,
        ));
        self.runtime.root.lock().await.push((name, handle));
        done
    }

    /// Stop the listener, terminate its connections, await supervisor exit,
    /// and remove the definition. Success is reported only after the
    /// runtime transition commits.
    pub async fn delete_proxy(&self, name: &str) -> Result<u64, ControlError> {
        let (cancel, done) = {
            let mut map = self.runtime.proxies.write().await;
            let Some(entry) = map.remove(name) else {
                return Err(ControlError::NotFound(name.to_owned()));
            };
            (entry.cancel, entry.done)
        };
        cancel.cancel();
        done.join().await;
        self.untrack(name).await;
        // Connections observe the proxy cancellation through child tokens;
        // the supervisor purges stragglers before its done-flag resolves.
        Ok(self.next_generation())
    }

    /// Enable or disable a proxy. Disabling stops the listener and
    /// terminates active connections while retaining the definition and
    /// fault plans. Enabling rebinds through the same machinery as create.
    pub async fn set_enabled(&self, name: &str, enabled: bool) -> Result<u64, ControlError> {
        if enabled {
            let spec = self
                .runtime
                .proxies
                .read()
                .await
                .get(name)
                .map(|entry| (entry.spec.clone(), entry.running))
                .ok_or_else(|| ControlError::NotFound(name.to_owned()))?;
            if spec.1 {
                return Ok(self.generation());
            }
            self.start_stored(name, &spec.0).await?;
            return Ok(self.next_generation());
        }
        let (cancel, done) = {
            let mut map = self.runtime.proxies.write().await;
            let Some(entry) = map.get_mut(name) else {
                return Err(ControlError::NotFound(name.to_owned()));
            };
            if !entry.running {
                return Ok(self.generation());
            }
            entry.running = false;
            entry.bound_addr = None;
            entry.spec.enabled = false;
            // Fresh scope for the next enable; active connections observe
            // the old scope's cancellation.
            let old =
                std::mem::replace(&mut entry.cancel, self.runtime.shutdown_token.child_token());
            (old, entry.done.clone())
        };
        cancel.cancel();
        done.join().await;
        self.untrack(name).await;
        Ok(self.next_generation())
    }

    /// Update proxy fields. Fault-only changes are out of scope here (use
    /// fault CRUD). `max_connections`/`connect_timeout` apply to new
    /// connections. Listen/upstream changes are restart-class: the
    /// replacement binds before the old listener stops, so a bind failure
    /// keeps the old listener serving and reports `RestartFailed` without
    /// changing the spec.
    pub async fn update_proxy(
        &self,
        name: &str,
        patch: ProxyPatch,
    ) -> Result<(ProxyView, u64), ControlError> {
        if let Some(enabled) = patch.enabled {
            self.set_enabled(name, enabled).await?;
        }
        if let Some(connect_timeout_ms) = patch.connect_timeout_ms {
            if connect_timeout_ms == 0 {
                return Err(ControlError::Invalid(
                    "connect timeout must be non-zero".into(),
                ));
            }
        }
        let current = self
            .runtime
            .proxies
            .read()
            .await
            .get(name)
            .map(|entry| (entry.spec.clone(), entry.running))
            .ok_or_else(|| ControlError::NotFound(name.to_owned()))?;
        let restart = patch
            .listen
            .is_some_and(|listen| listen != current.0.listen)
            || patch
                .upstream
                .is_some_and(|upstream| upstream != current.0.upstream);
        let mut spec = current.0;
        if let Some(listen) = patch.listen {
            spec.listen = listen;
        }
        if let Some(upstream) = patch.upstream {
            spec.upstream = upstream;
        }
        if let Some(max_connections) = patch.max_connections {
            spec.max_connections = max_connections;
        }
        if let Some(connect_timeout_ms) = patch.connect_timeout_ms {
            spec.connect_timeout = Duration::from_millis(connect_timeout_ms);
        }
        spec.validate().map_err(|error| match error {
            EggchaosError::InvalidProxy(message) => ControlError::Invalid(message),
            EggchaosError::InvalidPlan(error) => {
                ControlError::Invalid(format!("invalid fault plan: {error}"))
            }
            other => ControlError::Invalid(other.to_string()),
        })?;
        if !restart || !current.1 {
            // Live fields, or a stopped proxy whose spec simply updates.
            let mut map = self.runtime.proxies.write().await;
            let Some(entry) = map.get_mut(name) else {
                return Err(ControlError::NotFound(name.to_owned()));
            };
            let running = entry.running;
            let bound_addr = entry.bound_addr;
            let cancel = entry.cancel.clone();
            let done = entry.done.clone();
            let active = entry.active.clone();
            let ordinal = entry.ordinal.clone();
            // Preserve live policies: fault plans are untouched by this
            // patch, so policies stay synchronized by construction.
            let mut next = spec.clone();
            next.upstream_policy = entry.spec.upstream_policy.clone();
            next.downstream_policy = entry.spec.downstream_policy.clone();
            *entry = ManagedProxy {
                spec: next,
                running,
                bound_addr,
                cancel,
                done,
                active,
                ordinal,
            };
            let view = RuntimeInner::view_of(entry);
            return Ok((view, self.next_generation()));
        }
        // Restart-class: pre-bind the replacement before touching the old
        // listener, so a bind failure keeps the old listener serving with
        // its bound address and spec untouched.
        let prebound = match TcpListener::bind(spec.listen).await {
            Ok(listener) => listener,
            Err(source) => {
                return Err(ControlError::RestartFailed {
                    proxy: name.to_owned(),
                    reason: format!(
                        "replacement bind failed ({source}); old listener still serving"
                    ),
                });
            }
        };
        let bound_addr = prebound
            .local_addr()
            .map_err(|source| ControlError::RestartFailed {
                proxy: name.to_owned(),
                reason: format!("replacement bound but address unreadable: {source}"),
            })?;
        let (old_cancel, old_done) = {
            let mut map = self.runtime.proxies.write().await;
            let Some(entry) = map.get_mut(name) else {
                return Err(ControlError::NotFound(name.to_owned()));
            };
            entry.running = false;
            entry.bound_addr = None;
            let old_cancel =
                std::mem::replace(&mut entry.cancel, self.runtime.shutdown_token.child_token());
            (old_cancel, entry.done.clone())
        };
        old_cancel.cancel();
        old_done.join().await;
        self.untrack(name).await;
        let cancel = self.runtime.shutdown_token.child_token();
        let done = self
            .spawn_supervisor(prebound, name.to_owned(), cancel.clone())
            .await;
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(name) else {
            // Proxy vanished while restarting: stop the orphan.
            drop(map);
            cancel.cancel();
            done.join().await;
            self.untrack(name).await;
            return Err(ControlError::NotFound(name.to_owned()));
        };
        let mut next = spec.clone();
        next.upstream_policy = entry.spec.upstream_policy.clone();
        next.downstream_policy = entry.spec.downstream_policy.clone();
        entry.spec = next;
        entry.running = true;
        entry.bound_addr = Some(bound_addr);
        entry.cancel = cancel;
        entry.done = done;
        let view = RuntimeInner::view_of(entry);
        Ok((view, self.next_generation()))
    }

    fn build_fault_spec(upsert: &FaultUpsert) -> Result<FaultSpec, ControlError> {
        if upsert.id.is_empty() || upsert.id.len() > 128 {
            return Err(ControlError::Invalid(
                "fault id must be 1..=128 bytes".into(),
            ));
        }
        // Fault identities travel as single URL path segments.
        if !upsert
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(ControlError::Invalid(
                "fault id must match [A-Za-z0-9._-]+".into(),
            ));
        }
        if !(0.0..=1.0).contains(&upsert.probability) || !upsert.probability.is_finite() {
            return Err(ControlError::Invalid(
                "probability must be finite and between 0 and 1".into(),
            ));
        }
        Ok(FaultSpec {
            id: eggchaos_core::FaultId::new(upsert.id.clone())
                .map_err(|error| ControlError::Invalid(error.to_string()))?,
            probability: eggchaos_core::Probability::new(upsert.probability)
                .map_err(|error| ControlError::Invalid(error.to_string()))?,
            kind: upsert.kind,
        })
    }

    /// Add a fault to one direction, updating the canonical plan and the
    /// live policy together under one lock. Fault IDs are unique within a
    /// direction/proxy.
    pub async fn add_fault(
        &self,
        proxy: &str,
        upsert: FaultUpsert,
    ) -> Result<(Direction, FaultSpec, u64), ControlError> {
        let spec = Self::build_fault_spec(&upsert)?;
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(proxy) else {
            return Err(ControlError::NotFound(proxy.to_owned()));
        };
        // The base plan comes from one atomic snapshot load, so a
        // concurrent publication cannot slip between the read and the
        // publish below: the write lock serializes publishers.
        let (base, policy) = match upsert.direction {
            Direction::Upstream => (
                entry.spec.upstream_policy.snapshot(),
                entry.spec.upstream_policy.clone(),
            ),
            Direction::Downstream => (
                entry.spec.downstream_policy.snapshot(),
                entry.spec.downstream_policy.clone(),
            ),
        };
        if base.plan.get(spec.id.as_str()).is_some() {
            return Err(ControlError::Conflict(format!(
                "fault {} already exists on {}/{}",
                spec.id,
                proxy,
                upsert.direction.as_str()
            )));
        }
        let mut next_faults = base.plan.faults().to_vec();
        next_faults.push(spec.clone());
        let next = FaultPlan::new(next_faults)
            .map_err(|error| ControlError::Invalid(error.to_string()))?;
        let published = policy
            .publish(next, base.seed_namespace)
            .map_err(|error| ControlError::Invalid(error.to_string()))?;
        // Canonical mirror refreshes from the published snapshot, never
        // from a locally built plan.
        match upsert.direction {
            Direction::Upstream => entry.spec.upstream_faults = (*published.plan).clone(),
            Direction::Downstream => entry.spec.downstream_faults = (*published.plan).clone(),
        }
        Ok((upsert.direction, spec, self.next_generation()))
    }

    /// Update a fault's probability and/or behavior in place, preserving
    /// order and keeping canonical and live state synchronized.
    pub async fn update_fault(
        &self,
        proxy: &str,
        id: &str,
        patch: FaultPatch,
    ) -> Result<(Direction, FaultSpec, u64), ControlError> {
        if let Some(probability) = patch.probability {
            if !(0.0..=1.0).contains(&probability) || !probability.is_finite() {
                return Err(ControlError::Invalid(
                    "probability must be finite and between 0 and 1".into(),
                ));
            }
        }
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(proxy) else {
            return Err(ControlError::NotFound(proxy.to_owned()));
        };
        for direction in [Direction::Upstream, Direction::Downstream] {
            let policy = match direction {
                Direction::Upstream => entry.spec.upstream_policy.clone(),
                Direction::Downstream => entry.spec.downstream_policy.clone(),
            };
            let base = policy.snapshot();
            let Some(existing) = base.plan.get(id) else {
                continue;
            };
            let mut next_spec = existing.clone();
            if let Some(probability) = patch.probability {
                next_spec.probability = eggchaos_core::Probability::new(probability)
                    .map_err(|error| ControlError::Invalid(error.to_string()))?;
            }
            if let Some(kind) = patch.kind {
                next_spec.kind = kind;
            }
            let mut next_faults = base.plan.faults().to_vec();
            let position = next_faults
                .iter()
                .position(|fault| fault.id.as_str() == id)
                .expect("fault found in base");
            next_faults[position] = next_spec.clone();
            let next = FaultPlan::new(next_faults)
                .map_err(|error| ControlError::Invalid(error.to_string()))?;
            let published = policy
                .publish(next, base.seed_namespace)
                .map_err(|error| ControlError::Invalid(error.to_string()))?;
            match direction {
                Direction::Upstream => entry.spec.upstream_faults = (*published.plan).clone(),
                Direction::Downstream => entry.spec.downstream_faults = (*published.plan).clone(),
            }
            return Ok((direction, next_spec, self.next_generation()));
        }
        Err(ControlError::NotFound(format!("fault {id} on {proxy}")))
    }

    /// Remove a fault from either direction.
    pub async fn remove_fault(&self, proxy: &str, id: &str) -> Result<u64, ControlError> {
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(proxy) else {
            return Err(ControlError::NotFound(proxy.to_owned()));
        };
        for direction in [Direction::Upstream, Direction::Downstream] {
            let policy = match direction {
                Direction::Upstream => entry.spec.upstream_policy.clone(),
                Direction::Downstream => entry.spec.downstream_policy.clone(),
            };
            let base = policy.snapshot();
            if base.plan.get(id).is_none() {
                continue;
            }
            let next = (*base.plan).clone().without_fault(id);
            next.validate()
                .map_err(|error| ControlError::Invalid(error.to_string()))?;
            let published = policy
                .publish(next, base.seed_namespace)
                .map_err(|error| ControlError::Invalid(error.to_string()))?;
            match direction {
                Direction::Upstream => entry.spec.upstream_faults = (*published.plan).clone(),
                Direction::Downstream => entry.spec.downstream_faults = (*published.plan).clone(),
            }
            return Ok(self.next_generation());
        }
        Err(ControlError::NotFound(format!("fault {id} on {proxy}")))
    }

    /// Fetch one fault, searching upstream then downstream. Plans come
    /// from live snapshots, so reads agree with what streams compile.
    pub async fn get_fault(&self, proxy: &str, id: &str) -> Option<(Direction, FaultSpec)> {
        let map = self.runtime.proxies.read().await;
        let entry = map.get(proxy)?;
        if let Some(fault) = entry.spec.upstream_policy.snapshot().plan.get(id) {
            return Some((Direction::Upstream, fault.clone()));
        }
        entry
            .spec
            .downstream_policy
            .snapshot()
            .plan
            .get(id)
            .map(|fault| (Direction::Downstream, fault.clone()))
    }

    /// List both directional fault plans from live snapshots.
    pub async fn list_faults(&self, proxy: &str) -> Option<(Vec<FaultSpec>, Vec<FaultSpec>)> {
        let map = self.runtime.proxies.read().await;
        let entry = map.get(proxy)?;
        Some((
            entry.spec.upstream_policy.snapshot().plan.faults().to_vec(),
            entry
                .spec
                .downstream_policy
                .snapshot()
                .plan
                .faults()
                .to_vec(),
        ))
    }

    /// Publish complete fault-plan generations for an existing proxy,
    /// updating live policies and canonical mirrors together. Seed
    /// namespaces are retained.
    pub async fn publish_plans(
        &self,
        name: &str,
        upstream: FaultPlan,
        downstream: FaultPlan,
    ) -> Result<u64, ControlError> {
        upstream
            .validate()
            .map_err(|error| ControlError::Invalid(error.to_string()))?;
        downstream
            .validate()
            .map_err(|error| ControlError::Invalid(error.to_string()))?;
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(name) else {
            return Err(ControlError::NotFound(name.to_owned()));
        };
        let base_upstream = entry.spec.upstream_policy.snapshot();
        let base_downstream = entry.spec.downstream_policy.snapshot();
        let published_upstream = entry
            .spec
            .upstream_policy
            .publish(upstream, base_upstream.seed_namespace)
            .map_err(|error| ControlError::Invalid(error.to_string()))?;
        let published_downstream = entry
            .spec
            .downstream_policy
            .publish(downstream, base_downstream.seed_namespace)
            .map_err(|error| ControlError::Invalid(error.to_string()))?;
        entry.spec.upstream_faults = (*published_upstream.plan).clone();
        entry.spec.downstream_faults = (*published_downstream.plan).clone();
        Ok(self.next_generation())
    }

    /// Publish plans only when both directions still sit on the expected
    /// generations, deriving no seed change: a concurrent publication
    /// fails fast with `Conflict` instead of being silently overwritten
    /// by a stale base. Scenario events use this path. Returns the global
    /// generation plus the two published policy generations for trails.
    pub async fn publish_plans_expected(
        &self,
        name: &str,
        publish: ExpectedPublish,
    ) -> Result<(u64, u64, u64), ControlError> {
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(name) else {
            return Err(ControlError::NotFound(name.to_owned()));
        };
        let published_upstream = entry
            .spec
            .upstream_policy
            .publish_expected(
                publish.upstream,
                publish.upstream_seed,
                publish.expected_upstream,
            )
            .map_err(|error| match error {
                eggchaos_core::PublishError::Invalid(error) => {
                    ControlError::Invalid(error.to_string())
                }
                eggchaos_core::PublishError::Conflict(conflict) => {
                    ControlError::Conflict(conflict_message(name, conflict))
                }
            })?;
        let published_downstream = entry
            .spec
            .downstream_policy
            .publish_expected(
                publish.downstream,
                publish.downstream_seed,
                publish.expected_downstream,
            )
            .map_err(|error| match error {
                eggchaos_core::PublishError::Invalid(error) => {
                    ControlError::Invalid(error.to_string())
                }
                eggchaos_core::PublishError::Conflict(conflict) => {
                    ControlError::Conflict(conflict_message(name, conflict))
                }
            })?;
        entry.spec.upstream_faults = (*published_upstream.plan).clone();
        entry.spec.downstream_faults = (*published_downstream.plan).clone();
        Ok((
            self.next_generation(),
            published_upstream.generation,
            published_downstream.generation,
        ))
    }

    /// Maximum retained scenario run records.
    pub const MAX_SCENARIO_RUNS: usize = 32;

    /// Publish one direction only when it still sits on the expected
    /// generation. Scenario events use this so an event touches exactly
    /// its target direction: the other direction keeps its plan,
    /// generation, and seed namespace. Returns the global generation
    /// plus the published policy generation.
    pub async fn publish_direction_expected(
        &self,
        name: &str,
        direction: Direction,
        plan: FaultPlan,
        seed_namespace: u64,
        expected: u64,
    ) -> Result<(u64, u64), ControlError> {
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(name) else {
            return Err(ControlError::NotFound(name.to_owned()));
        };
        let policy = match direction {
            Direction::Upstream => entry.spec.upstream_policy.clone(),
            Direction::Downstream => entry.spec.downstream_policy.clone(),
        };
        let published = policy
            .publish_expected(plan, seed_namespace, expected)
            .map_err(|error| match error {
                eggchaos_core::PublishError::Invalid(error) => {
                    ControlError::Invalid(error.to_string())
                }
                eggchaos_core::PublishError::Conflict(conflict) => {
                    ControlError::Conflict(conflict_message(name, conflict))
                }
            })?;
        match direction {
            Direction::Upstream => entry.spec.upstream_faults = (*published.plan).clone(),
            Direction::Downstream => entry.spec.downstream_faults = (*published.plan).clone(),
        }
        Ok((self.next_generation(), published.generation))
    }

    /// Atomically snapshot both directional policies for a proxy: the
    /// plans a scenario event must base its publication on.
    pub async fn snapshot_policies(
        &self,
        name: &str,
    ) -> Option<(
        Arc<eggchaos_core::PublishedPolicy>,
        Arc<eggchaos_core::PublishedPolicy>,
    )> {
        let map = self.runtime.proxies.read().await;
        let entry = map.get(name)?;
        Some((
            entry.spec.upstream_policy.snapshot(),
            entry.spec.downstream_policy.snapshot(),
        ))
    }

    /// Start an owned scenario run. The document validates entirely
    /// upfront; the run task is supervised by the service and cancelled
    /// on shutdown, never detached.
    pub async fn start_scenario(
        &self,
        scenario: crate::scenario::Scenario,
    ) -> Result<crate::scenario::ScenarioRunRecord, EggchaosError> {
        crate::scenario::validate_scenario(self, &scenario).await?;
        let run_id = self.runtime.next_run_id.fetch_add(1, Ordering::AcqRel);
        let record = crate::scenario::ScenarioRunRecord {
            run_id,
            seed: scenario.seed,
            status: crate::scenario::ScenarioRunStatus::Pending,
            applied: 0,
            failure: None,
            trail: Vec::new(),
        };
        {
            let mut runs = self.runtime.scenario_runs.lock().await;
            // Prune oldest finished runs first; active runs always fit
            // until the active cap, which fails fast instead of queuing.
            let active = runs
                .values()
                .filter(|record| {
                    matches!(
                        record.status,
                        crate::scenario::ScenarioRunStatus::Pending
                            | crate::scenario::ScenarioRunStatus::Running
                            | crate::scenario::ScenarioRunStatus::Cancelling
                    )
                })
                .count();
            if active >= Self::MAX_SCENARIO_RUNS {
                return Err(EggchaosError::Control(ControlError::Conflict(
                    "too many active scenario runs".into(),
                )));
            }
            while runs.len() >= Self::MAX_SCENARIO_RUNS {
                let oldest_finished = runs
                    .iter()
                    .find(|(_, record)| {
                        !matches!(
                            record.status,
                            crate::scenario::ScenarioRunStatus::Pending
                                | crate::scenario::ScenarioRunStatus::Running
                                | crate::scenario::ScenarioRunStatus::Cancelling
                        )
                    })
                    .map(|(id, _)| *id);
                let Some(oldest) = oldest_finished else {
                    break;
                };
                runs.remove(&oldest);
            }
            runs.insert(run_id, record.clone());
        }
        let token = self.runtime.shutdown_token.child_token();
        self.runtime
            .scenario_tokens
            .lock()
            .await
            .insert(run_id, token.clone());
        let state = self.clone();
        self.runtime
            .scenario_tasks
            .lock()
            .await
            .spawn(crate::scenario::drive_scenario_run(
                state, run_id, scenario, token,
            ));
        Ok(record)
    }

    /// Fetch one scenario run record.
    pub async fn get_scenario(&self, run_id: u64) -> Option<crate::scenario::ScenarioRunRecord> {
        self.runtime
            .scenario_runs
            .lock()
            .await
            .get(&run_id)
            .cloned()
    }

    /// Cancel an active scenario run, returning its latest record.
    /// Returns `None` for unknown run IDs. Cancelling a finished run
    /// returns its final record unchanged.
    pub async fn cancel_scenario(&self, run_id: u64) -> Option<crate::scenario::ScenarioRunRecord> {
        let token = self.runtime.scenario_tokens.lock().await.remove(&run_id);
        if let Some(token) = token {
            token.cancel();
            self.update_scenario_run(run_id, |record| {
                if matches!(
                    record.status,
                    crate::scenario::ScenarioRunStatus::Pending
                        | crate::scenario::ScenarioRunStatus::Running
                ) {
                    record.status = crate::scenario::ScenarioRunStatus::Cancelling;
                }
            })
            .await;
        }
        self.get_scenario(run_id).await
    }

    /// Apply a record mutation for a scenario run, if still retained.
    pub async fn update_scenario_run(
        &self,
        run_id: u64,
        update: impl FnOnce(&mut crate::scenario::ScenarioRunRecord),
    ) {
        if let Some(record) = self.runtime.scenario_runs.lock().await.get_mut(&run_id) {
            update(record);
        }
    }

    /// Drop a finished run's cancellation token.
    pub async fn remove_scenario_token(&self, run_id: u64) {
        self.runtime.scenario_tokens.lock().await.remove(&run_id);
    }

    /// Reset the service: retain definitions and listen/upstream addresses,
    /// enable every proxy, replace all fault plans with empty plans, and
    /// terminate active connections. Exactly one generation covers the
    /// transaction; proxies whose bind fails stay disabled and are reported.
    pub async fn reset(&self) -> Result<ResetReport, ControlError> {
        let mut failed_enables = Vec::new();
        let to_start: Vec<String> = {
            let mut map = self.runtime.proxies.write().await;
            // Terminate active connections through level-triggered tokens.
            for token in self.runtime.cancellations.read().await.values() {
                token.cancel();
            }
            for entry in map.values_mut() {
                entry.spec.enabled = true;
                entry.spec.upstream_faults = FaultPlan::empty();
                entry.spec.downstream_faults = FaultPlan::empty();
                // Reset clears plans but retains seed namespaces; the next
                // scenario or manual publication sets its own.
                let upstream_ns = entry.spec.upstream_policy.seed_namespace();
                let downstream_ns = entry.spec.downstream_policy.seed_namespace();
                let _ = entry
                    .spec
                    .upstream_policy
                    .publish(FaultPlan::empty(), upstream_ns);
                let _ = entry
                    .spec
                    .downstream_policy
                    .publish(FaultPlan::empty(), downstream_ns);
            }
            map.iter()
                .filter(|(_, entry)| !entry.running)
                .map(|(name, _)| name.clone())
                .collect()
        };
        for name in to_start {
            let spec = self
                .runtime
                .proxies
                .read()
                .await
                .get(&name)
                .map(|entry| entry.spec.clone());
            let Some(spec) = spec else { continue };
            if let Err(error) = self.start_stored(&name, &spec).await {
                tracing::warn!("reset could not enable proxy {name}: {error}");
                failed_enables.push(name);
            }
        }
        Ok(ResetReport {
            generation: self.next_generation(),
            reset: true,
            failed_enables,
        })
    }

    /// Mark service shutdown and cascade cancellation to every proxy and
    /// connection scope. Level-triggered: tasks that begin waiting later
    /// still observe it.
    pub fn initiate_shutdown(&self) {
        self.runtime
            .shutdown_initiated
            .store(true, Ordering::Release);
        self.runtime.shutdown_token.cancel();
    }

    /// Shut down and wait until every supervised listener and connection
    /// task has drained. Owned scenario runs observe shutdown through
    /// their tokens and are joined here, so none outlive the service.
    /// Idempotent.
    pub async fn shutdown_and_join(&self) {
        self.initiate_shutdown();
        let tracked: Vec<(String, JoinHandle<()>)> = {
            let mut root = self.runtime.root.lock().await;
            std::mem::take(&mut *root)
        };
        for (_, handle) in tracked {
            let _ = handle.await;
        }
        let mut scenarios = self.runtime.scenario_tasks.lock().await;
        while scenarios.join_next().await.is_some() {}
    }

    /// Actual bound addresses for running proxies.
    pub async fn bound_addresses(&self) -> HashMap<String, SocketAddr> {
        self.runtime
            .proxies
            .read()
            .await
            .iter()
            .filter_map(|(name, entry)| entry.bound_addr.map(|addr| (name.clone(), addr)))
            .collect()
    }
}

/// Builder for a structured multi-proxy service.
pub struct ServiceBuilder {
    seed: u64,
    proxies: Vec<ProxySpec>,
    limits: AdmissionLimits,
    half_close: HalfClosePolicy,
    relay_buffer: NonZeroUsize,
    term_grace: Duration,
}

impl ServiceBuilder {
    /// Create a builder with an explicit run seed.
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            proxies: Vec::new(),
            limits: AdmissionLimits::default(),
            half_close: HalfClosePolicy::Drain,
            relay_buffer: NonZeroUsize::new(64 * 1024).expect("non-zero buffer"),
            term_grace: Duration::from_secs(5),
        }
    }
    /// Add a fixed-target proxy.
    pub fn proxy(mut self, proxy: ProxySpec) -> Self {
        self.proxies.push(proxy);
        self
    }
    /// Add several fixed-target proxies.
    pub fn proxy_all(mut self, proxies: impl IntoIterator<Item = ProxySpec>) -> Self {
        self.proxies.extend(proxies);
        self
    }
    /// Set admission limits.
    pub fn limits(mut self, limits: AdmissionLimits) -> Self {
        self.limits = limits;
        self
    }
    /// Set relay half-close behavior.
    pub fn half_close(mut self, policy: HalfClosePolicy) -> Self {
        self.half_close = policy;
        self
    }
    /// Set the bounded relay copy buffer.
    pub fn relay_buffer(mut self, bytes: NonZeroUsize) -> Self {
        self.relay_buffer = bytes;
        self
    }
    /// Set the graceful-termination drain grace period.
    pub fn termination_grace(mut self, grace: Duration) -> Self {
        self.term_grace = grace;
        self
    }
    /// Validate and create a startable service.
    pub fn build(self) -> Result<EggchaosService, EggchaosError> {
        let mut names = std::collections::HashSet::new();
        for proxy in &self.proxies {
            proxy.validate()?;
            if !names.insert(proxy.name.clone()) {
                return Err(EggchaosError::DuplicateProxy(proxy.name.clone()));
            }
        }
        if self.limits.global_connections == 0 {
            return Err(EggchaosError::InvalidProxy(
                "global connection limit must be non-zero".into(),
            ));
        }
        Ok(EggchaosService {
            params: RuntimeParams {
                seed: self.seed,
                limits: self.limits,
                half_close: self.half_close,
                relay_buffer: self.relay_buffer,
                term_grace: self.term_grace,
            },
            proxies: self.proxies,
        })
    }
}

/// A configured but not yet started service.
pub struct EggchaosService {
    params: RuntimeParams,
    proxies: Vec<ProxySpec>,
}

impl EggchaosService {
    /// Return a builder.
    pub fn builder(seed: u64) -> ServiceBuilder {
        ServiceBuilder::new(seed)
    }
    /// Start all enabled listeners through the runtime control authority,
    /// returning a handle over actual bound state.
    pub async fn start(self) -> Result<ServiceHandle, EggchaosError> {
        let control = ControlState::with_params(self.params);
        for proxy in self.proxies {
            if proxy.enabled {
                control.create_proxy(proxy).await?;
            } else {
                control.import_definition(proxy).await?;
            }
        }
        Ok(ServiceHandle { control })
    }
}

/// Runtime control handle with structured listener ownership. Every proxy
/// supervisor task is owned by the shared root `JoinSet`; every connection
/// task is owned by its proxy supervisor. No listener or connection task is
/// ever detached.
pub struct ServiceHandle {
    control: ControlState,
}

impl ServiceHandle {
    /// Actual bound addresses for running proxies.
    pub async fn bound_addresses(&self) -> HashMap<String, SocketAddr> {
        self.control.bound_addresses().await
    }
    /// Request service shutdown. The request is level-triggered and
    /// cascades to every proxy and connection scope.
    pub fn shutdown(&self) {
        self.control.initiate_shutdown();
    }
    /// Snapshot active connections.
    pub async fn connections(&self) -> Vec<ConnectionSnapshot> {
        self.control.connections().await
    }
    /// Snapshot bounded closed-connection history.
    pub async fn history(&self) -> Vec<ClosedConnection> {
        self.control.history().await
    }
    /// Return the shared native mutation authority for this runtime.
    pub fn control_state(&self) -> ControlState {
        self.control.clone()
    }
    /// Wait until shutdown cascades and every supervised task drains.
    pub async fn wait(&self) {
        self.control.shutdown_and_join().await;
    }
}

#[derive(Clone)]
struct ProxyConnParams {
    name: String,
    upstream: SocketAddr,
    connect_timeout: Duration,
    upstream_policy: LivePolicy,
    downstream_policy: LivePolicy,
}

async fn proxy_supervisor(
    listener: TcpListener,
    proxy_name: String,
    runtime: Arc<RuntimeInner>,
    cancel: CancellationToken,
    done: Arc<SupervisorDone>,
) {
    // Level-triggered completion even on panic: waiters join the flag.
    let _guard = DoneGuard(done.clone());
    let mut children = JoinSet::new();
    loop {
        // Re-read the entry every accept so updates apply to new
        // connections; exit when the proxy is gone or its scope cancelled.
        let entry = runtime.proxies.read().await.get(&proxy_name).cloned();
        let Some(entry) = entry else { break };
        if cancel.is_cancelled() || runtime.shutdown_token.is_cancelled() {
            break;
        }
        tokio::select! {
            () = cancel.cancelled() => break,
            () = runtime.shutdown_token.cancelled() => break,
            accepted = listener.accept() => {
                let Ok((client, peer)) = accepted else {
                    if cancel.is_cancelled() || runtime.shutdown_token.is_cancelled() {
                        break;
                    }
                    continue;
                };
                accept_connection(&runtime, &entry, client, peer, &mut children).await;
            }
        }
    }
    // Backstop: tokens already told well-behaved connections to exit;
    // abort stragglers, then purge accounting so no record or counter leaks.
    // The done-flag resolves only after the purge, so control operations
    // joining the flag observe settled state.
    children.shutdown().await;
    let outcome = if runtime.shutdown_initiated.load(Ordering::Acquire) {
        ConnectionOutcome::ServiceShutdown
    } else {
        ConnectionOutcome::ProxyRemoved
    };
    purge_proxy_connections(&runtime, &proxy_name, outcome).await;
    done.finish();
}

#[allow(clippy::too_many_lines)]
async fn accept_connection(
    runtime: &Arc<RuntimeInner>,
    entry: &ManagedProxy,
    client: TcpStream,
    peer: SocketAddr,
    children: &mut JoinSet<()>,
) {
    let current = runtime.active.fetch_add(1, Ordering::AcqRel) + 1;
    let current_proxy = entry.active.fetch_add(1, Ordering::AcqRel) + 1;
    let over_global = current > runtime.params.limits.global_connections;
    let over_proxy = entry
        .spec
        .max_connections
        .is_some_and(|limit| current_proxy > limit);
    if over_global || over_proxy {
        runtime.active.fetch_sub(1, Ordering::AcqRel);
        entry.active.fetch_sub(1, Ordering::AcqRel);
        runtime.metrics.rejected.fetch_add(1, Ordering::Relaxed);
        return;
    }
    runtime.metrics.accepted.fetch_add(1, Ordering::Relaxed);
    {
        let mut tables = runtime.metrics.tables.lock().expect("metrics lock");
        tables.record_accept(&entry.spec.name);
    }
    let id = runtime.next_conn_id.fetch_add(1, Ordering::AcqRel);
    let key = entry.ordinal.fetch_add(1, Ordering::AcqRel);
    let generation = runtime.generation.load(Ordering::Acquire);
    // Accept-time policy state comes from one atomic snapshot per
    // direction, so accepted generations and namespaces always describe
    // the plans the new streams will compile.
    let upstream_snapshot = entry.spec.upstream_policy.snapshot();
    let downstream_snapshot = entry.spec.downstream_policy.snapshot();
    let conn_token = entry.cancel.child_token();
    runtime.connections.write().await.insert(
        id,
        ConnectionSnapshot {
            id,
            proxy: entry.spec.name.clone(),
            ordinal: key,
            peer,
            upstream: entry.spec.upstream,
            state: ConnectionState::Connecting,
            connection_key: key,
            seed: runtime.params.seed ^ entry.spec.seed,
            generation,
            accepted_upstream_generation: upstream_snapshot.generation,
            accepted_downstream_generation: downstream_snapshot.generation,
            accepted_upstream_seed: upstream_snapshot.seed_namespace,
            accepted_downstream_seed: downstream_snapshot.seed_namespace,
            observed_upstream_generation: upstream_snapshot.generation,
            observed_downstream_generation: downstream_snapshot.generation,
            pending_upstream_generation: None,
            pending_downstream_generation: None,
            upstream_seed_namespace: upstream_snapshot.seed_namespace,
            downstream_seed_namespace: downstream_snapshot.seed_namespace,
            upstream_transitions: 0,
            downstream_transitions: 0,
            upstream_bytes: DirectionBytes::default(),
            downstream_bytes: DirectionBytes::default(),
            upstream_faults: Vec::new(),
            upstream_faults_truncated: false,
            downstream_faults: Vec::new(),
            downstream_faults_truncated: false,
            rng_version: RngVersion::V1,
        },
    );
    runtime
        .cancellations
        .write()
        .await
        .insert(id, conn_token.clone());
    let params = ProxyConnParams {
        name: entry.spec.name.clone(),
        upstream: entry.spec.upstream,
        connect_timeout: entry.spec.connect_timeout,
        upstream_policy: entry.spec.upstream_policy.clone(),
        downstream_policy: entry.spec.downstream_policy.clone(),
    };
    let runtime_child = runtime.clone();
    let proxy_cancel = entry.cancel.clone();
    let proxy_active = entry.active.clone();
    children.spawn(run_connection(
        client,
        id,
        key,
        params,
        runtime_child,
        conn_token,
        proxy_cancel,
        proxy_active,
        runtime.params.half_close,
        runtime.params.relay_buffer,
        runtime.params.term_grace,
    ));
}

/// Remove exactly one connection record plus its cancellation token and
/// live evidence. Returns the snapshot, token, and evidence when this
/// caller won the removal; counters and history update only on `Some`,
/// so concurrent finish/purge paths cannot double-count or underflow.
async fn take_connection(
    runtime: &RuntimeInner,
    id: u64,
) -> Option<(
    ConnectionSnapshot,
    Option<CancellationToken>,
    Option<ConnectionEvidence>,
)> {
    let snapshot = runtime.connections.write().await.remove(&id);
    let token = runtime.cancellations.write().await.remove(&id);
    let evidence = runtime.evidence.write().await.remove(&id);
    snapshot.map(|snapshot| (snapshot, token, evidence))
}

async fn record_close(
    runtime: &RuntimeInner,
    id: u64,
    proxy_active: &Arc<AtomicUsize>,
    outcome: ConnectionOutcome,
    upstream_termination: Option<TerminationInfo>,
    downstream_termination: Option<TerminationInfo>,
    detail: Option<String>,
) {
    let Some((snapshot, _, evidence)) = take_connection(runtime, id).await else {
        return;
    };
    runtime.active.fetch_sub(1, Ordering::AcqRel);
    proxy_active.fetch_sub(1, Ordering::AcqRel);
    // Final evidence merges before metrics and history so both report the
    // streams' real closing state, including any drain-pending transition.
    let snapshot = merge_evidence(&snapshot, evidence.as_ref());
    aggregate_close_metrics(
        runtime,
        &snapshot,
        evidence.as_ref(),
        &outcome,
        upstream_termination
            .as_ref()
            .or(downstream_termination.as_ref()),
    );
    runtime.metrics.completed.fetch_add(1, Ordering::Relaxed);
    let mut snapshot = snapshot;
    snapshot.state = ConnectionState::Closed;
    if runtime.params.limits.history == 0 {
        return;
    }
    let mut history = runtime.history.write().await;
    while history.len() >= runtime.params.limits.history {
        history.pop_front();
    }
    history.push_back(ClosedConnection {
        snapshot,
        outcome,
        upstream_termination,
        downstream_termination,
        detail,
    });
}

/// Fold one closed connection into reconciling metrics. Totals are
/// derived from final stream evidence, so counters always agree with the
/// history records they summarize.
fn aggregate_close_metrics(
    runtime: &RuntimeInner,
    snapshot: &ConnectionSnapshot,
    evidence: Option<&ConnectionEvidence>,
    outcome: &ConnectionOutcome,
    termination: Option<&TerminationInfo>,
) {
    runtime.metrics.outcomes[outcome_class(outcome)].fetch_add(1, Ordering::Relaxed);
    match termination.map(|info| info.request) {
        Some(TerminationRequest::Graceful) => {
            runtime
                .metrics
                .graceful_requests
                .fetch_add(1, Ordering::Relaxed);
        }
        Some(TerminationRequest::HardReset) => {
            runtime
                .metrics
                .hard_reset_requests
                .fetch_add(1, Ordering::Relaxed);
        }
        None => {}
    }
    if let ConnectionOutcome::HardReset { client, upstream } = outcome {
        for result in [client, upstream] {
            match result {
                ResetResult::Applied => {
                    runtime
                        .metrics
                        .reset_applied
                        .fetch_add(1, Ordering::Relaxed);
                }
                ResetResult::Unsupported(_) => {
                    runtime
                        .metrics
                        .reset_unsupported
                        .fetch_add(1, Ordering::Relaxed);
                }
                ResetResult::Failed(_) => {
                    runtime.metrics.reset_failed.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
    let mut up = [0; 7];
    let mut down = [0; 7];
    if let Some(record) = evidence {
        up = record.upstream.activations();
        down = record.downstream.activations();
        runtime.metrics.transitions.fetch_add(
            record.upstream.transitions() + record.downstream.transitions(),
            Ordering::Relaxed,
        );
    }
    runtime.metrics.bytes_accepted.fetch_add(
        snapshot.upstream_bytes.accepted + snapshot.downstream_bytes.accepted,
        Ordering::Relaxed,
    );
    runtime.metrics.bytes_forwarded.fetch_add(
        snapshot.upstream_bytes.forwarded + snapshot.downstream_bytes.forwarded,
        Ordering::Relaxed,
    );
    runtime.metrics.bytes_discarded.fetch_add(
        snapshot.upstream_bytes.discarded + snapshot.downstream_bytes.discarded,
        Ordering::Relaxed,
    );
    {
        let mut tables = runtime.metrics.tables.lock().expect("metrics lock");
        tables.record_complete(
            &snapshot.proxy,
            [
                snapshot.upstream_bytes.accepted,
                snapshot.upstream_bytes.forwarded,
                snapshot.upstream_bytes.discarded,
            ],
            [
                snapshot.downstream_bytes.accepted,
                snapshot.downstream_bytes.forwarded,
                snapshot.downstream_bytes.discarded,
            ],
        );
        tables.record_activations(&snapshot.proxy, "upstream", up);
        tables.record_activations(&snapshot.proxy, "downstream", down);
    }
}

/// Purge every remaining record for a proxy after its supervisor exits.
/// Aborted tasks never run their finalizer, so the purge owns their
/// accounting exactly once via `take_connection`, including bounded
/// history with the purge outcome.
async fn purge_proxy_connections(
    runtime: &RuntimeInner,
    proxy_name: &str,
    outcome: ConnectionOutcome,
) {
    let ids: Vec<u64> = runtime
        .connections
        .read()
        .await
        .iter()
        .filter(|(_, snapshot)| snapshot.proxy == proxy_name)
        .map(|(id, _)| *id)
        .collect();
    if ids.is_empty() {
        return;
    }
    let proxy_active = runtime
        .proxies
        .read()
        .await
        .get(proxy_name)
        .map(|entry| entry.active.clone());
    for id in ids {
        let Some((snapshot, _, evidence)) = take_connection(runtime, id).await else {
            continue;
        };
        runtime.active.fetch_sub(1, Ordering::AcqRel);
        if let Some(active) = &proxy_active {
            active.fetch_sub(1, Ordering::AcqRel);
        }
        let snapshot = merge_evidence(&snapshot, evidence.as_ref());
        aggregate_close_metrics(runtime, &snapshot, evidence.as_ref(), &outcome, None);
        runtime.metrics.completed.fetch_add(1, Ordering::Relaxed);
        let mut snapshot = snapshot;
        snapshot.state = ConnectionState::Closed;
        if runtime.params.limits.history == 0 {
            continue;
        }
        let mut history = runtime.history.write().await;
        while history.len() >= runtime.params.limits.history {
            history.pop_front();
        }
        history.push_back(ClosedConnection {
            snapshot,
            outcome: outcome.clone(),
            upstream_termination: None,
            downstream_termination: None,
            detail: Some("supervisor purge after task abort".into()),
        });
    }
}

fn is_eggchaos_termination(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::ConnectionAborted
        && error.to_string().starts_with("eggchaos stream terminated")
}

/// Human-readable stale-base conflict detail for scenario fail-fast.
fn conflict_message(proxy: &str, conflict: PolicyConflict) -> String {
    format!(
        "proxy {proxy} policy moved from generation {} to {} during the operation; retry from current state",
        conflict.expected, conflict.found,
    )
}

/// Merge live stream evidence into a snapshot copy. Accept-time fields
/// are preserved; observed/pending generations, seed namespaces,
/// transitions, counters, and active fault lists come from the streams.
fn merge_evidence(
    snapshot: &ConnectionSnapshot,
    evidence: Option<&ConnectionEvidence>,
) -> ConnectionSnapshot {
    let mut merged = snapshot.clone();
    let Some(record) = evidence else {
        return merged;
    };
    for (handle, observed, pending, namespace, transitions, bytes, faults, truncated) in [
        (
            &record.upstream,
            &mut merged.observed_upstream_generation,
            &mut merged.pending_upstream_generation,
            &mut merged.upstream_seed_namespace,
            &mut merged.upstream_transitions,
            &mut merged.upstream_bytes,
            &mut merged.upstream_faults,
            &mut merged.upstream_faults_truncated,
        ),
        (
            &record.downstream,
            &mut merged.observed_downstream_generation,
            &mut merged.pending_downstream_generation,
            &mut merged.downstream_seed_namespace,
            &mut merged.downstream_transitions,
            &mut merged.downstream_bytes,
            &mut merged.downstream_faults,
            &mut merged.downstream_faults_truncated,
        ),
    ] {
        *observed = handle.observed_generation();
        let pending_value = handle.pending_generation();
        *pending = if pending_value == 0 {
            None
        } else {
            Some(pending_value)
        };
        *namespace = handle.seed_namespace();
        *transitions = handle.transitions();
        let (accepted, forwarded, discarded) = handle.byte_counts();
        *bytes = DirectionBytes {
            accepted,
            forwarded,
            discarded,
        };
        let (faults_live, truncated_live) = handle.active_faults();
        *faults = faults_live;
        *truncated = truncated_live;
    }
    merged
}

#[allow(clippy::too_many_arguments)]
async fn run_connection(
    client: TcpStream,
    id: u64,
    key: u64,
    params: ProxyConnParams,
    runtime: Arc<RuntimeInner>,
    conn_token: CancellationToken,
    proxy_cancel: CancellationToken,
    proxy_active: Arc<AtomicUsize>,
    half_close: HalfClosePolicy,
    relay_buffer: NonZeroUsize,
    term_grace: Duration,
) {
    let classify_cancel = || {
        if runtime.shutdown_initiated.load(Ordering::Acquire) {
            ConnectionOutcome::ServiceShutdown
        } else if proxy_cancel.is_cancelled() {
            ConnectionOutcome::ProxyRemoved
        } else {
            ConnectionOutcome::KilledByOperator
        }
    };
    // Upstream dial with durable cancellation: a kill issued before the
    // relay begins waiting is still observed.
    let dial = TcpStream::connect(params.upstream);
    tokio::pin!(dial);
    let upstream = tokio::select! {
        () = conn_token.cancelled() => {
            let outcome = classify_cancel();
            record_close(&runtime, id, &proxy_active, outcome, None, None, Some("cancelled during upstream connect".into())).await;
            return;
        }
        result = timeout(params.connect_timeout, &mut dial) => {
            match result {
                Ok(Ok(stream)) => stream,
                Ok(Err(error)) => {
                    record_close(&runtime, id, &proxy_active, ConnectionOutcome::ConnectFailed(format!("dial: {error}")), None, None, None).await;
                    return;
                }
                Err(_) => {
                    record_close(&runtime, id, &proxy_active, ConnectionOutcome::ConnectFailed(format!("dial timed out after {:?}", params.connect_timeout)), None, None, None).await;
                    return;
                }
            }
        }
    };
    if let Some(snapshot) = runtime.connections.write().await.get_mut(&id) {
        snapshot.state = ConnectionState::Relaying;
    }
    let (client_wrapped, client_reset) = ResettableTcpStream::new(client);
    let (upstream_wrapped, upstream_reset) = ResettableTcpStream::new(upstream);
    // Engines compile from the accepted atomic snapshots, so the seed
    // namespaces and plans always agree with the accepted generation.
    let client_chaos = match ChaosStream::new_live(
        client_wrapped,
        params.downstream_policy.clone(),
        &params.name,
        key,
        Direction::Downstream,
    ) {
        Ok(stream) => stream,
        Err(error) => {
            record_close(
                &runtime,
                id,
                &proxy_active,
                ConnectionOutcome::RelayError(format!("downstream engine: {error}")),
                None,
                None,
                None,
            )
            .await;
            return;
        }
    };
    let upstream_chaos = match ChaosStream::new_live(
        upstream_wrapped,
        params.upstream_policy.clone(),
        &params.name,
        key,
        Direction::Upstream,
    ) {
        Ok(stream) => stream,
        Err(error) => {
            record_close(
                &runtime,
                id,
                &proxy_active,
                ConnectionOutcome::RelayError(format!("upstream engine: {error}")),
                None,
                None,
                None,
            )
            .await;
            return;
        }
    };
    let term_upstream = upstream_chaos.termination_handle();
    let term_downstream = client_chaos.termination_handle();
    // Register live evidence before relaying so snapshots report
    // observed generations, counters, and active faults from the start.
    runtime.evidence.write().await.insert(
        id,
        ConnectionEvidence {
            upstream: upstream_chaos.stream_evidence(),
            downstream: client_chaos.stream_evidence(),
        },
    );
    let options = RelayOptions {
        buffer_size: relay_buffer,
        half_close,
    };
    let mut relay_box = Box::pin(egress_relay::relay_with_options(
        client_chaos,
        upstream_chaos,
        options,
    ));
    let terms = || (term_upstream.get(), term_downstream.get());
    tokio::select! {
        biased;
        () = conn_token.cancelled() => {
            drop(relay_box);
            let outcome = classify_cancel();
            let (up_term, down_term) = terms();
            record_close(&runtime, id, &proxy_active, outcome, up_term, down_term, None).await;
        }
        relay_outcome = &mut relay_box => {
            let (up_term, down_term) = terms();
            match relay_outcome {
                Ok(report) => {
                    record_close(&runtime, id, &proxy_active, ConnectionOutcome::RelayCompleted, up_term, down_term, Some(format!("relay {} (+{}/{})", report.termination, report.bytes_upstream, report.bytes_downstream))).await;
                }
                Err(failure) => {
                    if is_eggchaos_termination(&failure.source) {
                        // The engine terminated the stream after its accepted
                        // prefix resolved; the durable handle names the mode.
                        let requested = up_term.as_ref().or(down_term.as_ref()).map(|info| info.request);
                        match requested {
                            Some(TerminationRequest::HardReset) => {
                                // Relay failed concurrently with a hard
                                // request but the sockets are already gone:
                                // no abortive close could be applied.
                                let unsupported = ResetResult::Unsupported("relay failed as the hard-reset request resolved; sockets already released".into());
                                record_close(&runtime, id, &proxy_active, ConnectionOutcome::HardReset { client: unsupported.clone(), upstream: unsupported }, up_term, down_term, Some(failure.to_string())).await;
                            }
                            _ => {
                                record_close(&runtime, id, &proxy_active, ConnectionOutcome::GracefulTermination { drained: true }, up_term, down_term, Some(failure.to_string())).await;
                            }
                        }
                    } else {
                        record_close(&runtime, id, &proxy_active, ConnectionOutcome::RelayError(failure.to_string()), up_term, down_term, None).await;
                    }
                }
            }
        }
        info = term_upstream.terminated() => {
            handle_termination(&runtime, id, &proxy_active, info, &client_reset, &upstream_reset, relay_box, term_grace, terms()).await;
        }
        info = term_downstream.terminated() => {
            handle_termination(&runtime, id, &proxy_active, info, &client_reset, &upstream_reset, relay_box, term_grace, terms()).await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_termination<F>(
    runtime: &Arc<RuntimeInner>,
    id: u64,
    proxy_active: &Arc<AtomicUsize>,
    info: TerminationInfo,
    client_reset: &Arc<TcpResetHandle>,
    upstream_reset: &Arc<TcpResetHandle>,
    relay_box: std::pin::Pin<Box<F>>,
    term_grace: Duration,
    terms: (Option<TerminationInfo>, Option<TerminationInfo>),
) where
    F: std::future::Future<Output = Result<egress_relay::RelayReport, egress_relay::RelayFailure>>,
{
    match info.request {
        TerminationRequest::Graceful => {
            // Let an active relay drain its accepted prefix; reap an idle
            // relay when the bounded grace expires.
            match timeout(term_grace, relay_box).await {
                Ok(Ok(report)) => {
                    record_close(
                        runtime,
                        id,
                        proxy_active,
                        ConnectionOutcome::GracefulTermination { drained: true },
                        terms.0,
                        terms.1,
                        Some(format!("relay {}", report.termination)),
                    )
                    .await;
                }
                Ok(Err(failure)) => {
                    if is_eggchaos_termination(&failure.source) {
                        record_close(
                            runtime,
                            id,
                            proxy_active,
                            ConnectionOutcome::GracefulTermination { drained: true },
                            terms.0,
                            terms.1,
                            Some(failure.to_string()),
                        )
                        .await;
                    } else {
                        record_close(
                            runtime,
                            id,
                            proxy_active,
                            ConnectionOutcome::RelayError(format!(
                                "graceful termination due, then relay failed: {failure}"
                            )),
                            terms.0,
                            terms.1,
                            None,
                        )
                        .await;
                    }
                }
                Err(_) => {
                    record_close(
                        runtime,
                        id,
                        proxy_active,
                        ConnectionOutcome::GracefulTermination { drained: false },
                        terms.0,
                        terms.1,
                        Some(format!(
                            "grace period of {term_grace:?} expired; relay aborted"
                        )),
                    )
                    .await;
                }
            }
        }
        TerminationRequest::HardReset => {
            // Abortive close: flag both sockets, drop the relay (whose
            // wrappers apply SO_LINGER=0 on release), then read truthful
            // per-socket outcomes.
            client_reset.request_reset();
            upstream_reset.request_reset();
            drop(relay_box);
            let client = client_reset.outcome().unwrap_or(ResetResult::Unsupported(
                "client socket outcome unavailable".into(),
            ));
            let upstream = upstream_reset.outcome().unwrap_or(ResetResult::Unsupported(
                "upstream socket outcome unavailable".into(),
            ));
            record_close(
                runtime,
                id,
                proxy_active,
                ConnectionOutcome::HardReset { client, upstream },
                terms.0,
                terms.1,
                Some(format!(
                    "hard reset requested by {}",
                    info.fault_id.as_deref().unwrap_or("unknown fault")
                )),
            )
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{Scenario, ScenarioAction, ScenarioEvent, ScenarioRunStatus};
    use eggchaos_core::{derive_policy_seed, FaultId, Probability};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn fault(id: &str, kind: FaultKind) -> FaultSpec {
        FaultSpec {
            id: FaultId::new(id).unwrap(),
            probability: Probability::new(1.0).unwrap(),
            kind,
        }
    }

    async fn echo_server() -> (SocketAddr, JoinHandle<()>) {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = origin.local_addr().unwrap();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = origin.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buffer = [0; 4096];
                    loop {
                        match stream.read(&mut buffer).await {
                            Ok(0) => break,
                            Ok(n) => {
                                if stream.write_all(&buffer[..n]).await.is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });
            }
        });
        (addr, task)
    }

    async fn exchange(addr: SocketAddr, message: &[u8]) -> Vec<u8> {
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(message).await.unwrap();
        let mut response = vec![0; message.len()];
        tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        response
    }

    fn test_control() -> ControlState {
        ControlState::with_params(RuntimeParams {
            seed: 7,
            term_grace: Duration::from_millis(100),
            ..RuntimeParams::default()
        })
    }

    #[tokio::test]
    async fn ephemeral_fixed_target_proxy_relays_and_drains() {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin_addr = origin.local_addr().unwrap();
        let origin_task = tokio::spawn(async move {
            let (mut stream, _) = origin.accept().await.unwrap();
            let mut buf = [0; 5];
            stream.read_exact(&mut buf).await.unwrap();
            stream.write_all(&buf).await.unwrap();
        });
        let service = ServiceBuilder::new(1)
            .proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .build()
            .unwrap();
        let handle = service.start().await.unwrap();
        let addr = handle.bound_addresses().await["echo"];
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"hello").await.unwrap();
        let mut response = [0; 5];
        tokio::time::timeout(Duration::from_secs(2), client.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&response, b"hello");
        drop(client);
        origin_task.await.unwrap();
        handle.shutdown();
        handle.wait().await;
    }

    #[tokio::test]
    async fn per_proxy_connection_limit_is_not_global() {
        let first_origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let first_address = first_origin.local_addr().unwrap();
        let first_task = tokio::spawn(async move {
            let (_stream, _) = first_origin.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let second_origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let second_address = second_origin.local_addr().unwrap();
        let second_task = tokio::spawn(async move {
            let (mut stream, _) = second_origin.accept().await.unwrap();
            let mut bytes = [0; 5];
            stream.read_exact(&mut bytes).await.unwrap();
            stream.write_all(&bytes).await.unwrap();
        });
        let service = ServiceBuilder::new(1)
            .proxy(
                ProxySpec::new("first", "127.0.0.1:0".parse().unwrap(), first_address)
                    .with_max_connections(1),
            )
            .proxy(
                ProxySpec::new("second", "127.0.0.1:0".parse().unwrap(), second_address)
                    .with_max_connections(1),
            )
            .build()
            .unwrap();
        let handle = service.start().await.unwrap();
        let first_client = TcpStream::connect(handle.bound_addresses().await["first"])
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let mut second_client = TcpStream::connect(handle.bound_addresses().await["second"])
            .await
            .unwrap();
        second_client.write_all(b"hello").await.unwrap();
        let mut response = [0; 5];
        tokio::time::timeout(
            Duration::from_secs(2),
            second_client.read_exact(&mut response),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(&response, b"hello");
        drop(first_client);
        drop(second_client);
        handle.shutdown();
        handle.wait().await;
        first_task.abort();
        second_task.await.unwrap();
    }

    #[tokio::test]
    async fn create_with_port_zero_reports_actual_bound_address() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let (view, generation) = control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        assert!(generation > 1);
        let bound = view.bound_addr.expect("port 0 resolves");
        assert_ne!(bound.port(), 0);
        assert!(view.running);
        assert_eq!(exchange(bound, b"hello").await, b"hello");
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn create_bind_conflict_leaves_no_ghost_proxy() {
        let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = occupied.local_addr().unwrap();
        let control = test_control();
        let before = control.generation();
        let error = control
            .create_proxy(ProxySpec::new("ghost", addr, addr))
            .await
            .unwrap_err();
        assert!(matches!(error, ControlError::BindFailed { .. }));
        assert!(control.get("ghost").await.is_none());
        assert!(control.list().await.is_empty());
        assert_eq!(control.generation(), before);
        control.shutdown_and_join().await;
    }

    #[tokio::test]
    async fn delete_stops_listener_and_removes_definition() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let (view, _) = control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let bound = view.bound_addr.unwrap();
        assert_eq!(exchange(bound, b"hi").await, b"hi");
        control.delete_proxy("echo").await.unwrap();
        assert!(control.get("echo").await.is_none());
        assert!(TcpStream::connect(bound).await.is_err());
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn disable_enable_lifecycle_retains_definition() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let (view, _) = control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let first_bound = view.bound_addr.unwrap();
        control
            .add_fault(
                "echo",
                FaultUpsert {
                    direction: Direction::Downstream,
                    id: "note".into(),
                    probability: 1.0,
                    kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                        delay: Duration::ZERO,
                    }),
                },
            )
            .await
            .unwrap();
        control.set_enabled("echo", false).await.unwrap();
        let view = control.get("echo").await.unwrap();
        assert!(!view.running);
        assert!(view.bound_addr.is_none());
        assert!(TcpStream::connect(first_bound).await.is_err());
        control.set_enabled("echo", true).await.unwrap();
        let view = control.get("echo").await.unwrap();
        assert!(view.running);
        // Fault plans survive the disable/enable round trip.
        assert!(view.downstream_faults.get("note").is_some());
        assert_eq!(exchange(view.bound_addr.unwrap(), b"yo").await, b"yo");
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn upstream_update_redirects_new_traffic() {
        let (first_addr, first_task) = echo_server().await;
        let second_origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let second_addr = second_origin.local_addr().unwrap();
        let second_task = tokio::spawn(async move {
            let (mut stream, _) = second_origin.accept().await.unwrap();
            let mut buffer = [0; 6];
            stream.read_exact(&mut buffer).await.unwrap();
            stream.write_all(b"SECOND").await.unwrap();
        });
        let control = test_control();
        let (view, _) = control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                first_addr,
            ))
            .await
            .unwrap();
        let bound = view.bound_addr.unwrap();
        assert_eq!(exchange(bound, b"hello").await, b"hello");
        let (view, _) = control
            .update_proxy(
                "echo",
                ProxyPatch {
                    upstream: Some(second_addr),
                    ..ProxyPatch::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(view.upstream, second_addr);
        assert_eq!(
            exchange(view.bound_addr.unwrap(), b"second").await,
            b"SECOND"
        );
        second_task.await.unwrap();
        control.shutdown_and_join().await;
        first_task.abort();
    }

    #[tokio::test]
    async fn failed_restart_keeps_old_listener_running() {
        let (origin_addr, origin_task) = echo_server().await;
        let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let conflict = occupied.local_addr().unwrap();
        let control = test_control();
        let (view, _) = control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let old_bound = view.bound_addr.unwrap();
        let error = control
            .update_proxy(
                "echo",
                ProxyPatch {
                    listen: Some(conflict),
                    ..ProxyPatch::default()
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ControlError::RestartFailed { .. }));
        // Documented consistent state: old listener still serves, spec kept.
        let view = control.get("echo").await.unwrap();
        assert!(view.running);
        assert_eq!(view.bound_addr, Some(old_bound));
        assert_eq!(exchange(old_bound, b"ok").await, b"ok");
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn concurrent_mutations_serialize_into_unique_generations() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let mut handles = Vec::new();
        for index in 0..10 {
            let control = control.clone();
            handles.push(tokio::spawn(async move {
                control
                    .create_proxy(ProxySpec::new(
                        format!("proxy-{index}"),
                        "127.0.0.1:0".parse().unwrap(),
                        origin_addr,
                    ))
                    .await
                    .map(|(_, generation)| generation)
            }));
        }
        let mut generations = Vec::new();
        for handle in handles {
            generations.push(handle.await.unwrap().unwrap());
        }
        generations.sort_unstable();
        generations.dedup();
        assert_eq!(generations.len(), 10);
        // Concurrent fault updates on one proxy also serialize uniquely.
        let mut handles = Vec::new();
        for index in 0..5 {
            let control = control.clone();
            handles.push(tokio::spawn(async move {
                control
                    .add_fault(
                        "proxy-0",
                        FaultUpsert {
                            direction: Direction::Upstream,
                            id: format!("fault-{index}"),
                            probability: 1.0,
                            kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                                delay: Duration::ZERO,
                            }),
                        },
                    )
                    .await
                    .map(|(_, _, generation)| generation)
            }));
        }
        let mut generations = Vec::new();
        for handle in handles {
            generations.push(handle.await.unwrap().unwrap());
        }
        generations.sort_unstable();
        generations.dedup();
        assert_eq!(generations.len(), 5);
        let (upstream, _) = control.list_faults("proxy-0").await.unwrap();
        assert_eq!(upstream.len(), 5);
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn fault_crud_syncs_canonical_and_live_state() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        // Add then add again: the second publish must build on the first,
        // proving GET state and live policy never roll back to stale plans.
        control
            .add_fault(
                "echo",
                FaultUpsert {
                    direction: Direction::Downstream,
                    id: "first".into(),
                    probability: 1.0,
                    kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                        delay: Duration::ZERO,
                    }),
                },
            )
            .await
            .unwrap();
        control
            .add_fault(
                "echo",
                FaultUpsert {
                    direction: Direction::Downstream,
                    id: "second".into(),
                    probability: 0.5,
                    kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                        delay: Duration::from_millis(1),
                    }),
                },
            )
            .await
            .unwrap();
        let view = control.get("echo").await.unwrap();
        assert!(view.downstream_faults.get("first").is_some());
        assert!(view.downstream_faults.get("second").is_some());
        // Update preserves order and identity while changing behavior.
        let (direction, updated, _) = control
            .update_fault(
                "echo",
                "second",
                FaultPatch {
                    probability: Some(1.0),
                    kind: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(direction, Direction::Downstream);
        assert_eq!(updated.probability.get(), 1.0);
        // Get one fault reports its direction truthfully.
        let (direction, _) = control.get_fault("echo", "second").await.unwrap();
        assert_eq!(direction, Direction::Downstream);
        // Remove narrows the plan; unknown identities 404.
        control.remove_fault("echo", "first").await.unwrap();
        let view = control.get("echo").await.unwrap();
        assert!(view.downstream_faults.get("first").is_none());
        assert!(view.downstream_faults.get("second").is_some());
        assert!(control.remove_fault("echo", "missing").await.is_err());
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn latency_fault_engages_live_traffic_without_reconnect() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let (view, _) = control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let bound = view.bound_addr.unwrap();
        let mut client = TcpStream::connect(bound).await.unwrap();
        client.write_all(b"fast").await.unwrap();
        let mut response = [0; 4];
        tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&response, b"fast");
        // Add downstream latency on the live connection: the next exchange
        // must observe it without reconnecting.
        control
            .add_fault(
                "echo",
                FaultUpsert {
                    direction: Direction::Downstream,
                    id: "slow".into(),
                    probability: 1.0,
                    kind: FaultKind::Latency(eggchaos_core::LatencyConfig {
                        delay: Duration::from_millis(150),
                        jitter: Duration::ZERO,
                        max_buffer_bytes: std::num::NonZeroU64::new(64 * 1024).unwrap(),
                    }),
                },
            )
            .await
            .unwrap();
        let start = tokio::time::Instant::now();
        client.write_all(b"slow").await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&response, b"slow");
        assert!(start.elapsed() >= Duration::from_millis(100));
        // Removing the fault restores fast traffic on the same connection.
        control.remove_fault("echo", "slow").await.unwrap();
        let start = tokio::time::Instant::now();
        client.write_all(b"fast").await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert!(start.elapsed() < Duration::from_millis(100));
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn reset_empties_faults_and_enables_proxies() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let (view, _) = control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let bound = view.bound_addr.unwrap();
        control
            .add_fault(
                "echo",
                FaultUpsert {
                    direction: Direction::Upstream,
                    id: "fault".into(),
                    probability: 1.0,
                    kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                        delay: Duration::ZERO,
                    }),
                },
            )
            .await
            .unwrap();
        // A disabled proxy with faults is re-enabled and cleared by reset.
        let (disabled, _) = control
            .create_proxy(ProxySpec::new(
                "spare",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let _ = disabled;
        control.set_enabled("spare", false).await.unwrap();
        let before = control.generation();
        let report = control.reset().await.unwrap();
        assert!(report.reset);
        assert!(report.failed_enables.is_empty());
        assert_eq!(report.generation, before + 1);
        for name in ["echo", "spare"] {
            let view = control.get(name).await.unwrap();
            assert!(view.enabled);
            assert!(view.running);
            assert!(view.upstream_faults.is_empty());
            assert!(view.downstream_faults.is_empty());
        }
        assert_eq!(exchange(bound, b"ok").await, b"ok");
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn kill_during_relay_terminates_and_cleans_up() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let (view, _) = control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let bound = view.bound_addr.unwrap();
        let mut client = TcpStream::connect(bound).await.unwrap();
        client.write_all(b"hold").await.unwrap();
        let id = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(snapshot) = control.connections().await.first().cloned() {
                    break snapshot.id;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(control.kill(id).await);
        // Repeated kills are idempotent and report absence.
        assert!(!control.kill(id).await);
        // The relay ends promptly after the kill.
        let mut buffer = [0; 4];
        tokio::time::timeout(Duration::from_secs(5), client.read(&mut buffer))
            .await
            .unwrap()
            .unwrap();
        // The record is removed exactly once; metrics stay consistent.
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if control.connections().await.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let history = control.history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].outcome, ConnectionOutcome::KilledByOperator);
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn kill_during_upstream_connect_is_not_lost() {
        // Fill a listener backlog without accepting so the upstream dial
        // stays in SYN retry instead of completing or refusing.
        let stall = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let stall_addr = stall.local_addr().unwrap();
        let mut fillers = Vec::new();
        for _ in 0..256 {
            match TcpStream::connect(stall_addr).await {
                Ok(stream) => fillers.push(stream),
                Err(_) => break,
            }
        }
        let pending: Vec<_> = (0..32)
            .map(|_| {
                let addr = stall_addr;
                tokio::spawn(async move { TcpStream::connect(addr).await })
            })
            .collect();
        let control = ControlState::with_params(RuntimeParams {
            seed: 7,
            term_grace: Duration::from_millis(100),
            ..RuntimeParams::default()
        });
        // Long dial timeout so the kill wins the race deterministically.
        let mut spec = ProxySpec::new("stall", "127.0.0.1:0".parse().unwrap(), stall_addr);
        spec.connect_timeout = Duration::from_secs(30);
        let (view, _) = control.create_proxy(spec).await.unwrap();
        let bound = view.bound_addr.unwrap();
        let _client = TcpStream::connect(bound).await.unwrap();
        // Wait until the connection sits in Connecting (dial stalled).
        let id = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let connections = control.connections().await;
                if let Some(snapshot) = connections
                    .iter()
                    .find(|snapshot| matches!(snapshot.state, ConnectionState::Connecting))
                {
                    break snapshot.id;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(control.kill(id).await);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if control.connections().await.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let history = control.history().await;
        assert!(
            history
                .iter()
                .any(|record| record.outcome == ConnectionOutcome::KilledByOperator),
            "kill during connect must be recorded, got {history:?}"
        );
        control.shutdown_and_join().await;
        drop(fillers);
        for task in pending {
            task.abort();
        }
    }

    #[tokio::test]
    async fn shutdown_with_many_connections_leaves_zero_active() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let bound = control.get("echo").await.unwrap().bound_addr.unwrap();
        let mut clients = Vec::new();
        for _ in 0..10 {
            clients.push(TcpStream::connect(bound).await.unwrap());
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if control.connections().await.len() == 10 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        control.shutdown_and_join().await;
        assert!(control.connections().await.is_empty());
        assert_eq!(control.history().await.len(), 10);
        drop(clients);
        origin_task.abort();
    }

    #[tokio::test]
    async fn connection_finish_under_registry_contention_removes_records() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let (view, _) = control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let bound = view.bound_addr.unwrap();
        // Drive one exchange per connection so every connection reaches
        // Relaying before the concurrent close.
        let mut clients = Vec::new();
        for _ in 0..16 {
            let mut client = TcpStream::connect(bound).await.unwrap();
            client.write_all(b"ping").await.unwrap();
            let mut response = [0; 4];
            tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
                .await
                .unwrap()
                .unwrap();
            clients.push(client);
        }
        // Contend the registry while every connection finishes at once:
        // readers and kills race client-side closes on the removal path.
        let probing = tokio::spawn({
            let control = control.clone();
            async move {
                for _ in 0..200 {
                    let connections = control.connections().await;
                    for snapshot in connections {
                        let _ = control.kill(snapshot.id).await;
                    }
                }
            }
        });
        drop(clients);
        probing.await.unwrap();
        // Every finish removes its record exactly once: the active registry
        // drains and history holds one snapshot per connection.
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if control.connections().await.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(control.history().await.len(), 16);
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn closed_history_honors_configured_bound() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = ControlState::with_params(RuntimeParams {
            seed: 7,
            term_grace: Duration::from_millis(50),
            limits: AdmissionLimits {
                global_connections: 64,
                history: 2,
            },
            ..RuntimeParams::default()
        });
        control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let bound = control.get("echo").await.unwrap().bound_addr.unwrap();
        for _ in 0..3 {
            assert_eq!(exchange(bound, b"ping").await, b"ping");
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if control.history().await.len() == 2 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        // The bound holds even after more closures arrive.
        assert_eq!(exchange(bound, b"ping").await, b"ping");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(control.history().await.len() <= 2);
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn graceful_disconnect_terminates_with_evidence() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let plan = eggchaos_core::FaultPlan::new(vec![fault(
            "bye",
            FaultKind::Disconnect(eggchaos_core::DisconnectConfig {
                after: Duration::ZERO,
                hard_reset: false,
            }),
        )])
        .unwrap();
        let mut spec = ProxySpec::new("echo", "127.0.0.1:0".parse().unwrap(), origin_addr);
        spec.downstream_faults = plan;
        let (view, _) = control.create_proxy(spec).await.unwrap();
        let bound = view.bound_addr.unwrap();
        // The exchange completes, then the relay ends on the termination.
        let mut client = TcpStream::connect(bound).await.unwrap();
        client.write_all(b"ping").await.unwrap();
        let mut response = [0; 4];
        let read = tokio::time::timeout(Duration::from_secs(5), client.read(&mut response)).await;
        assert!(read.is_ok());
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if !control.history().await.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let history = control.history().await;
        assert_eq!(
            history[0].outcome,
            ConnectionOutcome::GracefulTermination { drained: true }
        );
        assert_eq!(
            history[0]
                .downstream_termination
                .as_ref()
                .map(|info| info.request),
            Some(TerminationRequest::Graceful)
        );
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn hard_reset_reports_truthful_platform_outcome() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let plan = eggchaos_core::FaultPlan::new(vec![fault(
            "rst",
            FaultKind::Disconnect(eggchaos_core::DisconnectConfig {
                after: Duration::ZERO,
                hard_reset: true,
            }),
        )])
        .unwrap();
        let mut spec = ProxySpec::new("echo", "127.0.0.1:0".parse().unwrap(), origin_addr);
        spec.downstream_faults = plan;
        let (view, _) = control.create_proxy(spec).await.unwrap();
        let bound = view.bound_addr.unwrap();
        let mut client = TcpStream::connect(bound).await.unwrap();
        client.write_all(b"ping").await.unwrap();
        let mut response = [0; 4];
        let _ = tokio::time::timeout(Duration::from_secs(5), client.read(&mut response)).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if !control.history().await.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let history = control.history().await;
        let ConnectionOutcome::HardReset { client, upstream } = &history[0].outcome else {
            panic!("expected HardReset outcome, got {:?}", history[0].outcome);
        };
        // A concrete TCP edge always attempts the reset; only the
        // already-released race reports Unsupported.
        assert!(
            !matches!(client, ResetResult::Unsupported(_)),
            "client: {client:?}"
        );
        assert!(
            !matches!(upstream, ResetResult::Unsupported(_)),
            "upstream: {upstream:?}"
        );
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn publish_get_and_stream_observe_same_generation() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let (view, _) = control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        assert_eq!(view.downstream_generation, 1);
        control
            .add_fault(
                "echo",
                FaultUpsert {
                    direction: Direction::Downstream,
                    id: "note".into(),
                    probability: 1.0,
                    kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                        delay: Duration::ZERO,
                    }),
                },
            )
            .await
            .unwrap();
        // GET reports the published plan with its generation from one
        // atomic snapshot.
        let view = control.get("echo").await.unwrap();
        assert_eq!(view.downstream_generation, 2);
        assert!(view.downstream_faults.get("note").is_some());
        // A stream accepted afterwards observes the same generation and
        // fault identity. The client stays open so the relay (and its
        // record) is alive for the assertions below.
        let mut client = TcpStream::connect(view.bound_addr.unwrap()).await.unwrap();
        client.write_all(b"yo").await.unwrap();
        let mut response = [0; 2];
        tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&response, b"yo");
        let connections = control.connections().await;
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].observed_downstream_generation, 2);
        assert_eq!(connections[0].accepted_downstream_generation, 2);
        assert!(connections[0]
            .downstream_faults
            .iter()
            .any(|fault| fault.id == "note"));
        drop(client);
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn stale_base_publication_conflicts_instead_of_overwriting() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        // Read a base, then let a concurrent publication move the
        // generation before the stale base publishes.
        let (base_upstream, base_downstream) = control.snapshot_policies("echo").await.unwrap();
        control
            .add_fault(
                "echo",
                FaultUpsert {
                    direction: Direction::Downstream,
                    id: "fresh".into(),
                    probability: 1.0,
                    kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                        delay: Duration::ZERO,
                    }),
                },
            )
            .await
            .unwrap();
        let stale = (*base_downstream.plan).clone();
        let error = control
            .publish_plans_expected(
                "echo",
                ExpectedPublish {
                    upstream: (*base_upstream.plan).clone(),
                    downstream: stale,
                    upstream_seed: base_upstream.seed_namespace,
                    downstream_seed: base_downstream.seed_namespace,
                    expected_upstream: base_upstream.generation,
                    expected_downstream: base_downstream.generation,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ControlError::Conflict(_)));
        // The concurrent state survived: no silent rollback.
        let view = control.get("echo").await.unwrap();
        assert!(view.downstream_faults.get("fresh").is_some());
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn scenario_remove_builds_on_current_state() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        for id in ["alpha", "beta"] {
            control
                .add_fault(
                    "echo",
                    FaultUpsert {
                        direction: Direction::Downstream,
                        id: id.into(),
                        probability: 1.0,
                        kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                            delay: Duration::ZERO,
                        }),
                    },
                )
                .await
                .unwrap();
        }
        let record = control
            .start_scenario(Scenario {
                version: 1,
                seed: 11,
                events: vec![ScenarioEvent {
                    at_ms: 0,
                    action: ScenarioAction::RemoveFault {
                        proxy: "echo".into(),
                        direction: Direction::Downstream,
                        id: "alpha".into(),
                    },
                }],
            })
            .await
            .unwrap();
        let terminal = wait_scenario(&control, record.run_id).await;
        assert_eq!(terminal.status, ScenarioRunStatus::Completed);
        assert_eq!(terminal.applied, 1);
        // The removal derived from live B-state: beta survives, alpha is
        // gone, and the trail names the resulting generations.
        let view = control.get("echo").await.unwrap();
        assert!(view.downstream_faults.get("alpha").is_none());
        assert!(view.downstream_faults.get("beta").is_some());
        assert_eq!(terminal.trail.len(), 1);
        assert_eq!(
            terminal.trail[0].downstream_generation,
            view.downstream_generation
        );
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn concurrent_fault_publications_serialize_monotonically() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let mut handles = Vec::new();
        for index in 0..10 {
            let control = control.clone();
            handles.push(tokio::spawn(async move {
                control
                    .add_fault(
                        "echo",
                        FaultUpsert {
                            direction: Direction::Downstream,
                            id: format!("fault-{index}"),
                            probability: 1.0,
                            kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                                delay: Duration::ZERO,
                            }),
                        },
                    )
                    .await
                    .map(|(_, _, generation)| generation)
            }));
        }
        let mut generations = Vec::new();
        for handle in handles {
            generations.push(handle.await.unwrap().unwrap());
        }
        generations.sort();
        // Ten serialized publications yield ten unique monotonic global
        // generations.
        let mut deduped = generations.clone();
        deduped.dedup();
        assert_eq!(deduped.len(), 10);
        assert!(generations.windows(2).all(|pair| pair[0] < pair[1]));
        // Canonical GET and the live snapshot agree on the winner order.
        let view = control.get("echo").await.unwrap();
        assert_eq!(view.downstream_faults.faults().len(), 10);
        assert_eq!(view.downstream_generation, 11);
        let (upstream, downstream) = control.snapshot_policies("echo").await.unwrap();
        assert_eq!(downstream.generation, 11);
        assert_eq!(*downstream.plan, view.downstream_faults);
        assert_eq!(upstream.generation, 1);
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn transition_pending_while_buffer_drains() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let (view, _) = control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let bound = view.bound_addr.unwrap();
        control
            .add_fault(
                "echo",
                FaultUpsert {
                    direction: Direction::Downstream,
                    id: "slow".into(),
                    probability: 1.0,
                    kind: FaultKind::Latency(eggchaos_core::LatencyConfig {
                        delay: Duration::from_millis(500),
                        jitter: Duration::ZERO,
                        max_buffer_bytes: std::num::NonZeroU64::new(64 * 1024).unwrap(),
                    }),
                },
            )
            .await
            .unwrap();
        // Hold one connection open without exchanging: its upstream leg
        // (client to target) carries no fault and completes instantly.
        let mut client = TcpStream::connect(bound).await.unwrap();
        client.write_all(b"ping").await.unwrap();
        // Wait until the echo is accepted into the downstream latency
        // queue (still far from due): publishing before the relay
        // processes the first chunk would leave nothing to drain and no
        // observable pending window.
        let id = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let connections = control.connections().await;
                if let Some(snapshot) = connections.first() {
                    if snapshot.downstream_bytes.accepted == 4 {
                        break snapshot.id;
                    }
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        // Publish an empty plan while the downstream echo is still held
        // in the latency queue, then force the stream to observe it with
        // another write.
        control.remove_fault("echo", "slow").await.unwrap();
        client.write_all(b"ping").await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let connections = control.connections().await;
                if let Some(snapshot) = connections.first() {
                    if snapshot.pending_downstream_generation.is_some() {
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let snapshot = control.get_connection(id).await.unwrap();
        assert_eq!(snapshot.accepted_downstream_generation, 2);
        assert_eq!(snapshot.observed_downstream_generation, 2);
        assert_eq!(snapshot.pending_downstream_generation, Some(3));
        // After the buffer drains the transition completes observably.
        let snapshot = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let snapshot = control.get_connection(id).await.unwrap();
                if snapshot.pending_downstream_generation.is_none()
                    && snapshot.observed_downstream_generation == 3
                {
                    break snapshot;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(snapshot.downstream_transitions, 1);
        drop(client);
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn accepted_and_current_generations_update_on_traffic() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let (view, _) = control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let bound = view.bound_addr.unwrap();
        let mut client = TcpStream::connect(bound).await.unwrap();
        client.write_all(b"fast").await.unwrap();
        let mut response = [0; 4];
        tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        control
            .add_fault(
                "echo",
                FaultUpsert {
                    direction: Direction::Downstream,
                    id: "slow".into(),
                    probability: 1.0,
                    kind: FaultKind::Latency(eggchaos_core::LatencyConfig {
                        delay: Duration::from_millis(50),
                        jitter: Duration::ZERO,
                        max_buffer_bytes: std::num::NonZeroU64::new(64 * 1024).unwrap(),
                    }),
                },
            )
            .await
            .unwrap();
        // Traffic after the publication transitions the live connection.
        client.write_all(b"slow").await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        let snapshot = control.connections().await.pop().unwrap();
        assert_eq!(snapshot.accepted_downstream_generation, 1);
        assert_eq!(snapshot.observed_downstream_generation, 2);
        assert_eq!(snapshot.pending_downstream_generation, None);
        assert_eq!(snapshot.downstream_transitions, 1);
        assert_eq!(snapshot.accepted_upstream_generation, 1);
        assert_eq!(snapshot.observed_upstream_generation, 1);
        drop(client);
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn scenario_seed_drives_published_namespaces() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let first = control
            .start_scenario(Scenario {
                version: 1,
                seed: 21,
                events: vec![ScenarioEvent {
                    at_ms: 0,
                    action: ScenarioAction::SetPlan {
                        proxy: "echo".into(),
                        direction: Direction::Downstream,
                        faults: Vec::new(),
                    },
                }],
            })
            .await
            .unwrap();
        let first = wait_scenario(&control, first.run_id).await;
        assert_eq!(first.status, ScenarioRunStatus::Completed);
        let view = control.get("echo").await.unwrap();
        assert_eq!(
            view.downstream_seed_namespace,
            derive_policy_seed(21, first.run_id, 0)
        );
        // A different seed publishes a different namespace for the same
        // event identity.
        let second = control
            .start_scenario(Scenario {
                version: 1,
                seed: 22,
                events: vec![ScenarioEvent {
                    at_ms: 0,
                    action: ScenarioAction::SetPlan {
                        proxy: "echo".into(),
                        direction: Direction::Downstream,
                        faults: Vec::new(),
                    },
                }],
            })
            .await
            .unwrap();
        let second = wait_scenario(&control, second.run_id).await;
        assert_eq!(second.status, ScenarioRunStatus::Completed);
        let view = control.get("echo").await.unwrap();
        assert_eq!(
            view.downstream_seed_namespace,
            derive_policy_seed(22, second.run_id, 0)
        );
        assert_ne!(
            derive_policy_seed(21, first.run_id, 0),
            derive_policy_seed(22, second.run_id, 0)
        );
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn scenario_namespaces_replay_independent_of_scheduling() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let document = |at_ms: u64| Scenario {
            version: 1,
            seed: 33,
            events: vec![
                ScenarioEvent {
                    at_ms: 0,
                    action: ScenarioAction::SetPlan {
                        proxy: "echo".into(),
                        direction: Direction::Downstream,
                        faults: Vec::new(),
                    },
                },
                ScenarioEvent {
                    at_ms,
                    action: ScenarioAction::SetPlan {
                        proxy: "echo".into(),
                        direction: Direction::Upstream,
                        faults: Vec::new(),
                    },
                },
            ],
        };
        // Same seed and event identities under different timing produce
        // the same derived namespaces.
        let first = control.start_scenario(document(50)).await.unwrap();
        let first = wait_scenario(&control, first.run_id).await;
        let second = control.start_scenario(document(5)).await.unwrap();
        let second = wait_scenario(&control, second.run_id).await;
        assert_eq!(first.status, ScenarioRunStatus::Completed);
        assert_eq!(second.status, ScenarioRunStatus::Completed);
        for (record, at_ms) in [(&first, 50), (&second, 5)] {
            assert_eq!(record.trail.len(), 2);
            assert_eq!(record.trail[0].at_ms, 0);
            assert_eq!(record.trail[1].at_ms, at_ms);
        }
        let view = control.get("echo").await.unwrap();
        assert_eq!(
            view.downstream_seed_namespace,
            derive_policy_seed(33, second.run_id, 0)
        );
        assert_eq!(
            view.upstream_seed_namespace,
            derive_policy_seed(33, second.run_id, 1)
        );
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn scenario_cancel_during_sleep_returns_boundedly() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let record = control
            .start_scenario(Scenario {
                version: 1,
                seed: 44,
                events: vec![ScenarioEvent {
                    at_ms: 30_000,
                    action: ScenarioAction::SetPlan {
                        proxy: "echo".into(),
                        direction: Direction::Downstream,
                        faults: Vec::new(),
                    },
                }],
            })
            .await
            .unwrap();
        let start = tokio::time::Instant::now();
        let cancelled = control.cancel_scenario(record.run_id).await.unwrap();
        assert!(matches!(
            cancelled.status,
            ScenarioRunStatus::Cancelling
                | ScenarioRunStatus::Cancelled
                | ScenarioRunStatus::Running
                | ScenarioRunStatus::Pending
        ));
        let terminal = wait_scenario(&control, record.run_id).await;
        assert_eq!(terminal.status, ScenarioRunStatus::Cancelled);
        assert_eq!(terminal.applied, 0);
        assert!(start.elapsed() < Duration::from_secs(10));
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn service_shutdown_cancels_active_scenarios() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let record = control
            .start_scenario(Scenario {
                version: 1,
                seed: 55,
                events: vec![ScenarioEvent {
                    at_ms: 30_000,
                    action: ScenarioAction::SetPlan {
                        proxy: "echo".into(),
                        direction: Direction::Downstream,
                        faults: Vec::new(),
                    },
                }],
            })
            .await
            .unwrap();
        control.shutdown_and_join().await;
        let terminal = control.get_scenario(record.run_id).await.unwrap();
        assert_eq!(terminal.status, ScenarioRunStatus::Cancelled);
        origin_task.abort();
    }

    #[tokio::test]
    async fn scenario_failure_is_observable_not_discarded() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        // The event fires after the proxy is deleted, so validation
        // passes but application fails fast with an observable record.
        let record = control
            .start_scenario(Scenario {
                version: 1,
                seed: 66,
                events: vec![ScenarioEvent {
                    at_ms: 200,
                    action: ScenarioAction::SetPlan {
                        proxy: "echo".into(),
                        direction: Direction::Downstream,
                        faults: Vec::new(),
                    },
                }],
            })
            .await
            .unwrap();
        control.delete_proxy("echo").await.unwrap();
        let terminal = wait_scenario(&control, record.run_id).await;
        assert_eq!(terminal.status, ScenarioRunStatus::Failed);
        assert!(terminal.failure.as_deref().unwrap().contains("echo"));
        assert_eq!(terminal.applied, 0);
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn history_bound_zero_disables_retention_but_keeps_metrics() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = ControlState::with_params(RuntimeParams {
            seed: 7,
            term_grace: Duration::from_millis(100),
            limits: AdmissionLimits {
                global_connections: 64,
                history: 0,
            },
            ..RuntimeParams::default()
        });
        control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let bound = control.get("echo").await.unwrap().bound_addr.unwrap();
        assert_eq!(exchange(bound, b"ping").await, b"ping");
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if control.connections().await.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(control.history().await.is_empty());
        let metrics = control.metrics_text().await;
        assert!(metrics.contains("eggchaos_connections_completed_total 1\n"));
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn metrics_reconcile_with_deterministic_fixture() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let (view, _) = control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        let bound = view.bound_addr.unwrap();
        // A zero-delay graceful disconnect publishes exactly one
        // activation per connection however the bytes fragment, keeping
        // the whole fixture deterministic.
        control
            .add_fault(
                "echo",
                FaultUpsert {
                    direction: Direction::Downstream,
                    id: "bye".into(),
                    probability: 1.0,
                    kind: FaultKind::Disconnect(eggchaos_core::DisconnectConfig {
                        after: Duration::ZERO,
                        hard_reset: false,
                    }),
                },
            )
            .await
            .unwrap();
        // Three graceful echoes plus one operator kill.
        for _ in 0..3 {
            assert_eq!(exchange(bound, b"hello").await, b"hello");
        }
        // Wait until all three echoes fully closed: killing while a
        // naturally-closing connection still holds its record would race
        // the token against an already-decided close.
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if control.history().await.len() == 3 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let held = TcpStream::connect(bound).await.unwrap();
        // Identify the held connection by peer address: `connect`
        // returning does not mean the proxy registered it yet, so a bare
        // length wait could grab a lingering echo record instead.
        let held_peer = held.local_addr().unwrap();
        let id = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let connections = control.connections().await;
                if let Some(snapshot) = connections
                    .iter()
                    .find(|snapshot| snapshot.peer == held_peer)
                {
                    break snapshot.id;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(control.kill(id).await);
        drop(held);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if control.connections().await.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let metrics = control.metrics_text().await;
        // Exact global reconciliation: 4 accepted, 4 completed, 3
        // graceful drains, 1 kill, 30 bytes each way per echo leg.
        assert!(metrics.contains("eggchaos_connections_accepted_total 4\n"));
        assert!(metrics.contains("eggchaos_connections_completed_total 4\n"));
        assert!(metrics
            .contains("eggchaos_connection_outcomes_total{outcome=\"graceful_termination\"} 3\n"));
        assert!(metrics
            .contains("eggchaos_connection_outcomes_total{outcome=\"killed_by_operator\"} 1\n"));
        assert!(metrics.contains("eggchaos_termination_requests_total{request=\"graceful\"} 3\n"));
        assert!(metrics.contains("eggchaos_bytes_total{flow=\"accepted\"} 30\n"));
        assert!(metrics.contains("eggchaos_bytes_total{flow=\"forwarded\"} 30\n"));
        assert!(metrics.contains("eggchaos_bytes_total{flow=\"discarded\"} 0\n"));
        assert!(metrics.contains("eggchaos_proxy_connections_accepted_total{proxy=\"echo\"} 4\n"));
        assert!(metrics.contains("eggchaos_proxy_connections_completed_total{proxy=\"echo\"} 4\n"));
        // The disconnect fault activated exactly once per echo connection.
        assert!(metrics.contains(
            "eggchaos_fault_activations_total{proxy=\"echo\",direction=\"downstream\",fault_type=\"disconnect\"} 3\n"
        ));
        // History agrees with the completed counter.
        assert_eq!(control.history().await.len(), 4);
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn metrics_use_only_bounded_label_keys() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let (view, _) = control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        assert_eq!(exchange(view.bound_addr.unwrap(), b"ping").await, b"ping");
        let metrics = control.metrics_text().await;
        for line in metrics.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if let Some(labels) = line
                .split('{')
                .nth(1)
                .and_then(|rest| rest.split('}').next())
            {
                for pair in labels.split(',') {
                    let key = pair.split('=').next().unwrap_or_default();
                    assert!(
                        [
                            "proxy",
                            "direction",
                            "flow",
                            "outcome",
                            "request",
                            "result",
                            "fault_type"
                        ]
                        .contains(&key),
                        "unexpected metric label {key:?} in {line:?}"
                    );
                }
            }
        }
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    #[tokio::test]
    async fn connection_evidence_contains_no_payload_bytes() {
        let (origin_addr, origin_task) = echo_server().await;
        let control = test_control();
        let (view, _) = control
            .create_proxy(ProxySpec::new(
                "echo",
                "127.0.0.1:0".parse().unwrap(),
                origin_addr,
            ))
            .await
            .unwrap();
        assert_eq!(exchange(view.bound_addr.unwrap(), b"PxQ9z").await, b"PxQ9z");
        let live = serde_json::to_string(&control.connections().await).unwrap();
        assert!(!live.contains("PxQ9z"));
        assert!(!live.contains("payload"));
        let closed = serde_json::to_string(&control.history().await).unwrap();
        assert!(!closed.contains("PxQ9z"));
        assert!(!closed.contains("payload"));
        control.shutdown_and_join().await;
        origin_task.abort();
    }

    async fn wait_scenario(
        control: &ControlState,
        run_id: u64,
    ) -> crate::scenario::ScenarioRunRecord {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let record = control.get_scenario(run_id).await.unwrap();
                if matches!(
                    record.status,
                    ScenarioRunStatus::Completed
                        | ScenarioRunStatus::Cancelled
                        | ScenarioRunStatus::Failed
                ) {
                    break record;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap()
    }
}
