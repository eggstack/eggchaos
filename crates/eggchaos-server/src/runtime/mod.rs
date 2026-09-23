use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    io,
    net::SocketAddr,
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use eggchaos_core::{
    ActiveFault, ChaosStream, Direction, FaultKind, FaultPlan, FaultSpec, LivePolicy,
    PolicyConflict, RngVersion, StreamEvidence, TerminationInfo, TerminationRequest,
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

mod connection;
mod control;
mod metrics;
mod model;
mod supervisor;
mod transport;
use connection::*;
pub use metrics::{
    MetricTables, MetricsCounters, PerProxyMetrics, MAX_METRIC_ACTIVATIONS, MAX_METRIC_PROXIES,
    OUTCOME_CLASS_NAMES,
};
pub use model::*;
use supervisor::*;
pub use transport::{ResetResult, ResettableTcpStream, TcpResetHandle};

/// Server crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_CONNECTION_LIMIT: usize = 1_000_000;
const MAX_HISTORY_LIMIT: usize = 1_000_000;
const MAX_RELAY_BUFFER_BYTES: usize = 16 * 1024 * 1024;
const MAX_CONTROL_TIMEOUT_MS: u128 = 300_000;

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
            connect_timeout: Duration::from_millis(crate::native::NATIVE_DEFAULT_PROXY_TIMEOUT_MS),
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
        if self.connect_timeout.as_millis() > MAX_CONTROL_TIMEOUT_MS {
            return Err(EggchaosError::InvalidProxy(
                "connect timeout must be at most 300000 ms".into(),
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
            relay_buffer: NonZeroUsize::new(crate::native::NATIVE_DEFAULT_BUFFER_BYTES as usize)
                .expect("non-zero buffer"),
            term_grace: Duration::from_millis(crate::native::NATIVE_DEFAULT_PROXY_TIMEOUT_MS),
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
            relay_buffer: NonZeroUsize::new(crate::native::NATIVE_DEFAULT_BUFFER_BYTES as usize)
                .expect("non-zero buffer"),
            term_grace: Duration::from_millis(crate::native::NATIVE_DEFAULT_PROXY_TIMEOUT_MS),
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
        if self.limits.global_connections == 0
            || self.limits.global_connections > MAX_CONNECTION_LIMIT
            || self.limits.history > MAX_HISTORY_LIMIT
        {
            return Err(EggchaosError::InvalidProxy(
                "runtime connection/history limit is outside the supported range".into(),
            ));
        }
        if self.relay_buffer.get() > MAX_RELAY_BUFFER_BYTES
            || self.term_grace.as_millis() > MAX_CONTROL_TIMEOUT_MS
        {
            return Err(EggchaosError::InvalidProxy(
                "runtime buffer/grace setting is outside the supported range".into(),
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

#[cfg(test)]
mod tests;
