use std::{
    collections::HashMap,
    io,
    net::SocketAddr,
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use eggchaos_core::{ChaosStream, Direction, FaultPlan, LivePolicy};
use egress_relay::{HalfClosePolicy, RelayOptions};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Mutex, Notify, RwLock},
    task::{JoinHandle, JoinSet},
    time::timeout,
};

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
    fn validate(&self) -> Result<(), EggchaosError> {
        if self.name.is_empty() || self.name.len() > 128 {
            return Err(EggchaosError::InvalidProxy(
                "proxy name must be 1..=128 bytes".into(),
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
    /// Seed namespace.
    pub seed: u64,
    /// Configuration generation observed at accept time.
    pub generation: u64,
}

#[derive(Clone)]
struct RuntimeState {
    shutting_down: Arc<AtomicBool>,
    notify: Arc<Notify>,
    next_id: Arc<AtomicU64>,
    active: Arc<AtomicUsize>,
    connections: Arc<RwLock<HashMap<u64, ConnectionSnapshot>>>,
    cancellations: Arc<RwLock<HashMap<u64, Arc<Notify>>>>,
    limits: AdmissionLimits,
}

type ListenerTask = JoinHandle<Result<(), EggchaosError>>;

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

/// Builder for a structured multi-proxy service.
pub struct ServiceBuilder {
    seed: u64,
    proxies: Vec<ProxySpec>,
    limits: AdmissionLimits,
    half_close: HalfClosePolicy,
    relay_buffer: NonZeroUsize,
}

impl ServiceBuilder {
    /// Create a builder with an explicit run seed.
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            proxies: Vec::new(),
            limits: AdmissionLimits::default(),
            half_close: HalfClosePolicy::Drain,
            relay_buffer: NonZeroUsize::new(64 * 1024).unwrap(),
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
            seed: self.seed,
            proxies: self.proxies,
            limits: self.limits,
            half_close: self.half_close,
            relay_buffer: self.relay_buffer,
        })
    }
}

/// A configured but not yet started service.
pub struct EggchaosService {
    seed: u64,
    proxies: Vec<ProxySpec>,
    limits: AdmissionLimits,
    half_close: HalfClosePolicy,
    relay_buffer: NonZeroUsize,
}

impl EggchaosService {
    /// Return a builder.
    pub fn builder(seed: u64) -> ServiceBuilder {
        ServiceBuilder::new(seed)
    }
    /// Start all enabled listeners, returning actual bound addresses.
    pub async fn start(self) -> Result<ServiceHandle, EggchaosError> {
        let mut bound = Vec::new();
        for proxy in self.proxies.iter().filter(|p| p.enabled) {
            let listener =
                TcpListener::bind(proxy.listen)
                    .await
                    .map_err(|source| EggchaosError::Io {
                        context: format!("bind proxy {}", proxy.name),
                        source,
                    })?;
            bound.push((proxy.clone(), listener));
        }
        let state = RuntimeState {
            shutting_down: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
            next_id: Arc::new(AtomicU64::new(1)),
            active: Arc::new(AtomicUsize::new(0)),
            connections: Arc::new(RwLock::new(HashMap::new())),
            cancellations: Arc::new(RwLock::new(HashMap::new())),
            limits: self.limits,
        };
        let mut addresses = HashMap::new();
        let mut tasks = Vec::new();
        for (proxy, listener) in bound {
            addresses.insert(
                proxy.name.clone(),
                listener.local_addr().map_err(|source| EggchaosError::Io {
                    context: "read bound address".into(),
                    source,
                })?,
            );
            let state_clone = state.clone();
            tasks.push(tokio::spawn(listen_loop(
                listener,
                proxy,
                self.seed,
                state_clone,
                self.half_close,
                self.relay_buffer,
            )));
        }
        let control = crate::ControlState::new(self.proxies.clone());
        control
            .attach_runtime(state.connections.clone(), state.cancellations.clone())
            .await;
        Ok(ServiceHandle {
            state,
            addresses,
            tasks: Mutex::new(Some(tasks)),
            control,
        })
    }
}

/// Runtime control handle with structured listener ownership.
pub struct ServiceHandle {
    state: RuntimeState,
    addresses: HashMap<String, SocketAddr>,
    tasks: Mutex<Option<Vec<ListenerTask>>>,
    control: crate::ControlState,
}

impl ServiceHandle {
    /// Return actual bound addresses for enabled proxies.
    pub fn bound_proxies(&self) -> &HashMap<String, SocketAddr> {
        &self.addresses
    }
    /// Request service shutdown. The request is level-triggered.
    pub fn shutdown(&self) {
        self.state.shutting_down.store(true, Ordering::Release);
        self.state.notify.notify_waiters();
        self.state.notify.notify_one();
    }
    /// Snapshot active connections.
    pub async fn connections(&self) -> Vec<ConnectionSnapshot> {
        self.state
            .connections
            .read()
            .await
            .values()
            .cloned()
            .collect()
    }
    /// Return the shared native mutation authority for this runtime.
    pub fn control_state(&self) -> crate::ControlState {
        self.control.clone()
    }
    /// Forget an active connection record. The relay is closed by peer or service shutdown.
    pub async fn forget_connection(&self, id: u64) -> bool {
        self.state.connections.write().await.remove(&id).is_some()
    }
    /// Wait until listener and owned connection tasks have drained.
    pub async fn wait(&self) -> Result<(), EggchaosError> {
        let tasks = self.tasks.lock().await.take().unwrap_or_default();
        for task in tasks {
            task.await
                .map_err(|e| EggchaosError::Join(e.to_string()))??;
        }
        Ok(())
    }
}

async fn listen_loop(
    listener: TcpListener,
    proxy: ProxySpec,
    service_seed: u64,
    state: RuntimeState,
    half_close: HalfClosePolicy,
    relay_buffer: NonZeroUsize,
) -> Result<(), EggchaosError> {
    let ordinal = AtomicU64::new(1);
    let mut children = JoinSet::new();
    loop {
        tokio::select! {
            _ = state.notify.notified() => {
                if state.shutting_down.load(Ordering::Acquire) { break; }
            },
            accepted = listener.accept() => {
                let (client, peer) = accepted.map_err(|source| EggchaosError::Io { context: format!("accept proxy {}", proxy.name), source })?;
                let current = state.active.fetch_add(1, Ordering::AcqRel) + 1;
                if current > state.limits.global_connections || proxy.max_connections.is_some_and(|limit| current > limit) {
                    state.active.fetch_sub(1, Ordering::AcqRel);
                    drop(client);
                    continue;
                }
                let id = state.next_id.fetch_add(1, Ordering::AcqRel);
                let connection_key = ordinal.fetch_add(1, Ordering::AcqRel);
                state.connections.write().await.insert(id, ConnectionSnapshot { id, proxy: proxy.name.clone(), ordinal: connection_key, peer, upstream: proxy.upstream, state: ConnectionState::Connecting, connection_key, seed: service_seed ^ proxy.seed, generation: 1 });
                let cancellation = Arc::new(Notify::new());
                state.cancellations.write().await.insert(id, cancellation.clone());
                let state_child = state.clone();
                let proxy_child = proxy.clone();
                children.spawn(run_connection(client, id, connection_key, service_seed ^ proxy_child.seed, proxy_child, state_child, cancellation, half_close, relay_buffer));
            }
        }
    }
    children.shutdown().await;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_connection(
    client: TcpStream,
    id: u64,
    key: u64,
    seed: u64,
    proxy: ProxySpec,
    state: RuntimeState,
    cancellation: Arc<Notify>,
    half_close: HalfClosePolicy,
    relay_buffer: NonZeroUsize,
) {
    let result = timeout(proxy.connect_timeout, TcpStream::connect(proxy.upstream)).await;
    let Ok(Ok(upstream)) = result else {
        finish(&state, id);
        return;
    };
    if let Some(snapshot) = state.connections.write().await.get_mut(&id) {
        snapshot.state = ConnectionState::Relaying;
    }
    let client = match ChaosStream::new_live(
        client,
        proxy.downstream_policy,
        seed,
        &proxy.name,
        key,
        Direction::Downstream,
    ) {
        Ok(stream) => stream,
        Err(_) => {
            finish(&state, id);
            return;
        }
    };
    let upstream = match ChaosStream::new_live(
        upstream,
        proxy.upstream_policy,
        seed,
        &proxy.name,
        key,
        Direction::Upstream,
    ) {
        Ok(stream) => stream,
        Err(_) => {
            finish(&state, id);
            return;
        }
    };
    let options = RelayOptions {
        buffer_size: relay_buffer,
        half_close,
    };
    let relay = egress_relay::relay_with_options(client, upstream, options);
    tokio::pin!(relay);
    tokio::select! { _ = &mut relay => {}, _ = cancellation.notified() => {} }
    finish(&state, id);
}

fn finish(state: &RuntimeState, id: u64) {
    state.active.fetch_sub(1, Ordering::AcqRel);
    if let Ok(mut map) = state.connections.try_write() {
        map.remove(&id);
    }
    if let Ok(mut map) = state.cancellations.try_write() {
        map.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
        let addr = handle.bound_proxies()["echo"];
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
        handle.wait().await.unwrap();
    }
}
