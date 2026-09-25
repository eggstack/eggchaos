use super::*;

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
    /// Upstream stream-loss chunks evaluated (ADR 007 additive counter).
    #[serde(default)]
    pub upstream_stream_loss_chunks_evaluated: u64,
    /// Downstream stream-loss chunks evaluated.
    #[serde(default)]
    pub downstream_stream_loss_chunks_evaluated: u64,
    /// Upstream stream-loss chunks dropped.
    #[serde(default)]
    pub upstream_stream_loss_chunks_dropped: u64,
    /// Downstream stream-loss chunks dropped.
    #[serde(default)]
    pub downstream_stream_loss_chunks_dropped: u64,
    /// Upstream stream-loss bytes discarded (counted once; included in
    /// `upstream_bytes.discarded`).
    #[serde(default)]
    pub upstream_stream_loss_bytes_discarded: u64,
    /// Downstream stream-loss bytes discarded.
    #[serde(default)]
    pub downstream_stream_loss_bytes_discarded: u64,
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

/// Datagram fault update body: only supplied fields change. The fault
/// identity and direction are fixed; moving a fault across directions is
/// delete plus add.
#[derive(Debug, Clone, Default)]
pub struct DatagramFaultPatch {
    /// Replacement activation probability.
    pub probability: Option<f64>,
    /// Replacement fault behavior.
    pub kind: Option<eggchaos_core::DatagramFaultKind>,
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
    /// Datagram proxies that could not be re-enabled because their bind failed.
    #[serde(default)]
    pub failed_datagram_enables: Vec<String>,
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
