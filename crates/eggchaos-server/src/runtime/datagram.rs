//! Fixed-target UDP runtime. This is deliberately a sibling to the TCP
//! connection supervisor: UDP associations own message boundaries and one
//! connected upstream socket each.
use std::{
    collections::{HashMap, VecDeque},
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, RwLock as StdRwLock,
    },
    time::Duration,
};

use bytes::Bytes;
use eggchaos_core::{
    DatagramDirectionEngine, DatagramEvidence, DatagramLivePolicy, DatagramPlan,
    DatagramQueueLimits, DatagramScheduled, Direction, RngVersion,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    net::UdpSocket,
    sync::{mpsc, Mutex as AsyncMutex},
    task::JoinHandle,
    time::{interval, Instant, MissedTickBehavior},
};
use tokio_util::sync::CancellationToken;

const UDP_RECEIVE_BUFFER_BYTES: usize = 65_536;
const MAX_DATAGRAM_ASSOCIATIONS: usize = 65_536;
const MAX_DATAGRAM_PROXIES: usize = 1024;
const MAX_ASSOCIATION_HISTORY: usize = 65_536;
const MAX_INGRESS_QUEUE: usize = 1024;
const MAX_INGRESS_BUFFER_BYTES: usize = 1_073_741_824;
const MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(86_400);

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

struct RuntimeInner {
    limits: DatagramRuntimeLimits,
    proxies: AsyncMutex<HashMap<String, ManagedProxy>>,
    active_associations: Arc<AtomicUsize>,
    next_association: Arc<AtomicU64>,
    ingress_buffered_bytes: Arc<AtomicUsize>,
    history: Arc<Mutex<VecDeque<DatagramAssociationSnapshot>>>,
}

struct ManagedProxy {
    state: Arc<ProxyState>,
    listener: Option<ListenerOwner>,
}

struct ListenerOwner {
    bound: SocketAddr,
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

struct ProxyState {
    spec: StdRwLock<DatagramProxySpec>,
    policy_mutation: AsyncMutex<()>,
    associations: AsyncMutex<HashMap<SocketAddr, Arc<Association>>>,
    global_active: Arc<AtomicUsize>,
    next_id: Arc<AtomicU64>,
    ingress_buffered_bytes: Arc<AtomicUsize>,
    counters: Arc<ProxyCounters>,
    limits: DatagramRuntimeLimits,
    history: Arc<Mutex<VecDeque<DatagramAssociationSnapshot>>>,
}

#[derive(Default)]
struct ProxyCounters {
    oversize_datagrams: AtomicU64,
    capacity_rejections: AtomicU64,
    ingress_overflow: AtomicU64,
    association_setup_failures: AtomicU64,
}

struct Association {
    id: u64,
    client: SocketAddr,
    sender: mpsc::Sender<Ingress>,
    pending_ingress: AtomicUsize,
    administrative_cancel: AtomicUsize,
    cancel: CancellationToken,
    worker: AsyncMutex<Option<JoinHandle<()>>>,
    record: Arc<Mutex<AssociationRecord>>,
}

struct Ingress {
    payload: Bytes,
    policy: Arc<eggchaos_core::PublishedDatagramPolicy>,
    _reservation: IngressReservation,
}

struct IngressReservation {
    counter: Arc<AtomicUsize>,
    bytes: usize,
}

impl Drop for IngressReservation {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

struct AssociationRecord {
    id: u64,
    proxy: String,
    client: SocketAddr,
    upstream: SocketAddr,
    created: Instant,
    last_activity: Instant,
    ingress_datagrams: u64,
    ingress_bytes: u64,
    egress_datagrams: u64,
    egress_bytes: u64,
    ingress_queue_overflow: u64,
    oversize_datagrams: u64,
    administrative_discards: u64,
    send_errors: u64,
    upstream_evidence: DatagramEvidence,
    downstream_evidence: DatagramEvidence,
}

struct AssociationSendContext<'a> {
    upstream_socket: &'a UdpSocket,
    listener_socket: &'a UdpSocket,
    client: SocketAddr,
    record: &'a Arc<Mutex<AssociationRecord>>,
    cancel: &'a CancellationToken,
}

impl AssociationRecord {
    fn snapshot(&self, now: Instant) -> DatagramAssociationSnapshot {
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

/// Independent fixed-target UDP listener and association runtime.
#[derive(Clone)]
pub struct DatagramRuntime {
    inner: Arc<RuntimeInner>,
}

impl DatagramRuntime {
    /// Create a bounded runtime without listeners.
    pub fn new(limits: DatagramRuntimeLimits) -> Result<Self, DatagramRuntimeError> {
        let limits = limits.validate()?;
        Ok(Self {
            inner: Arc::new(RuntimeInner {
                limits,
                proxies: AsyncMutex::new(HashMap::new()),
                active_associations: Arc::new(AtomicUsize::new(0)),
                next_association: Arc::new(AtomicU64::new(1)),
                ingress_buffered_bytes: Arc::new(AtomicUsize::new(0)),
                history: Arc::new(Mutex::new(VecDeque::new())),
            }),
        })
    }

    /// Bind and start one fixed-target proxy before reporting success.
    pub async fn create_proxy(
        &self,
        spec: DatagramProxySpec,
    ) -> Result<DatagramProxyView, DatagramRuntimeError> {
        spec.validate(self.inner.limits.max_associations)?;
        {
            let proxies = self.inner.proxies.lock().await;
            if proxies.contains_key(&spec.name) {
                return Err(DatagramRuntimeError::Duplicate(spec.name));
            }
            if proxies.len() >= self.inner.limits.max_proxies {
                return Err(DatagramRuntimeError::Invalid(
                    "proxy definition limit reached".into(),
                ));
            }
        }
        let socket = Arc::new(
            UdpSocket::bind(spec.listen)
                .await
                .map_err(DatagramRuntimeError::Bind)?,
        );
        let bound = socket.local_addr().map_err(DatagramRuntimeError::Bind)?;
        let state = Arc::new(ProxyState {
            spec: StdRwLock::new(spec.clone()),
            policy_mutation: AsyncMutex::new(()),
            associations: AsyncMutex::new(HashMap::new()),
            global_active: self.inner.active_associations.clone(),
            next_id: self.inner.next_association.clone(),
            ingress_buffered_bytes: self.inner.ingress_buffered_bytes.clone(),
            counters: Arc::new(ProxyCounters::default()),
            limits: self.inner.limits,
            history: self.inner.history.clone(),
        });
        let owner = spawn_listener(socket, state.clone());
        let mut proxies = self.inner.proxies.lock().await;
        if proxies.contains_key(&spec.name) {
            owner.cancel.cancel();
            let _ = owner.task.await;
            return Err(DatagramRuntimeError::Duplicate(spec.name));
        }
        if proxies.len() >= self.inner.limits.max_proxies {
            owner.cancel.cancel();
            let _ = owner.task.await;
            return Err(DatagramRuntimeError::Invalid(
                "proxy definition limit reached".into(),
            ));
        }
        proxies.insert(
            spec.name.clone(),
            ManagedProxy {
                state: state.clone(),
                listener: Some(owner),
            },
        );
        Ok(proxy_view(&state, Some(bound), true).await)
    }

    /// Stop a listener and its associations while retaining the definition.
    pub async fn disable_proxy(&self, name: &str) -> Result<(), DatagramRuntimeError> {
        let owner = {
            let mut proxies = self.inner.proxies.lock().await;
            let managed = proxies
                .get_mut(name)
                .ok_or_else(|| DatagramRuntimeError::NotFound(name.into()))?;
            managed.listener.take()
        };
        if let Some(owner) = owner {
            owner.cancel.cancel();
            let _ = owner.task.await;
        }
        Ok(())
    }

    /// Restart a disabled proxy listener, binding before changing visible state.
    pub async fn enable_proxy(
        &self,
        name: &str,
    ) -> Result<DatagramProxyView, DatagramRuntimeError> {
        let (state, listen) = {
            let proxies = self.inner.proxies.lock().await;
            let managed = proxies
                .get(name)
                .ok_or_else(|| DatagramRuntimeError::NotFound(name.into()))?;
            if managed.listener.is_some() {
                return Ok(proxy_view(
                    &managed.state,
                    managed.listener.as_ref().map(|x| x.bound),
                    true,
                )
                .await);
            }
            let listen = managed
                .state
                .spec
                .read()
                .expect("datagram proxy spec")
                .listen;
            (managed.state.clone(), listen)
        };
        let socket = Arc::new(
            UdpSocket::bind(listen)
                .await
                .map_err(DatagramRuntimeError::Bind)?,
        );
        let bound = socket.local_addr().map_err(DatagramRuntimeError::Bind)?;
        let owner = spawn_listener(socket, state.clone());
        let mut proxies = self.inner.proxies.lock().await;
        let managed = proxies
            .get_mut(name)
            .ok_or_else(|| DatagramRuntimeError::NotFound(name.into()))?;
        if managed.listener.is_some() {
            owner.cancel.cancel();
            let _ = owner.task.await;
            return Ok(proxy_view(
                &managed.state,
                managed.listener.as_ref().map(|x| x.bound),
                true,
            )
            .await);
        }
        managed.listener = Some(owner);
        Ok(proxy_view(&state, Some(bound), true).await)
    }

    /// Remove the definition, stop the listener, and join every association task.
    pub async fn delete_proxy(&self, name: &str) -> Result<bool, DatagramRuntimeError> {
        let Some(mut managed) = self.inner.proxies.lock().await.remove(name) else {
            return Ok(false);
        };
        if let Some(owner) = managed.listener.take() {
            owner.cancel.cancel();
            let _ = owner.task.await;
        }
        Ok(true)
    }

    /// Current proxy definitions and actual listener state.
    pub async fn proxies(&self) -> Vec<DatagramProxyView> {
        let proxies = self.inner.proxies.lock().await;
        let mut views = Vec::with_capacity(proxies.len());
        for managed in proxies.values() {
            views.push(
                proxy_view(
                    &managed.state,
                    managed.listener.as_ref().map(|x| x.bound),
                    managed.listener.is_some(),
                )
                .await,
            );
        }
        views.sort_by(|a, b| a.name.cmp(&b.name));
        views
    }

    /// Snapshot one named proxy.
    pub async fn proxy(&self, name: &str) -> Option<DatagramProxyView> {
        let proxies = self.inner.proxies.lock().await;
        let managed = proxies.get(name)?;
        Some(
            proxy_view(
                &managed.state,
                managed.listener.as_ref().map(|owner| owner.bound),
                managed.listener.is_some(),
            )
            .await,
        )
    }

    /// Update listener/target/lifecycle settings. Address changes bind a
    /// replacement listener before stopping the current one; target changes
    /// retain the listener but cancel existing associations so only future
    /// associations use the new fixed target.
    pub async fn update_proxy(
        &self,
        name: &str,
        listen: Option<SocketAddr>,
        upstream: Option<SocketAddr>,
        max_associations: Option<usize>,
        association_idle_timeout: Option<Duration>,
        enabled: Option<bool>,
    ) -> Result<DatagramProxyView, DatagramRuntimeError> {
        let (state, running, mut next) = {
            let proxies = self.inner.proxies.lock().await;
            let managed = proxies
                .get(name)
                .ok_or_else(|| DatagramRuntimeError::NotFound(name.into()))?;
            let next = managed
                .state
                .spec
                .read()
                .expect("datagram proxy spec")
                .clone();
            (managed.state.clone(), managed.listener.is_some(), next)
        };
        let old = state.spec.read().expect("datagram proxy spec").clone();
        if let Some(listen) = listen {
            next.listen = listen;
        }
        if let Some(upstream) = upstream {
            next.upstream = upstream;
        }
        if let Some(max_associations) = max_associations {
            next.max_associations = max_associations;
        }
        if let Some(timeout) = association_idle_timeout {
            next.association_idle_timeout = timeout;
        }
        next.validate(self.inner.limits.max_associations)?;
        if state.associations.lock().await.len() > next.max_associations {
            return Err(DatagramRuntimeError::Invalid(
                "max_associations cannot be lower than the current active count".into(),
            ));
        }
        let listen_changed = next.listen != old.listen;
        let upstream_changed = next.upstream != old.upstream;
        let desired_running = enabled.unwrap_or(running);
        let prebound = if desired_running && (!running || listen_changed) {
            Some(Arc::new(
                UdpSocket::bind(next.listen)
                    .await
                    .map_err(DatagramRuntimeError::Bind)?,
            ))
        } else {
            None
        };
        let bound = if let Some(socket) = &prebound {
            Some(socket.local_addr().map_err(DatagramRuntimeError::Bind)?)
        } else {
            None
        };
        let must_stop_old = running && (!desired_running || listen_changed);
        let old_owner = if must_stop_old {
            let mut proxies = self.inner.proxies.lock().await;
            proxies
                .get_mut(name)
                .and_then(|managed| managed.listener.take())
        } else {
            None
        };
        if let Some(owner) = old_owner {
            owner.cancel.cancel();
            let _ = owner.task.await;
        }
        if upstream_changed && !must_stop_old {
            let stale = {
                let mut associations = state.associations.lock().await;
                *state.spec.write().expect("datagram proxy spec") = next;
                associations
                    .drain()
                    .map(|(_, association)| association)
                    .collect::<Vec<_>>()
            };
            for association in stale {
                stop_association(&state, association, true).await;
            }
        } else {
            *state.spec.write().expect("datagram proxy spec") = next;
        }
        if let Some(socket) = prebound {
            let owner = spawn_listener(socket, state.clone());
            let mut proxies = self.inner.proxies.lock().await;
            let managed = proxies
                .get_mut(name)
                .ok_or_else(|| DatagramRuntimeError::NotFound(name.into()))?;
            managed.listener = Some(owner);
        }
        let proxies = self.inner.proxies.lock().await;
        let managed = proxies
            .get(name)
            .ok_or_else(|| DatagramRuntimeError::NotFound(name.into()))?;
        Ok(proxy_view(
            &state,
            bound.or_else(|| managed.listener.as_ref().map(|owner| owner.bound)),
            managed.listener.is_some(),
        )
        .await)
    }

    /// Read a directional plan and its generation/namespace as one snapshot.
    pub async fn fault_plan(
        &self,
        name: &str,
        direction: Direction,
    ) -> Option<(DatagramPlan, u64, u64)> {
        let proxies = self.inner.proxies.lock().await;
        let state = &proxies.get(name)?.state;
        let spec = state.spec.read().expect("datagram proxy spec").clone();
        let snapshot = match direction {
            Direction::Upstream => spec.upstream_policy.snapshot(),
            Direction::Downstream => spec.downstream_policy.snapshot(),
        };
        Some((
            (*snapshot.plan).clone(),
            snapshot.generation,
            snapshot.seed_namespace,
        ))
    }

    /// Publish one directional plan with optional expected-generation guard.
    pub async fn publish_fault_plan(
        &self,
        name: &str,
        direction: Direction,
        plan: DatagramPlan,
        seed_namespace: u64,
        expected_generation: Option<u64>,
    ) -> Result<u64, DatagramRuntimeError> {
        plan.validate()
            .map_err(|error| DatagramRuntimeError::Invalid(error.into()))?;
        let proxies = self.inner.proxies.lock().await;
        let state = &proxies
            .get(name)
            .ok_or_else(|| DatagramRuntimeError::NotFound(name.into()))?
            .state;
        let _guard = state.policy_mutation.lock().await;
        let spec = state.spec.read().expect("datagram proxy spec").clone();
        let policy = match direction {
            Direction::Upstream => &spec.upstream_policy,
            Direction::Downstream => &spec.downstream_policy,
        };
        let current = policy.snapshot();
        if expected_generation.is_some_and(|expected| expected != current.generation) {
            return Err(DatagramRuntimeError::Conflict(format!(
                "generation moved from {} to {}",
                expected_generation.unwrap(),
                current.generation
            )));
        }
        policy
            .publish(plan, seed_namespace)
            .map(|snapshot| snapshot.generation)
            .map_err(|error| DatagramRuntimeError::Invalid(error.into()))
    }

    /// Active association snapshots.
    pub async fn associations(&self) -> Vec<DatagramAssociationSnapshot> {
        let proxies = self.inner.proxies.lock().await;
        let mut views = Vec::new();
        for managed in proxies.values() {
            let associations = managed.state.associations.lock().await;
            for association in associations.values() {
                views.push(
                    association
                        .record
                        .lock()
                        .expect("association record")
                        .snapshot(Instant::now()),
                );
            }
        }
        views.sort_by_key(|x| x.id);
        views
    }

    /// Active plus retained completed association evidence, newest last.
    pub async fn all_associations(&self) -> Vec<DatagramAssociationSnapshot> {
        let mut views = self
            .inner
            .history
            .lock()
            .expect("association history")
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        views.extend(self.associations().await);
        views.sort_by_key(|view| view.id);
        views
    }

    /// Active or retained association snapshot.
    pub async fn association(&self, id: u64) -> Option<DatagramAssociationSnapshot> {
        let proxies = self.inner.proxies.lock().await;
        for managed in proxies.values() {
            let associations = managed.state.associations.lock().await;
            if let Some(association) = associations.values().find(|x| x.id == id) {
                return Some(
                    association
                        .record
                        .lock()
                        .expect("association record")
                        .snapshot(Instant::now()),
                );
            }
        }
        self.inner
            .history
            .lock()
            .expect("association history")
            .iter()
            .find(|x| x.id == id)
            .cloned()
    }

    /// Explicitly cancel one association and record queued administrative discards.
    pub async fn kill_association(&self, id: u64) -> bool {
        let proxies = self.inner.proxies.lock().await;
        for managed in proxies.values() {
            let found = {
                let mut associations = managed.state.associations.lock().await;
                let key = associations
                    .iter()
                    .find_map(|(key, association)| (association.id == id).then_some(*key));
                key.and_then(|key| associations.remove(&key))
            };
            if let Some(association) = found {
                stop_association(&managed.state, association, true).await;
                return true;
            }
        }
        false
    }

    /// Stop every listener and join all association tasks owned by this runtime.
    pub async fn shutdown(&self) {
        let names: Vec<_> = self.inner.proxies.lock().await.keys().cloned().collect();
        for name in names {
            let _ = self.delete_proxy(&name).await;
        }
    }
}

async fn proxy_view(
    state: &ProxyState,
    bound: Option<SocketAddr>,
    running: bool,
) -> DatagramProxyView {
    let spec = state.spec.read().expect("datagram proxy spec").clone();
    DatagramProxyView {
        name: spec.name.clone(),
        listen: spec.listen,
        bound_addr: bound,
        upstream: spec.upstream,
        running,
        max_associations: spec.max_associations,
        association_idle_timeout_ms: spec
            .association_idle_timeout
            .as_millis()
            .min(u64::MAX as u128) as u64,
        max_datagram_size: spec.queue_limits.max_datagram_bytes.get(),
        max_queued_datagrams: spec.queue_limits.max_queued_datagrams.get(),
        max_queued_bytes: spec.queue_limits.max_queued_bytes.get(),
        seed: spec.seed,
        active_associations: state.associations.lock().await.len(),
        upstream_generation: spec.upstream_policy.snapshot().generation,
        downstream_generation: spec.downstream_policy.snapshot().generation,
        oversize_datagrams: state.counters.oversize_datagrams.load(Ordering::Relaxed),
        association_capacity_rejections: state.counters.capacity_rejections.load(Ordering::Relaxed),
        ingress_queue_overflow: state.counters.ingress_overflow.load(Ordering::Relaxed),
        association_setup_failures: state
            .counters
            .association_setup_failures
            .load(Ordering::Relaxed),
    }
}

fn spawn_listener(socket: Arc<UdpSocket>, state: Arc<ProxyState>) -> ListenerOwner {
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let bound = socket
        .local_addr()
        .expect("bound UDP socket has local address");
    let task = tokio::spawn(async move {
        listener_loop(socket, state, task_cancel).await;
    });
    ListenerOwner {
        bound,
        cancel,
        task,
    }
}

fn reserve_ingress(
    counter: Arc<AtomicUsize>,
    bytes: usize,
    limit: usize,
) -> Option<IngressReservation> {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current.checked_add(bytes).filter(|next| *next <= limit)
        })
        .ok()?;
    Some(IngressReservation { counter, bytes })
}

async fn listener_loop(socket: Arc<UdpSocket>, state: Arc<ProxyState>, cancel: CancellationToken) {
    let mut recv = vec![0u8; UDP_RECEIVE_BUFFER_BYTES];
    let mut reaper = interval(Duration::from_millis(10));
    reaper.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            received = socket.recv_from(&mut recv) => {
                let Ok((size, client)) = received else { continue };
                receive_client_datagram(&socket, &state, client, Bytes::copy_from_slice(&recv[..size])).await;
            }
            _ = reaper.tick() => reap_idle(&state).await,
        }
    }
    let associations: Vec<_> = state
        .associations
        .lock()
        .await
        .drain()
        .map(|(_, value)| value)
        .collect();
    for association in associations {
        stop_association(&state, association, true).await;
    }
}

async fn receive_client_datagram(
    socket: &Arc<UdpSocket>,
    state: &Arc<ProxyState>,
    client: SocketAddr,
    payload: Bytes,
) {
    let spec = state.spec.read().expect("datagram proxy spec").clone();
    if payload.len() as u64 > spec.queue_limits.max_datagram_bytes.get() {
        state
            .counters
            .oversize_datagrams
            .fetch_add(1, Ordering::Relaxed);
        if let Some(association) = state.associations.lock().await.get(&client).cloned() {
            let mut record = association.record.lock().expect("association record");
            record.oversize_datagrams = record.oversize_datagrams.saturating_add(1);
            record.last_activity = Instant::now();
        }
        return;
    }
    let existing = { state.associations.lock().await.get(&client).cloned() };
    let association = match existing {
        Some(association) => association,
        None => match create_association(socket.clone(), state, client).await {
            Ok(association) => association,
            Err(DatagramRuntimeError::AssociationLimit) => {
                state
                    .counters
                    .capacity_rejections
                    .fetch_add(1, Ordering::Relaxed);
                return;
            }
            Err(_) => {
                state
                    .counters
                    .association_setup_failures
                    .fetch_add(1, Ordering::Relaxed);
                return;
            }
        },
    };
    let policy = spec.upstream_policy.snapshot();
    let ingress_bytes = payload.len() as u64;
    let Some(reservation) = reserve_ingress(
        state.ingress_buffered_bytes.clone(),
        payload.len(),
        state.limits.max_ingress_queue_bytes,
    ) else {
        state
            .counters
            .ingress_overflow
            .fetch_add(1, Ordering::Relaxed);
        let mut record = association.record.lock().expect("association record");
        record.ingress_queue_overflow = record.ingress_queue_overflow.saturating_add(1);
        record.last_activity = Instant::now();
        return;
    };
    let ingress = Ingress {
        payload,
        policy,
        _reservation: reservation,
    };
    association.pending_ingress.fetch_add(1, Ordering::Relaxed);
    match association.sender.try_send(ingress) {
        Ok(()) => {
            let mut record = association.record.lock().expect("association record");
            record.ingress_datagrams = record.ingress_datagrams.saturating_add(1);
            record.ingress_bytes = record.ingress_bytes.saturating_add(ingress_bytes);
            record.last_activity = Instant::now();
        }
        Err(mpsc::error::TrySendError::Full(_)) => {
            association.pending_ingress.fetch_sub(1, Ordering::Relaxed);
            state
                .counters
                .ingress_overflow
                .fetch_add(1, Ordering::Relaxed);
            let mut record = association.record.lock().expect("association record");
            record.ingress_queue_overflow = record.ingress_queue_overflow.saturating_add(1);
            record.last_activity = Instant::now();
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            association.pending_ingress.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

async fn create_association(
    socket: Arc<UdpSocket>,
    state: &Arc<ProxyState>,
    client: SocketAddr,
) -> Result<Arc<Association>, DatagramRuntimeError> {
    let mut associations = state.associations.lock().await;
    if let Some(association) = associations.get(&client) {
        return Ok(association.clone());
    }
    let spec = state.spec.read().expect("datagram proxy spec").clone();
    if associations.len() >= spec.max_associations {
        return Err(DatagramRuntimeError::AssociationLimit);
    }
    let prior = state.global_active.fetch_add(1, Ordering::AcqRel);
    if prior >= state.limits.max_associations {
        state.global_active.fetch_sub(1, Ordering::AcqRel);
        return Err(DatagramRuntimeError::AssociationLimit);
    }
    let local = match spec.upstream.ip() {
        IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    };
    let upstream = match UdpSocket::bind(local).await {
        Ok(socket) => socket,
        Err(error) => {
            state.global_active.fetch_sub(1, Ordering::AcqRel);
            return Err(DatagramRuntimeError::Bind(error));
        }
    };
    if let Err(error) = upstream.connect(spec.upstream).await {
        state.global_active.fetch_sub(1, Ordering::AcqRel);
        return Err(DatagramRuntimeError::Bind(error));
    }
    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let now = Instant::now();
    let record = Arc::new(Mutex::new(AssociationRecord {
        id,
        proxy: spec.name.clone(),
        client,
        upstream: spec.upstream,
        created: now,
        last_activity: now,
        ingress_datagrams: 0,
        ingress_bytes: 0,
        egress_datagrams: 0,
        egress_bytes: 0,
        ingress_queue_overflow: 0,
        oversize_datagrams: 0,
        administrative_discards: 0,
        send_errors: 0,
        upstream_evidence: DatagramEvidence::default(),
        downstream_evidence: DatagramEvidence::default(),
    }));
    let (sender, receiver) = mpsc::channel(state.limits.ingress_per_association);
    let cancel = CancellationToken::new();
    let association = Arc::new(Association {
        id,
        client,
        sender,
        pending_ingress: AtomicUsize::new(0),
        administrative_cancel: AtomicUsize::new(0),
        cancel: cancel.clone(),
        worker: AsyncMutex::new(None),
        record: record.clone(),
    });
    let worker = tokio::spawn(association_loop(
        upstream,
        socket,
        spec,
        receiver,
        association.clone(),
        record,
        state.counters.clone(),
    ));
    *association.worker.lock().await = Some(worker);
    associations.insert(client, association.clone());
    Ok(association)
}

async fn association_loop(
    upstream_socket: UdpSocket,
    listener_socket: Arc<UdpSocket>,
    spec: DatagramProxySpec,
    mut ingress: mpsc::Receiver<Ingress>,
    association: Arc<Association>,
    record: Arc<Mutex<AssociationRecord>>,
    counters: Arc<ProxyCounters>,
) {
    let mut upstream = DatagramDirectionEngine::new(
        spec.queue_limits,
        &spec.name,
        association.id,
        Direction::Upstream,
        RngVersion::V1,
    )
    .expect("validated queue limits");
    let mut downstream = DatagramDirectionEngine::new(
        spec.queue_limits,
        &spec.name,
        association.id,
        Direction::Downstream,
        RngVersion::V1,
    )
    .expect("validated queue limits");
    let mut recv = vec![0u8; UDP_RECEIVE_BUFFER_BYTES];
    let send_context = AssociationSendContext {
        upstream_socket: &upstream_socket,
        listener_socket: &listener_socket,
        client: association.client,
        record: &record,
        cancel: &association.cancel,
    };
    loop {
        let now = Instant::now();
        if emit_ready(&send_context, &mut upstream, &mut downstream, now).await {
            break;
        }
        let next = [upstream.next_deadline(), downstream.next_deadline()]
            .into_iter()
            .flatten()
            .min();
        tokio::select! {
            biased;
            _ = association.cancel.cancelled() => break,
            item = ingress.recv() => match item {
                Some(item) => {
                    association.pending_ingress.fetch_sub(1, Ordering::Relaxed);
                    upstream.admit(Instant::now(), item.payload, &item.policy);
                    let mut r = record.lock().expect("association record"); r.last_activity = Instant::now();
                }
                None => break,
            },
            received = upstream_socket.recv(&mut recv) => match received {
                Ok(size) => {
                    if size as u64 > spec.queue_limits.max_datagram_bytes.get() {
                        counters.oversize_datagrams.fetch_add(1, Ordering::Relaxed);
                        let mut r = record.lock().expect("association record"); r.oversize_datagrams = r.oversize_datagrams.saturating_add(1); r.last_activity = Instant::now();
                    } else {
                        downstream.admit(Instant::now(), Bytes::copy_from_slice(&recv[..size]), &spec.downstream_policy.snapshot());
                        let mut r = record.lock().expect("association record"); r.last_activity = Instant::now();
                    }
                }
                Err(_) => { let mut r = record.lock().expect("association record"); r.send_errors = r.send_errors.saturating_add(1); }
            },
            _ = wait_deadline(next) => {},
        }
        update_evidence(&record, &upstream, &downstream);
    }
    let discarded = upstream
        .discard_all()
        .saturating_add(downstream.discard_all()) as u64
        + ingress.len() as u64;
    let mut r = record.lock().expect("association record");
    if association.administrative_cancel.load(Ordering::Relaxed) > 0 {
        r.administrative_discards = r.administrative_discards.saturating_add(discarded);
    }
    r.upstream_evidence = upstream.evidence().clone();
    r.downstream_evidence = downstream.evidence().clone();
}

async fn wait_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending::<()>().await,
    }
}

async fn emit_ready(
    context: &AssociationSendContext<'_>,
    upstream: &mut DatagramDirectionEngine,
    downstream: &mut DatagramDirectionEngine,
    now: Instant,
) -> bool {
    let ready = upstream.take_ready(now);
    for (index, DatagramScheduled { payload, .. }) in ready.iter().enumerate() {
        let result = tokio::select! {
            _ = context.cancel.cancelled() => {
                let mut r = context.record.lock().expect("association record");
                r.administrative_discards = r.administrative_discards.saturating_add((ready.len() - index) as u64);
                return true;
            }
            result = context.upstream_socket.send(payload) => result,
        };
        match result {
            Ok(size) if size == payload.len() => {
                let mut r = context.record.lock().expect("association record");
                r.egress_datagrams = r.egress_datagrams.saturating_add(1);
                r.egress_bytes = r.egress_bytes.saturating_add(size as u64);
            }
            _ => {
                let mut r = context.record.lock().expect("association record");
                r.send_errors = r.send_errors.saturating_add(1);
            }
        }
    }
    let ready = downstream.take_ready(now);
    for (index, DatagramScheduled { payload, .. }) in ready.iter().enumerate() {
        let result = tokio::select! {
            _ = context.cancel.cancelled() => {
                let mut r = context.record.lock().expect("association record");
                r.administrative_discards = r.administrative_discards.saturating_add((ready.len() - index) as u64);
                return true;
            }
            result = context.listener_socket.send_to(payload, context.client) => result,
        };
        match result {
            Ok(size) if size == payload.len() => {
                let mut r = context.record.lock().expect("association record");
                r.egress_datagrams = r.egress_datagrams.saturating_add(1);
                r.egress_bytes = r.egress_bytes.saturating_add(size as u64);
            }
            _ => {
                let mut r = context.record.lock().expect("association record");
                r.send_errors = r.send_errors.saturating_add(1);
            }
        }
    }
    false
}

fn update_evidence(
    record: &Arc<Mutex<AssociationRecord>>,
    upstream: &DatagramDirectionEngine,
    downstream: &DatagramDirectionEngine,
) {
    let mut r = record.lock().expect("association record");
    r.upstream_evidence = upstream.evidence().clone();
    r.downstream_evidence = downstream.evidence().clone();
}

async fn reap_idle(state: &Arc<ProxyState>) {
    let now = Instant::now();
    let idle_timeout = state
        .spec
        .read()
        .expect("datagram proxy spec")
        .association_idle_timeout;
    let expired: Vec<SocketAddr> = {
        let associations = state.associations.lock().await;
        associations
            .iter()
            .filter_map(|(client, association)| {
                let record = association.record.lock().expect("association record");
                let queues_empty = record.upstream_evidence.queued_datagrams == 0
                    && record.downstream_evidence.queued_datagrams == 0;
                (queues_empty
                    && association.pending_ingress.load(Ordering::Relaxed) == 0
                    && now.saturating_duration_since(record.last_activity) >= idle_timeout)
                    .then_some(*client)
            })
            .collect()
    };
    for client in expired {
        if let Some(association) = state.associations.lock().await.remove(&client) {
            stop_association(state, association, false).await;
        }
    }
}

async fn stop_association(state: &ProxyState, association: Arc<Association>, administrative: bool) {
    if administrative {
        association
            .administrative_cancel
            .store(1, Ordering::Relaxed);
    }
    association.cancel.cancel();
    if let Some(worker) = association.worker.lock().await.take() {
        let _ = worker.await;
    }
    state.global_active.fetch_sub(1, Ordering::AcqRel);
    let snapshot = association
        .record
        .lock()
        .expect("association record")
        .snapshot(Instant::now());
    let mut history = state.history.lock().expect("association history");
    if state.limits.history > 0 {
        while history.len() >= state.limits.history {
            history.pop_front();
        }
        history.push_back(snapshot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU64;
    use tokio::time::timeout;

    fn limits() -> DatagramQueueLimits {
        DatagramQueueLimits {
            max_queued_datagrams: NonZeroU64::new(16).unwrap(),
            max_queued_bytes: NonZeroU64::new(65_536).unwrap(),
            max_datagram_bytes: NonZeroU64::new(65_535).unwrap(),
        }
    }

    async fn echo_target() -> (SocketAddr, CancellationToken) {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        let cancel = CancellationToken::new();
        let done = cancel.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 65_536];
            loop {
                tokio::select! { _ = done.cancelled() => break, result = socket.recv_from(&mut buf) => if let Ok((len, peer)) = result { let _ = socket.send_to(&buf[..len], peer).await; } }
            }
        });
        (addr, cancel)
    }

    async fn multi_response_target() -> (SocketAddr, CancellationToken, Arc<Mutex<Vec<SocketAddr>>>)
    {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        let cancel = CancellationToken::new();
        let done = cancel.clone();
        let peers = Arc::new(Mutex::new(Vec::new()));
        let known_peers = peers.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 65_536];
            loop {
                tokio::select! {
                    _ = done.cancelled() => break,
                    result = socket.recv_from(&mut buf) => if let Ok((len, peer)) = result {
                        if &buf[..len] == b"push" {
                            let recipients = known_peers.lock().expect("peer list").clone();
                            for recipient in recipients { let _ = socket.send_to(b"unsolicited", recipient).await; }
                        } else {
                            known_peers.lock().expect("peer list").push(peer);
                            let first = [&buf[..len], b":one"].concat();
                            let second = [&buf[..len], b":two"].concat();
                            let _ = socket.send_to(&first, peer).await;
                            let _ = socket.send_to(&second, peer).await;
                        }
                    }
                }
            }
        });
        (addr, cancel, peers)
    }

    #[tokio::test]
    async fn fixed_target_isolation_and_multiple_clients() {
        let (target, stop_target) = echo_target().await;
        let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default()).unwrap();
        let spec = DatagramProxySpec::new("echo", "127.0.0.1:0".parse().unwrap(), target, limits())
            .unwrap();
        let view = runtime.create_proxy(spec).await.unwrap();
        let listen = view.bound_addr.unwrap();
        let c1 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let c2 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        c1.send_to(b"client-one", listen).await.unwrap();
        c2.send_to(b"client-two", listen).await.unwrap();
        let mut b1 = [0u8; 64];
        let mut b2 = [0u8; 64];
        let (n1, _) = timeout(Duration::from_secs(2), c1.recv_from(&mut b1))
            .await
            .unwrap()
            .unwrap();
        let (n2, _) = timeout(Duration::from_secs(2), c2.recv_from(&mut b2))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&b1[..n1], b"client-one");
        assert_eq!(&b2[..n2], b"client-two");
        let associations = runtime.associations().await;
        assert_eq!(associations.len(), 2);
        assert_ne!(associations[0].id, associations[1].id);
        assert!(associations.iter().all(|a| a.egress_datagrams == 2));
        runtime.delete_proxy("echo").await.unwrap();
        stop_target.cancel();
        assert!(runtime.associations().await.is_empty());
    }

    #[tokio::test]
    async fn explicit_kill_records_admin_discard_and_disable_reenable_is_clean() {
        let (target, stop_target) = echo_target().await;
        let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default()).unwrap();
        let spec = DatagramProxySpec::new("echo", "127.0.0.1:0".parse().unwrap(), target, limits())
            .unwrap();
        let delay = eggchaos_core::DatagramFaultSpec {
            id: eggchaos_core::FaultId::new("delay").unwrap(),
            probability: eggchaos_core::Probability::new(1.0).unwrap(),
            kind: eggchaos_core::DatagramFaultKind::Delay {
                delay: Duration::from_secs(30),
                jitter: Duration::ZERO,
            },
        };
        spec.upstream_policy
            .publish(DatagramPlan::new(vec![delay]).unwrap(), 0)
            .unwrap();
        let view = runtime.create_proxy(spec).await.unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client
            .send_to(b"ping", view.bound_addr.unwrap())
            .await
            .unwrap();
        let pending = timeout(Duration::from_secs(2), async {
            loop {
                if let Some(association) = runtime.associations().await.first() {
                    if association.upstream_evidence.queued_datagrams == 1 {
                        break association.id;
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let id = pending;
        assert!(runtime.kill_association(id).await);
        let final_view = runtime.association(id).await.unwrap();
        assert_eq!(final_view.administrative_discards, 1);
        assert_eq!(final_view.upstream_evidence.configured_loss, 0);
        runtime.disable_proxy("echo").await.unwrap();
        assert!(!runtime.proxies().await[0].running);
        let restarted = runtime.enable_proxy("echo").await.unwrap();
        assert!(restarted.running);
        assert_ne!(restarted.bound_addr.unwrap().port(), 0);
        runtime.delete_proxy("echo").await.unwrap();
        stop_target.cancel();
    }

    #[tokio::test]
    async fn capacity_rejects_clients_and_oversize_is_classified_after_receive() {
        let (target, stop_target) = echo_target().await;
        let runtime = DatagramRuntime::new(DatagramRuntimeLimits {
            max_associations: 1,
            ..DatagramRuntimeLimits::default()
        })
        .unwrap();
        let queue_limits = DatagramQueueLimits {
            max_datagram_bytes: NonZeroU64::new(4).unwrap(),
            ..limits()
        };
        let mut spec =
            DatagramProxySpec::new("echo", "127.0.0.1:0".parse().unwrap(), target, queue_limits)
                .unwrap();
        spec.max_associations = 1;
        let view = runtime.create_proxy(spec).await.unwrap();
        let c1 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let c2 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        c1.send_to(b"ok", view.bound_addr.unwrap()).await.unwrap();
        let mut buf = [0u8; 64];
        let _ = timeout(Duration::from_secs(2), c1.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        c2.send_to(b"12345678", view.bound_addr.unwrap())
            .await
            .unwrap();
        c2.send_to(b"z", view.bound_addr.unwrap()).await.unwrap();
        assert_eq!(runtime.associations().await.len(), 1);
        let view = timeout(Duration::from_secs(2), async {
            loop {
                let view = runtime.proxies().await.remove(0);
                if view.oversize_datagrams == 1 && view.association_capacity_rejections == 1 {
                    break view;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(view.oversize_datagrams, 1);
        assert_eq!(view.association_capacity_rejections, 1);
        runtime.delete_proxy("echo").await.unwrap();
        stop_target.cancel();
    }

    #[tokio::test]
    async fn association_routes_multiple_and_unsolicited_replies_and_keeps_source_port() {
        let (target, stop_target, peers) = multi_response_target().await;
        let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default()).unwrap();
        let spec =
            DatagramProxySpec::new("multi", "127.0.0.1:0".parse().unwrap(), target, limits())
                .unwrap();
        let view = runtime.create_proxy(spec).await.unwrap();
        let listen = view.bound_addr.unwrap();
        let c1 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let c2 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        c1.send_to(b"a", listen).await.unwrap();
        c2.send_to(b"b", listen).await.unwrap();
        let mut buf = [0u8; 64];
        let mut a = Vec::new();
        let mut b = Vec::new();
        for _ in 0..2 {
            let (n, _) = timeout(Duration::from_secs(2), c1.recv_from(&mut buf))
                .await
                .unwrap()
                .unwrap();
            a.push(buf[..n].to_vec());
        }
        for _ in 0..2 {
            let (n, _) = timeout(Duration::from_secs(2), c2.recv_from(&mut buf))
                .await
                .unwrap()
                .unwrap();
            b.push(buf[..n].to_vec());
        }
        assert!(a.iter().all(|x| x.starts_with(b"a:")));
        assert!(b.iter().all(|x| x.starts_with(b"b:")));
        let first_peers = peers.lock().unwrap().clone();
        assert_eq!(first_peers.len(), 2);
        assert_ne!(first_peers[0], first_peers[1]);
        c1.send_to(b"a2", listen).await.unwrap();
        for _ in 0..2 {
            let _ = timeout(Duration::from_secs(2), c1.recv_from(&mut buf))
                .await
                .unwrap()
                .unwrap();
        }
        assert_eq!(peers.lock().unwrap()[2], first_peers[0]);
        let push = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        push.send_to(b"push", listen).await.unwrap();
        let (n1, _) = timeout(Duration::from_secs(2), c1.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf[..n1], b"unsolicited");
        let (n2, _) = timeout(Duration::from_secs(2), c2.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf[..n2], b"unsolicited");
        runtime.shutdown().await;
        assert!(runtime.proxies().await.is_empty());
        stop_target.cancel();
    }

    #[tokio::test]
    async fn delayed_datagram_prevents_idle_association_expiry() {
        let (target, stop_target) = echo_target().await;
        let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default()).unwrap();
        let mut spec =
            DatagramProxySpec::new("delay", "127.0.0.1:0".parse().unwrap(), target, limits())
                .unwrap();
        spec.association_idle_timeout = Duration::from_millis(30);
        let fault = eggchaos_core::DatagramFaultSpec {
            id: eggchaos_core::FaultId::new("delay").unwrap(),
            probability: eggchaos_core::Probability::new(1.0).unwrap(),
            kind: eggchaos_core::DatagramFaultKind::Delay {
                delay: Duration::from_millis(180),
                jitter: Duration::ZERO,
            },
        };
        spec.upstream_policy
            .publish(DatagramPlan::new(vec![fault]).unwrap(), 0)
            .unwrap();
        let view = runtime.create_proxy(spec).await.unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client
            .send_to(b"held", view.bound_addr.unwrap())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(90)).await;
        assert_eq!(runtime.associations().await.len(), 1);
        let mut buf = [0u8; 64];
        let (size, _) = timeout(Duration::from_secs(2), client.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf[..size], b"held");
        runtime.shutdown().await;
        stop_target.cancel();
    }

    #[tokio::test]
    async fn duplicate_policy_is_embedded_in_both_directions() {
        let (target, stop_target) = echo_target().await;
        let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default()).unwrap();
        let spec = DatagramProxySpec::new(
            "duplicate",
            "127.0.0.1:0".parse().unwrap(),
            target,
            limits(),
        )
        .unwrap();
        let duplicate = eggchaos_core::DatagramFaultSpec {
            id: eggchaos_core::FaultId::new("duplicate").unwrap(),
            probability: eggchaos_core::Probability::new(1.0).unwrap(),
            kind: eggchaos_core::DatagramFaultKind::Duplicate {
                additional_copies: 1,
            },
        };
        let plan = DatagramPlan::new(vec![duplicate]).unwrap();
        spec.upstream_policy.publish(plan.clone(), 0).unwrap();
        spec.downstream_policy.publish(plan, 0).unwrap();
        let view = runtime.create_proxy(spec).await.unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client
            .send_to(b"twice", view.bound_addr.unwrap())
            .await
            .unwrap();
        let mut buf = [0u8; 32];
        for _ in 0..4 {
            let (size, _) = timeout(Duration::from_secs(2), client.recv_from(&mut buf))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&buf[..size], b"twice");
        }
        let snapshot = runtime.associations().await.remove(0);
        assert_eq!(snapshot.upstream_evidence.duplicated_copies, 1);
        assert_eq!(snapshot.downstream_evidence.duplicated_copies, 2);
        runtime.shutdown().await;
        stop_target.cancel();
    }

    #[tokio::test]
    async fn restart_patch_bind_failure_keeps_old_udp_listener_serving() {
        let (target, stop_target) = echo_target().await;
        let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default()).unwrap();
        let spec =
            DatagramProxySpec::new("rollback", "127.0.0.1:0".parse().unwrap(), target, limits())
                .unwrap();
        let original = runtime.create_proxy(spec).await.unwrap();
        let blocker = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let conflict_addr = blocker.local_addr().unwrap();
        let result = runtime
            .update_proxy("rollback", Some(conflict_addr), None, None, None, None)
            .await;
        assert!(matches!(result, Err(DatagramRuntimeError::Bind(_))));
        let current = runtime.proxy("rollback").await.unwrap();
        assert_eq!(current.bound_addr, original.bound_addr);
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client
            .send_to(b"still-serving", current.bound_addr.unwrap())
            .await
            .unwrap();
        let mut buf = [0u8; 32];
        let (size, _) = timeout(Duration::from_secs(2), client.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf[..size], b"still-serving");
        runtime.shutdown().await;
        stop_target.cancel();
    }

    #[tokio::test]
    async fn ipv6_loopback_works_when_host_capability_is_available() {
        let target_socket = match UdpSocket::bind("[::1]:0").await {
            Ok(socket) => socket,
            Err(error) => {
                println!("SKIP IPv6 UDP loopback unavailable: {error}");
                return;
            }
        };
        let target = target_socket.local_addr().unwrap();
        let target_cancel = CancellationToken::new();
        let target_done = target_cancel.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 65_536];
            loop {
                tokio::select! {
                    _ = target_done.cancelled() => break,
                    result = target_socket.recv_from(&mut buf) => if let Ok((size, peer)) = result { let _ = target_socket.send_to(&buf[..size], peer).await; }
                }
            }
        });
        let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default()).unwrap();
        let spec =
            DatagramProxySpec::new("v6", "[::1]:0".parse().unwrap(), target, limits()).unwrap();
        let proxy = match runtime.create_proxy(spec).await {
            Ok(proxy) => proxy,
            Err(DatagramRuntimeError::Bind(error)) => {
                println!("SKIP IPv6 UDP listener unavailable: {error}");
                target_cancel.cancel();
                return;
            }
            Err(error) => panic!("unexpected IPv6 proxy error: {error}"),
        };
        let client = match UdpSocket::bind("[::1]:0").await {
            Ok(client) => client,
            Err(error) => {
                println!("SKIP IPv6 UDP client unavailable: {error}");
                runtime.shutdown().await;
                target_cancel.cancel();
                return;
            }
        };
        client
            .send_to(b"v6", proxy.bound_addr.unwrap())
            .await
            .unwrap();
        let mut buf = [0u8; 8];
        let (size, _) = timeout(Duration::from_secs(2), client.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf[..size], b"v6");
        runtime.shutdown().await;
        target_cancel.cancel();
    }
}
