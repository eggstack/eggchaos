//! Composable in-process Eggfetch `Dialer` integration.
//!
//! Eggfetch remains the authority for HTTP framing, pooling, TLS, SNI, and
//! certificate verification. This crate owns only physical-stream impairment:
//! it decorates the successfully dialed stream returned by a caller-selected
//! inner [`eggfetch_core::Dialer`] with a deterministic bidirectional fault
//! policy.
//!
//! ```text
//! inner Dialer
//!    |
//!    +-- dial(DialTarget)
//!           |
//!           +-- physical DialStream
//!                  |
//!                  +-- BidirectionalChaosStream
//!                         |
//!                         +-- EggFetch HTTP/TLS/pooling
//! ```
//!
//! The inner dialer is the sole authority for resolution, routing,
//! authentication, timeout, and connect errors. Eggchaos never performs a
//! second dial after the inner dialer succeeds and never inspects HTTP,
//! TLS, headers, or payload bytes.
//!
//! The physical connection is the deterministic fault unit: all logical
//! requests sharing one keep-alive or multiplexed `H2` connection share one
//! connection key and one chaos realization. There is no per-request
//! transport-fault identity.
//!
//! Because Eggfetch receives a type-erased stream, per-connection evidence
//! is delivered out of band through an optional [`ConnectionObserver`].
//! The adapter itself retains no connection history and spawns no
//! background tasks.
#![forbid(unsafe_code)]

use std::{
    collections::VecDeque,
    io,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use eggchaos_core::{BidirectionalChaosStream, FaultPlan, LiveBidirectionalEvidence, LivePolicy};
use eggfetch_core::{DialError, DialErrorKind, DialFuture, DialStream, DialTarget, Dialer};
use tokio::{net::lookup_host, time::timeout};

/// First physical connection ordinal assigned by a new adapter instance.
///
/// Ordinals count successfully wrapped physical connections starting here.
/// They are adapter-local and deterministic for a deterministic dial
/// sequence; they are not shared across adapter instances.
pub const INITIAL_CONNECTION_ORDINAL: u64 = 1;

/// Maximum byte length of a caller-supplied integration identity.
pub const MAX_INTEGRATION_ID_BYTES: usize = 128;

/// Maximum byte length retained from a connection-key provider failure.
pub const MAX_KEY_ERROR_BYTES: usize = 256;

/// Default capacity of [`RecordingObserver`].
pub const DEFAULT_RECORDING_CAPACITY: usize = 1024;

/// Maximum physical connect timeout accepted by [`DirectDialer`].
pub const MAX_CONNECT_TIMEOUT: Duration = Duration::from_secs(300);

/// Adapter configuration failure. Returned at configuration time, never
/// after a successful network dial.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChaosConfigError {
    /// Caller integration identity exceeds [`MAX_INTEGRATION_ID_BYTES`].
    #[error("integration identity exceeds 128 bytes")]
    IntegrationIdTooLong,
    /// Connect timeout exceeds [`MAX_CONNECT_TIMEOUT`].
    #[error("connect timeout exceeds 300 seconds")]
    ConnectTimeoutTooLong,
}

/// Connection-key provider failure. Fails the dial before any byte is
/// exposed to Eggfetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyError {
    message: String,
}

impl KeyError {
    /// Construct a bounded provider error; overlong messages are truncated
    /// to [`MAX_KEY_ERROR_BYTES`] bytes on a character boundary.
    pub fn new(message: impl Into<String>) -> Self {
        let message = message.into();
        let truncated = truncate_to_bytes(&message, MAX_KEY_ERROR_BYTES);
        Self { message: truncated }
    }

    /// Bounded failure description (no payloads, no secrets).
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "connection key provider failed: {}", self.message)
    }
}

impl std::error::Error for KeyError {}

fn truncate_to_bytes(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_owned();
    }
    let mut end = max_bytes;
    while !input.is_char_boundary(end) {
        end -= 1;
    }
    input[..end].to_owned()
}

/// Stable inputs for deterministic physical connection-key derivation.
///
/// A provider may depend on the adapter-local physical connection ordinal,
/// the logical [`DialTarget`], and the caller-configured integration
/// identity. It must not depend on logical HTTP request order, task IDs,
/// wall-clock timestamps, random UUIDs, or scheduler order.
#[derive(Debug, Clone, Copy)]
pub struct ConnectionKeyContext<'a> {
    /// Adapter-local physical connection ordinal (`1, 2, ...`).
    pub ordinal: u64,
    /// Logical dial target supplied to the inner dialer.
    pub target: &'a DialTarget,
    /// Caller integration identity (possibly empty).
    pub integration_id: &'a str,
}

/// Caller-controlled deterministic physical connection-key source.
///
/// Equal keys select equal deterministic fault-local namespaces where all
/// other derivation inputs match, so collisions are allowed only when the
/// caller explicitly selects them.
pub trait ConnectionKeyProvider: Send + Sync + 'static {
    /// Derive the connection key for one physical dial.
    ///
    /// # Errors
    ///
    /// Returns [`KeyError`] to fail the dial before bytes are exposed.
    fn connection_key(&self, ctx: &ConnectionKeyContext<'_>) -> Result<u64, KeyError>;
}

impl<T> ConnectionKeyProvider for Arc<T>
where
    T: ConnectionKeyProvider + ?Sized,
{
    fn connection_key(&self, ctx: &ConnectionKeyContext<'_>) -> Result<u64, KeyError> {
        self.as_ref().connection_key(ctx)
    }
}

/// Default provider: the connection key is the physical ordinal.
///
/// This preserves the historical direct-dialer behavior where connection
/// `n` compiles fault namespaces from key `n`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultConnectionKeyProvider;

impl ConnectionKeyProvider for DefaultConnectionKeyProvider {
    fn connection_key(&self, ctx: &ConnectionKeyContext<'_>) -> Result<u64, KeyError> {
        Ok(ctx.ordinal)
    }
}

/// Bounded transport metadata delivered per successfully wrapped dial.
#[derive(Debug, Clone)]
pub struct ConnectionReport {
    /// Adapter-local physical connection ordinal.
    pub ordinal: u64,
    /// Derived deterministic connection key (fault identity for life).
    pub connection_key: u64,
    /// Logical target host (from [`DialTarget`]; never credentials).
    pub target_host: String,
    /// Logical target port.
    pub target_port: u16,
    /// Caller integration identity.
    pub integration_id: Arc<str>,
    /// Shareable live evidence; safe to inspect after stream drop.
    pub evidence: LiveBidirectionalEvidence,
}

/// Synchronous hook invoked once per successfully wrapped physical dial.
///
/// The hook fires before the stream is handed to Eggfetch, so observer
/// failure (including a panic, which propagates) can never corrupt a
/// stream Eggfetch already owns. The hook must be non-blocking and must
/// not retain payload bytes; only bounded metadata and the shareable
/// evidence handle are provided. Abrupt stream drops after handoff are
/// representable as live counters that simply stop advancing.
pub trait ConnectionObserver: Send + Sync + 'static {
    /// Observe one wrapped physical connection.
    fn on_connection(&self, report: &ConnectionReport);
}

impl<T> ConnectionObserver for Arc<T>
where
    T: ConnectionObserver + ?Sized,
{
    fn on_connection(&self, report: &ConnectionReport) {
        self.as_ref().on_connection(report);
    }
}

/// One retained connection record: bounded metadata plus a snapshot.
#[derive(Debug, Clone)]
pub struct ConnectionRecord {
    /// Adapter-local physical connection ordinal.
    pub ordinal: u64,
    /// Derived deterministic connection key.
    pub connection_key: u64,
    /// Logical target host.
    pub target_host: String,
    /// Logical target port.
    pub target_port: u16,
    /// Caller integration identity.
    pub integration_id: Arc<str>,
    /// Live evidence handle (readable after stream drop).
    pub evidence: LiveBidirectionalEvidence,
}

/// Bounded test observer with explicit capacity and oldest-first eviction.
///
/// When full, the oldest record is dropped to admit the newest one. This
/// collector exists for tests and small harnesses; production consumers
/// should implement [`ConnectionObserver`] over their own bounded sink.
#[derive(Debug)]
pub struct RecordingObserver {
    capacity: usize,
    records: Mutex<VecDeque<ConnectionRecord>>,
}

impl RecordingObserver {
    /// Create a collector retaining at most `capacity` records.
    /// A zero capacity retains nothing but still counts dials.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            records: Mutex::new(VecDeque::new()),
        }
    }

    /// Create a collector with [`DEFAULT_RECORDING_CAPACITY`].
    pub fn with_default_capacity() -> Self {
        Self::new(DEFAULT_RECORDING_CAPACITY)
    }

    /// Number of retained records (never exceeds capacity).
    pub fn len(&self) -> usize {
        self.records.lock().expect("observer lock").len()
    }

    /// Whether no record is retained.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clone the retained records in dial order (oldest first).
    pub fn records(&self) -> Vec<ConnectionRecord> {
        self.records
            .lock()
            .expect("observer lock")
            .iter()
            .cloned()
            .collect()
    }
}

impl ConnectionObserver for RecordingObserver {
    fn on_connection(&self, report: &ConnectionReport) {
        if self.capacity == 0 {
            return;
        }
        let mut records = self.records.lock().expect("observer lock");
        while records.len() >= self.capacity {
            records.pop_front();
        }
        records.push_back(ConnectionRecord {
            ordinal: report.ordinal,
            connection_key: report.connection_key,
            target_host: report.target_host.clone(),
            target_port: report.target_port,
            integration_id: report.integration_id.clone(),
            evidence: report.evidence.clone(),
        });
    }
}

/// Direct TCP dialer used by the convenience path.
///
/// This is the only dialer that performs DNS resolution and TCP
/// connecting. It exists so the direct convenience mode composes over the
/// same single chaos wrapping path as any caller-supplied dialer.
#[derive(Debug, Clone)]
pub struct DirectDialer {
    connect_timeout: Duration,
}

impl DirectDialer {
    /// Create a direct dialer with a 10s physical connect timeout.
    pub fn new() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
        }
    }

    /// Set the bounded physical connect timeout.
    ///
    /// # Errors
    ///
    /// Returns [`ChaosConfigError`] when the timeout exceeds
    /// [`MAX_CONNECT_TIMEOUT`].
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Result<Self, ChaosConfigError> {
        if timeout > MAX_CONNECT_TIMEOUT {
            return Err(ChaosConfigError::ConnectTimeoutTooLong);
        }
        self.connect_timeout = timeout;
        Ok(self)
    }
}

impl Default for DirectDialer {
    fn default() -> Self {
        Self::new()
    }
}

impl Dialer for DirectDialer {
    fn dial(&self, target: DialTarget) -> DialFuture<'_> {
        let timeout_duration = self.connect_timeout;
        Box::pin(async move {
            let addresses = lookup_host((target.host(), target.port()))
                .await
                .map_err(|error| {
                    DialError::with_source(
                        DialErrorKind::Connection,
                        "direct route resolution failed",
                        error,
                    )
                })?;
            let mut last_error = None;
            for address in addresses {
                match timeout(timeout_duration, tokio::net::TcpStream::connect(address)).await {
                    Ok(Ok(stream)) => return Ok(Box::new(stream) as DialStream),
                    Ok(Err(error)) => last_error = Some(error),
                    Err(_) => {
                        return Err(DialError::new(
                            DialErrorKind::Timeout,
                            "direct route connect timed out",
                        ))
                    }
                }
            }
            Err(DialError::with_source(
                DialErrorKind::Connection,
                "direct route connection failed",
                last_error.unwrap_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "no resolved address")
                }),
            ))
        })
    }
}

/// Composable Eggfetch dialer: wraps an arbitrary inner dialer's physical
/// stream with a live bidirectional fault policy.
///
/// `ChaosDialer<DirectDialer>` (the default) preserves the historical
/// direct-dial convenience. `ChaosDialer<D>` for any caller-selected `D`
/// decorates that dialer's route without duplicating its connect logic.
///
/// One adapter instance owns one upstream and one downstream [`LivePolicy`]
/// authority shared by every connection it wraps. An already-open pooled
/// connection observes supported live generations at the physical stream
/// transition boundary; no reconnect is required to publish a new plan.
pub struct ChaosDialer<D = DirectDialer> {
    inner: D,
    upstream: LivePolicy,
    downstream: LivePolicy,
    proxy_name: Arc<str>,
    integration_id: Arc<str>,
    key_provider: Arc<dyn ConnectionKeyProvider>,
    observer: Option<Arc<dyn ConnectionObserver>>,
    connection_ordinal: Arc<AtomicU64>,
}

impl<D> std::fmt::Debug for ChaosDialer<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChaosDialer")
            .field("proxy_name", &self.proxy_name)
            .field("integration_id", &self.integration_id)
            .field("upstream", &self.upstream)
            .field("downstream", &self.downstream)
            .finish_non_exhaustive()
    }
}

impl<D: Clone> Clone for ChaosDialer<D> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            upstream: self.upstream.clone(),
            downstream: self.downstream.clone(),
            proxy_name: self.proxy_name.clone(),
            integration_id: self.integration_id.clone(),
            key_provider: self.key_provider.clone(),
            observer: self.observer.clone(),
            connection_ordinal: self.connection_ordinal.clone(),
        }
    }
}

impl ChaosDialer<DirectDialer> {
    /// Create a direct dialer with empty policies.
    pub fn new(run_seed: u64, proxy_name: impl Into<Arc<str>>) -> Self {
        Self::with_policies(run_seed, proxy_name, FaultPlan::empty(), FaultPlan::empty())
    }

    /// Create a direct dialer with validated initial directional plans. The
    /// seed is the namespace for both initial policy generations.
    pub fn with_policies(
        run_seed: u64,
        proxy_name: impl Into<Arc<str>>,
        upstream: FaultPlan,
        downstream: FaultPlan,
    ) -> Self {
        Self::wrap_with_policies(
            DirectDialer::new(),
            run_seed,
            proxy_name,
            upstream,
            downstream,
        )
    }

    /// Set the bounded physical connect timeout of the direct dialer.
    ///
    /// # Errors
    ///
    /// Returns [`ChaosConfigError`] when the timeout exceeds
    /// [`MAX_CONNECT_TIMEOUT`].
    pub fn with_connect_timeout(self, timeout: Duration) -> Result<Self, ChaosConfigError> {
        Ok(Self {
            inner: self.inner.with_connect_timeout(timeout)?,
            ..self
        })
    }
}

impl<D> ChaosDialer<D> {
    /// Decorate an arbitrary inner dialer with empty policies.
    pub fn wrap(inner: D, run_seed: u64, proxy_name: impl Into<Arc<str>>) -> Self {
        Self::wrap_with_policies(
            inner,
            run_seed,
            proxy_name,
            FaultPlan::empty(),
            FaultPlan::empty(),
        )
    }

    /// Decorate an arbitrary inner dialer with initial directional plans.
    pub fn wrap_with_policies(
        inner: D,
        run_seed: u64,
        proxy_name: impl Into<Arc<str>>,
        upstream: FaultPlan,
        downstream: FaultPlan,
    ) -> Self {
        Self {
            inner,
            upstream: LivePolicy::new(upstream, run_seed),
            downstream: LivePolicy::new(downstream, run_seed),
            proxy_name: proxy_name.into(),
            integration_id: Arc::from(""),
            key_provider: Arc::new(DefaultConnectionKeyProvider),
            observer: None,
            connection_ordinal: Arc::new(AtomicU64::new(INITIAL_CONNECTION_ORDINAL)),
        }
    }

    /// Set the caller integration identity mixed into key-derivation
    /// inputs and reported with every connection.
    ///
    /// # Errors
    ///
    /// Returns [`ChaosConfigError`] when the identity exceeds
    /// [`MAX_INTEGRATION_ID_BYTES`] bytes.
    pub fn with_integration_id(
        mut self,
        integration_id: impl Into<String>,
    ) -> Result<Self, ChaosConfigError> {
        let integration_id = integration_id.into();
        if integration_id.len() > MAX_INTEGRATION_ID_BYTES {
            return Err(ChaosConfigError::IntegrationIdTooLong);
        }
        self.integration_id = Arc::from(integration_id);
        Ok(self)
    }

    /// Set the deterministic connection-key provider.
    pub fn with_key_provider(mut self, provider: Arc<dyn ConnectionKeyProvider>) -> Self {
        self.key_provider = provider;
        self
    }

    /// Set the optional evidence observer invoked per wrapped dial.
    pub fn with_observer(mut self, observer: Arc<dyn ConnectionObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Access the upstream live policy.
    pub fn upstream_policy(&self) -> LivePolicy {
        self.upstream.clone()
    }

    /// Access the downstream live policy.
    pub fn downstream_policy(&self) -> LivePolicy {
        self.downstream.clone()
    }

    /// Access the inner route-authoritative dialer.
    pub fn inner(&self) -> &D {
        &self.inner
    }

    /// Publish a new upstream generation, retaining the seed namespace.
    pub fn publish_upstream(&self, plan: FaultPlan) -> Result<u64, eggchaos_core::ValidationError> {
        let namespace = self.upstream.seed_namespace();
        self.upstream
            .publish(plan, namespace)
            .map(|snapshot| snapshot.generation)
    }

    /// Publish a new downstream generation, retaining the seed namespace.
    pub fn publish_downstream(
        &self,
        plan: FaultPlan,
    ) -> Result<u64, eggchaos_core::ValidationError> {
        let namespace = self.downstream.seed_namespace();
        self.downstream
            .publish(plan, namespace)
            .map(|snapshot| snapshot.generation)
    }

    /// Wrap one successfully dialed physical stream. This is the single
    /// chaos wrapping path for both direct and composed dialers.
    fn wrap_stream(
        &self,
        stream: DialStream,
        target: &DialTarget,
    ) -> Result<DialStream, DialError> {
        let ordinal = self.connection_ordinal.fetch_add(1, Ordering::AcqRel);
        let ctx = ConnectionKeyContext {
            ordinal,
            target,
            integration_id: &self.integration_id,
        };
        let connection_key = self.key_provider.connection_key(&ctx).map_err(|error| {
            DialError::new(
                DialErrorKind::Other,
                format!("connection key provider failed: {}", error.message()),
            )
        })?;
        let wrapped = BidirectionalChaosStream::new_live(
            stream,
            self.upstream.clone(),
            self.downstream.clone(),
            &*self.proxy_name,
            connection_key,
        )
        .map_err(|error| DialError::new(DialErrorKind::Other, error.to_string()))?;
        if let Some(observer) = &self.observer {
            observer.on_connection(&ConnectionReport {
                ordinal,
                connection_key,
                target_host: target.host().to_owned(),
                target_port: target.port(),
                integration_id: self.integration_id.clone(),
                evidence: wrapped.live_evidence(),
            });
        }
        Ok(Box::new(wrapped))
    }
}

impl<D> Dialer for ChaosDialer<D>
where
    D: Dialer,
{
    fn dial(&self, target: DialTarget) -> DialFuture<'_> {
        Box::pin(async move {
            // The inner dialer owns route/connect behavior entirely: its
            // error (kind and source) passes through untouched. Eggchaos
            // never retries or redials; exactly one inner attempt happens
            // per dial call.
            let stream = self.inner.dial(target.clone()).await?;
            self.wrap_stream(stream, &target)
        })
    }
}

#[cfg(test)]
mod tests;
