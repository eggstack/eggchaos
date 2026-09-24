#![forbid(unsafe_code)]
//! Toxiproxy v2.12 compatibility adapter over canonical native state.
//!
//! The adapter holds no proxy or toxic definitions of its own. Every view is
//! derived from [`ControlState`] snapshots (native plans plus actual bound
//! listener addresses) and every mutation goes through the native control
//! authority, so compatibility presentation cannot drift from native state.
//!
//! Wire shapes, status codes, defaults, and error envelopes follow the pinned
//! v2.12.0 oracle baseline recorded in
//! `qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md`. Known
//! intentional divergences (native fixed-target address validation, proxy
//! name charset, stream-case echo, toxicity clamping, zero-valued numeric
//! coalescing) are classified in `plans/reference/toxiproxy-parity.md`.

use std::{collections::BTreeMap, num::NonZeroU64, time::Duration};

use eggchaos_core::{
    BandwidthConfig, BlackholeConfig, Direction, DisconnectConfig, FaultId, FaultKind, FaultPlan,
    FaultSpec, LatencyConfig, LimitDataConfig, Probability, SliceConfig, SlowCloseConfig,
};
use eggchaos_server::{
    ControlError, ControlState, FaultPatch, FaultUpsert, ProxyPatch, ProxySpec, ProxyView,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

/// Oracle-exact compatibility failure: JSON body `{"error","status"}` served
/// as `text/plain`, matching the pinned oracle's error content type.
#[derive(Debug, Error, Clone)]
#[error("{message}")]
pub struct CompatError {
    /// HTTP status code.
    pub status: u16,
    /// Oracle-shaped error message.
    pub message: String,
}

impl CompatError {
    fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    /// Unknown proxy, oracle message verbatim.
    pub fn proxy_not_found() -> Self {
        Self::new(404, "proxy not found")
    }

    /// Unknown toxic, oracle message verbatim.
    pub fn toxic_not_found() -> Self {
        Self::new(404, "toxic not found")
    }

    /// Duplicate proxy name, oracle message verbatim.
    pub fn proxy_exists() -> Self {
        Self::new(409, "proxy already exists")
    }

    /// Duplicate toxic name within a proxy, oracle message verbatim.
    pub fn toxic_exists() -> Self {
        Self::new(409, "toxic already exists")
    }

    /// Invalid toxic type, oracle message verbatim.
    pub fn invalid_type() -> Self {
        Self::new(400, "invalid toxic type")
    }

    /// Invalid stream, oracle message verbatim.
    pub fn invalid_stream() -> Self {
        Self::new(
            400,
            "stream was invalid, can be either upstream or downstream",
        )
    }

    /// Malformed JSON body. The prefix mirrors the oracle; the suffix is the
    /// local parse error because Go parse text is unreproducible.
    pub fn bad_body(error: impl std::fmt::Display) -> Self {
        Self::new(400, format!("bad request body: {error}"))
    }
}

/// Toxiproxy-compatible proxy JSON shape (responses).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proxy {
    /// Name.
    pub name: String,
    /// Listener.
    pub listen: String,
    /// Fixed upstream.
    pub upstream: String,
    /// Enabled state.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Toxics attached to the proxy.
    #[serde(default)]
    pub toxics: Vec<Toxic>,
}

fn default_true() -> bool {
    true
}

/// Compatibility proxy create/populate/update input. Unknown fields (such as
/// `toxics` on create, which the oracle ignores) are accepted and dropped.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyInput {
    /// Name; required on create/populate entries.
    #[serde(default)]
    pub name: Option<String>,
    /// Listener; absent means an ephemeral loopback listener.
    #[serde(default)]
    pub listen: Option<String>,
    /// Fixed upstream; required on create/populate entries.
    #[serde(default)]
    pub upstream: Option<String>,
    /// Enabled state; defaults to true.
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// Compatibility proxy update input: every field is optional.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyUpdate {
    /// Replacement listener (restart-class).
    #[serde(default)]
    pub listen: Option<String>,
    /// Replacement fixed target (restart-class).
    #[serde(default)]
    pub upstream: Option<String>,
    /// Enable or disable the listener lifecycle.
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// Toxiproxy v2.12 toxic JSON shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Toxic {
    /// Optional stable name; absent defaults to `<type>_<stream>`.
    #[serde(default)]
    pub name: Option<String>,
    /// Toxic type.
    pub r#type: String,
    /// Direction; Toxiproxy defaults to downstream.
    #[serde(default = "default_stream")]
    pub stream: String,
    /// Activation probability; defaults to 1.0.
    #[serde(default = "default_toxicity")]
    pub toxicity: f64,
    /// Typed toxic attributes; omitted attributes default to zero.
    #[serde(default)]
    pub attributes: ToxicAttributes,
}

fn default_stream() -> String {
    "downstream".into()
}
fn default_toxicity() -> f64 {
    1.0
}

/// Compatibility toxic update input. `type` and `stream` are accepted and
/// ignored, matching the oracle: updates only change toxicity and the
/// same-type attributes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToxicUpdate {
    /// Ignored; toxic type is immutable.
    #[serde(default)]
    pub r#type: Option<String>,
    /// Ignored; toxic direction is immutable.
    #[serde(default)]
    pub stream: Option<String>,
    /// Replacement activation probability.
    #[serde(default)]
    pub toxicity: Option<f64>,
    /// Replacement attributes; only keys belonging to the toxic's own type
    /// are applied.
    #[serde(default)]
    pub attributes: Option<ToxicAttributes>,
}

/// Bounded v2.12 toxic attribute superset. All fields are optional on input;
/// omitted attributes default to zero per the oracle baseline.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToxicAttributes {
    /// Milliseconds of latency.
    pub latency: Option<u64>,
    /// Milliseconds of jitter.
    pub jitter: Option<u64>,
    /// Bandwidth rate in oracle units (KiB/s).
    pub rate: Option<u64>,
    /// Slice average size.
    pub average_size: Option<u64>,
    /// Slice variation.
    pub size_variation: Option<u64>,
    /// Slice delay in microseconds; slow_close delay in milliseconds.
    pub delay: Option<u64>,
    /// Timeout in milliseconds.
    pub timeout: Option<u64>,
    /// Limit bytes.
    pub bytes: Option<u64>,
}

/// Compatibility translation failures.
#[derive(Debug, Error)]
pub enum CompatibilityError {
    /// Toxic value is invalid.
    #[error("invalid toxic: {0}")]
    Invalid(String),
    /// Unsupported toxic type.
    #[error("unsupported v2.12 toxic: {0}")]
    Unsupported(String),
    /// Address is not a fixed TCP socket address.
    #[error("invalid fixed target address: {0}")]
    Address(String),
}

impl From<CompatibilityError> for CompatError {
    fn from(error: CompatibilityError) -> Self {
        match error {
            CompatibilityError::Invalid(message) => CompatError::new(400, message),
            CompatibilityError::Unsupported(_) => CompatError::invalid_type(),
            CompatibilityError::Address(message) => CompatError::new(400, message),
        }
    }
}

/// Normalize a compat stream spelling. Only `upstream`/`downstream` (any
/// ASCII case, matching the oracle) are accepted.
fn parse_stream(stream: &str) -> Result<Direction, CompatError> {
    if stream.eq_ignore_ascii_case("upstream") {
        Ok(Direction::Upstream)
    } else if stream.eq_ignore_ascii_case("downstream") {
        Ok(Direction::Downstream)
    } else {
        Err(CompatError::invalid_stream())
    }
}

/// Clamp oracle toxicity into the native probability range. The oracle stores
/// out-of-range values verbatim, which native probabilities cannot represent;
/// clamping preserves runtime behavior (always/never applies) while keeping a
/// representable value. Recorded as intent-compatible in the parity matrix.
fn clamp_toxicity(toxicity: f64) -> Result<f64, CompatError> {
    if toxicity.is_finite() {
        Ok(toxicity.clamp(0.0, 1.0))
    } else {
        Err(CompatError::new(400, "toxicity must be finite"))
    }
}

/// Build a native fault kind from a v2.12 toxic type plus its attributes.
/// Zero-valued numerics that native `NonZero` bounds cannot represent are
/// coalesced to the minimal representable value (documented divergence):
/// bandwidth rate 0 -> 1 KiB/s, slicer average_size 0 -> 1, limit_data bytes
/// 0 -> 1. Timeout 0 means indefinite blackhole (`close_after: None`),
/// matching the oracle.
fn kind_from_attrs(kind: &str, attrs: &ToxicAttributes) -> Result<FaultKind, CompatError> {
    match kind {
        "latency" => Ok(FaultKind::Latency(LatencyConfig {
            delay: Duration::from_millis(attrs.latency.unwrap_or(0)),
            jitter: Duration::from_millis(attrs.jitter.unwrap_or(0)),
            max_buffer_bytes: NonZeroU64::new(64 * 1024).unwrap(),
        })),
        "bandwidth" => Ok(FaultKind::Bandwidth(BandwidthConfig {
            bytes_per_second: NonZeroU64::new(attrs.rate.unwrap_or(1).max(1).saturating_mul(1024))
                .expect("rate is non-zero after max(1)"),
            burst_bytes: NonZeroU64::new(64 * 1024).unwrap(),
        })),
        "timeout" => Ok(FaultKind::Blackhole(BlackholeConfig {
            close_after: attrs
                .timeout
                .filter(|timeout| *timeout > 0)
                .map(Duration::from_millis),
        })),
        "slow_close" => Ok(FaultKind::SlowClose(SlowCloseConfig {
            delay: Duration::from_millis(attrs.delay.unwrap_or(0)),
        })),
        "reset_peer" => Ok(FaultKind::Disconnect(DisconnectConfig {
            after: Duration::from_millis(attrs.timeout.unwrap_or(0)),
            hard_reset: true,
        })),
        "slicer" => Ok(FaultKind::Slice(SliceConfig {
            average_size: NonZeroU64::new(attrs.average_size.unwrap_or(1).max(1))
                .expect("average_size is non-zero after max(1)"),
            variation: attrs.size_variation.unwrap_or(0),
            delay: Duration::from_micros(attrs.delay.unwrap_or(0)),
        })),
        "limit_data" => Ok(FaultKind::LimitData(LimitDataConfig {
            bytes: NonZeroU64::new(attrs.bytes.unwrap_or(1).max(1))
                .expect("bytes is non-zero after max(1)"),
        })),
        _ => Err(CompatError::invalid_type()),
    }
}

/// Split a native fault back into its v2.12 toxic type plus fully populated
/// attributes (zero-filled, matching oracle echo shape).
fn attrs_from_kind(kind: &FaultKind) -> (&'static str, ToxicAttributes) {
    match *kind {
        FaultKind::Latency(config) => (
            "latency",
            ToxicAttributes {
                latency: Some(config.delay.as_millis().min(u64::MAX as u128) as u64),
                jitter: Some(config.jitter.as_millis().min(u64::MAX as u128) as u64),
                ..ToxicAttributes::default()
            },
        ),
        FaultKind::Bandwidth(config) => (
            "bandwidth",
            ToxicAttributes {
                rate: Some(config.bytes_per_second.get() / 1024),
                ..ToxicAttributes::default()
            },
        ),
        FaultKind::Blackhole(config) => (
            "timeout",
            ToxicAttributes {
                timeout: Some(
                    config
                        .close_after
                        .map(|after| after.as_millis().min(u64::MAX as u128) as u64)
                        .unwrap_or(0),
                ),
                ..ToxicAttributes::default()
            },
        ),
        FaultKind::LimitData(config) => (
            "limit_data",
            ToxicAttributes {
                bytes: Some(config.bytes.get()),
                ..ToxicAttributes::default()
            },
        ),
        FaultKind::SlowClose(config) => (
            "slow_close",
            ToxicAttributes {
                delay: Some(config.delay.as_millis().min(u64::MAX as u128) as u64),
                ..ToxicAttributes::default()
            },
        ),
        FaultKind::Slice(config) => (
            "slicer",
            ToxicAttributes {
                average_size: Some(config.average_size.get()),
                size_variation: Some(config.variation),
                delay: Some(config.delay.as_micros().min(u64::MAX as u128) as u64),
                ..ToxicAttributes::default()
            },
        ),
        FaultKind::Disconnect(config) => (
            "reset_peer",
            ToxicAttributes {
                timeout: Some(config.after.as_millis().min(u64::MAX as u128) as u64),
                ..ToxicAttributes::default()
            },
        ),
    }
}

/// Render the full per-type attribute object with all keys present
/// (zero-filled), matching oracle echo shape.
fn attributes_object(kind: &str, attrs: &ToxicAttributes) -> Value {
    let get = |value: Option<u64>| value.unwrap_or(0);
    match kind {
        "latency" => json!({"latency": get(attrs.latency), "jitter": get(attrs.jitter)}),
        "bandwidth" => json!({"rate": get(attrs.rate)}),
        "slow_close" => json!({"delay": get(attrs.delay)}),
        "timeout" | "reset_peer" => json!({"timeout": get(attrs.timeout)}),
        "slicer" => {
            json!({"average_size": get(attrs.average_size), "size_variation": get(attrs.size_variation), "delay": get(attrs.delay)})
        }
        "limit_data" => json!({"bytes": get(attrs.bytes)}),
        _ => json!({}),
    }
}

/// Render one native fault as a v2.12 toxic object. Stream echoes lowercase;
/// the oracle preserves exotic input case, which cannot round-trip through
/// the native direction (recorded divergence).
pub fn fault_to_toxic(direction: Direction, fault: &FaultSpec) -> Value {
    let (kind, attrs) = attrs_from_kind(&fault.kind);
    json!({
        "name": fault.id.as_str(),
        "type": kind,
        "stream": direction.as_str(),
        "toxicity": fault.probability.get(),
        "attributes": attributes_object(kind, &attrs),
    })
}

/// Merge a toxic update's attributes over the toxic's own type: only keys
/// belonging to the existing type are applied, matching the oracle (a
/// cross-type attribute payload leaves the toxic unchanged).
fn merge_attributes(
    kind: &str,
    base: &ToxicAttributes,
    patch: &ToxicAttributes,
) -> ToxicAttributes {
    let pick = |current: Option<u64>, incoming: Option<u64>| incoming.or(current);
    match kind {
        "latency" => ToxicAttributes {
            latency: pick(base.latency, patch.latency),
            jitter: pick(base.jitter, patch.jitter),
            ..ToxicAttributes::default()
        },
        "bandwidth" => ToxicAttributes {
            rate: pick(base.rate, patch.rate),
            ..ToxicAttributes::default()
        },
        "slow_close" => ToxicAttributes {
            delay: pick(base.delay, patch.delay),
            ..ToxicAttributes::default()
        },
        "timeout" | "reset_peer" => ToxicAttributes {
            timeout: pick(base.timeout, patch.timeout),
            ..ToxicAttributes::default()
        },
        "slicer" => ToxicAttributes {
            average_size: pick(base.average_size, patch.average_size),
            size_variation: pick(base.size_variation, patch.size_variation),
            delay: pick(base.delay, patch.delay),
            ..ToxicAttributes::default()
        },
        "limit_data" => ToxicAttributes {
            bytes: pick(base.bytes, patch.bytes),
            ..ToxicAttributes::default()
        },
        _ => ToxicAttributes::default(),
    }
}

impl Toxic {
    /// Translate one toxic into a native fault in the declared stream.
    /// Stream is validated before type (oracle precedence); the name
    /// defaults to `<type>_<stream>`; toxicity is clamped into [0, 1].
    pub fn to_fault(&self) -> Result<(Direction, FaultSpec), CompatibilityError> {
        let direction = if self.stream.eq_ignore_ascii_case("upstream") {
            Direction::Upstream
        } else if self.stream.eq_ignore_ascii_case("downstream") {
            Direction::Downstream
        } else {
            return Err(CompatibilityError::Invalid(
                "stream was invalid, can be either upstream or downstream".into(),
            ));
        };
        if !matches!(
            self.r#type.as_str(),
            "latency"
                | "bandwidth"
                | "timeout"
                | "slow_close"
                | "reset_peer"
                | "slicer"
                | "limit_data"
        ) {
            return Err(CompatibilityError::Unsupported(self.r#type.clone()));
        }
        let kind = kind_from_attrs(&self.r#type, &self.attributes)
            .map_err(|error| CompatibilityError::Invalid(error.message))?;
        let name = self
            .name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("{}_{}", self.r#type, direction.as_str()));
        let id =
            FaultId::new(name).map_err(|error| CompatibilityError::Invalid(error.to_string()))?;
        let toxicity = clamp_toxicity(self.toxicity)
            .map_err(|error| CompatibilityError::Invalid(error.message))?;
        let probability = Probability::new(toxicity)
            .map_err(|error| CompatibilityError::Invalid(error.to_string()))?;
        Ok((
            direction,
            FaultSpec {
                id,
                probability,
                kind,
            },
        ))
    }
}

/// Translate a Toxiproxy proxy into the native fixed-target model. Toxics in
/// the input are translated (create bodies may carry them even though the
/// oracle ignores them on create); use empty toxics for oracle-exact create.
pub fn translate_proxy(proxy: &Proxy) -> Result<ProxySpec, CompatibilityError> {
    let listen = proxy
        .listen
        .parse()
        .map_err(|e: std::net::AddrParseError| CompatibilityError::Address(e.to_string()))?;
    let upstream = proxy
        .upstream
        .parse()
        .map_err(|e: std::net::AddrParseError| CompatibilityError::Address(e.to_string()))?;
    let mut native = ProxySpec::new(proxy.name.clone(), listen, upstream);
    native.enabled = proxy.enabled;
    let mut up = Vec::new();
    let mut down = Vec::new();
    for toxic in &proxy.toxics {
        let (direction, fault) = toxic.to_fault()?;
        if direction == Direction::Upstream {
            up.push(fault);
        } else {
            down.push(fault);
        }
    }
    native.upstream_faults =
        FaultPlan::new(up).map_err(|e| CompatibilityError::Invalid(e.to_string()))?;
    native.downstream_faults =
        FaultPlan::new(down).map_err(|e| CompatibilityError::Invalid(e.to_string()))?;
    Ok(native)
}

/// Render a canonical proxy view as the oracle-shaped proxy object: actual
/// bound address when running (resolving port 0), configured listener
/// otherwise, plus an empty `Logger` and reverse-translated toxics.
pub fn proxy_json(view: &ProxyView) -> Value {
    let listen = view.bound_addr.unwrap_or(view.listen);
    let mut toxics = Vec::new();
    for fault in view.upstream_faults.faults() {
        toxics.push(fault_to_toxic(Direction::Upstream, fault));
    }
    for fault in view.downstream_faults.faults() {
        toxics.push(fault_to_toxic(Direction::Downstream, fault));
    }
    json!({
        "name": view.name,
        "listen": listen.to_string(),
        "upstream": view.upstream.to_string(),
        "enabled": view.enabled,
        "Logger": {},
        "toxics": toxics,
    })
}

/// Native mutation authority facade for compatibility clients. Holds no proxy
/// or toxic definitions: reads derive from [`ControlState`] snapshots and
/// writes go through the native control authority.
#[derive(Clone)]
pub struct ToxiproxyAdapter {
    state: ControlState,
}

impl ToxiproxyAdapter {
    /// Create a compatibility facade.
    pub fn new(state: ControlState) -> Self {
        Self { state }
    }

    /// Borrow the underlying native authority.
    pub fn control_state(&self) -> &ControlState {
        &self.state
    }

    /// List canonical proxy views.
    pub async fn list_views(&self) -> Vec<ProxyView> {
        self.state.list().await
    }

    /// List oracle-shaped proxies keyed by name.
    pub async fn list_json(&self) -> BTreeMap<String, Value> {
        self.state
            .list()
            .await
            .into_iter()
            .map(|view| (view.name.clone(), proxy_json(&view)))
            .collect()
    }

    /// Get one oracle-shaped proxy.
    pub async fn proxy_json(&self, name: &str) -> Option<Value> {
        self.state.get(name).await.map(|view| proxy_json(&view))
    }

    /// Resolve `name`/`listen`/`upstream` for create/populate entries.
    /// Missing listen means an ephemeral loopback listener (the oracle binds
    /// an ephemeral wildcard port; loopback keeps the native default).
    fn resolve_addrs(
        input: &ProxyInput,
    ) -> Result<(String, std::net::SocketAddr, std::net::SocketAddr), CompatError> {
        let name = input
            .name
            .clone()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| CompatError::new(400, "missing required field: name"))?;
        let upstream = input
            .upstream
            .clone()
            .filter(|upstream| !upstream.is_empty())
            .ok_or_else(|| CompatError::new(400, "missing required field: upstream"))?;
        // Native fixed-target invariant: upstream must be a socket address.
        // The oracle stores arbitrary strings; this is a recorded divergence.
        let upstream_addr: std::net::SocketAddr = upstream
            .parse()
            .map_err(|error: std::net::AddrParseError| CompatError::new(400, error.to_string()))?;
        let listen_addr: std::net::SocketAddr = match input.listen.clone().filter(|l| !l.is_empty())
        {
            Some(listen) => listen.parse().map_err(|error: std::net::AddrParseError| {
                CompatError::new(500, error.to_string())
            })?,
            None => "127.0.0.1:0"
                .parse()
                .expect("loopback ephemeral addr parses"),
        };
        Ok((name, listen_addr, upstream_addr))
    }

    /// Create a proxy through the native authority. Disabled creates import
    /// the definition without binding, matching the oracle.
    pub async fn create(&self, input: ProxyInput) -> Result<Value, CompatError> {
        let (name, listen, upstream) = Self::resolve_addrs(&input)?;
        let enabled = input.enabled.unwrap_or(true);
        let mut spec = ProxySpec::new(name.clone(), listen, upstream);
        spec.enabled = enabled;
        if enabled {
            match self.state.create_proxy(spec).await {
                Ok((view, _)) => Ok(proxy_json(&view)),
                Err(ControlError::Conflict(_)) => Err(CompatError::proxy_exists()),
                Err(ControlError::BindFailed { reason, .. }) => Err(CompatError::new(
                    500,
                    format!("listen tcp {listen}: bind: {reason}"),
                )),
                Err(error) => Err(CompatError::new(400, error.to_string())),
            }
        } else {
            match self.state.import_definition(spec).await {
                Ok(_) => self
                    .proxy_json(&name)
                    .await
                    .ok_or_else(CompatError::proxy_not_found),
                Err(ControlError::Conflict(_)) => Err(CompatError::proxy_exists()),
                Err(error) => Err(CompatError::new(400, error.to_string())),
            }
        }
    }

    /// Update listen/upstream/enabled through native restart-class machinery.
    /// Address parse failures report 500, matching the oracle's update path.
    pub async fn update(&self, name: &str, patch: ProxyUpdate) -> Result<Value, CompatError> {
        if self.state.get(name).await.is_none() {
            return Err(CompatError::proxy_not_found());
        }
        if let Some(enabled) = patch.enabled {
            self.state
                .set_enabled(name, enabled)
                .await
                .map_err(Self::map_update_error)?;
        }
        if patch.listen.is_some() || patch.upstream.is_some() {
            let listen = patch
                .listen
                .map(|listen| {
                    listen.parse::<std::net::SocketAddr>().map_err(
                        |error: std::net::AddrParseError| CompatError::new(500, error.to_string()),
                    )
                })
                .transpose()?;
            let upstream = patch
                .upstream
                .map(|upstream| {
                    upstream.parse::<std::net::SocketAddr>().map_err(
                        |error: std::net::AddrParseError| CompatError::new(500, error.to_string()),
                    )
                })
                .transpose()?;
            self.state
                .update_proxy(
                    name,
                    ProxyPatch {
                        listen,
                        upstream,
                        enabled: None,
                        max_connections: None,
                        connect_timeout_ms: None,
                    },
                )
                .await
                .map_err(Self::map_update_error)?;
        }
        self.proxy_json(name)
            .await
            .ok_or_else(CompatError::proxy_not_found)
    }

    fn map_update_error(error: ControlError) -> CompatError {
        match error {
            ControlError::NotFound(_) => CompatError::proxy_not_found(),
            ControlError::RestartFailed { reason, .. } => CompatError::new(500, reason),
            ControlError::BindFailed { reason, .. } => CompatError::new(500, reason),
            ControlError::Invalid(message) => CompatError::new(400, message),
            ControlError::Conflict(message) => CompatError::new(409, message),
        }
    }

    /// Delete a proxy through the native authority.
    pub async fn delete(&self, name: &str) -> Result<(), CompatError> {
        match self.state.delete_proxy(name).await {
            Ok(_) => Ok(()),
            Err(ControlError::NotFound(_)) => Err(CompatError::proxy_not_found()),
            Err(error) => Err(CompatError::new(400, error.to_string())),
        }
    }

    /// Populate proxies with oracle semantics: existing proxies whose
    /// listen+upstream match are returned untouched; entries with changed
    /// addresses are deleted and recreated (dropping toxics); unknown names
    /// are created; entries that fail to bind are skipped. Always 201.
    /// A missing entry name aborts with the oracle-exact 400 shape
    /// (1-based entry index, plus `"proxies":null`).
    pub async fn populate(&self, inputs: Vec<ProxyInput>) -> Result<Value, CompatError> {
        let mut rendered = Vec::with_capacity(inputs.len());
        for (index, input) in inputs.iter().enumerate() {
            if input.name.clone().filter(|n| !n.is_empty()).is_none() {
                return Err(CompatError {
                    status: 400,
                    message: format!("missing required field: name at proxy {}", index + 1),
                });
            }
            match self.populate_entry(input).await {
                Some(value) => rendered.push(value),
                None => continue,
            }
        }
        Ok(json!({ "proxies": Value::Array(rendered) }))
    }

    /// Populate one entry; `None` means the entry failed to bind and is
    /// skipped, matching the oracle.
    async fn populate_entry(&self, input: &ProxyInput) -> Option<Value> {
        let (name, listen, upstream) = Self::resolve_addrs(input).ok()?;
        let enabled = input.enabled.unwrap_or(true);
        if let Some(view) = self.state.get(&name).await {
            if view.listen == listen && view.upstream == upstream {
                return Some(proxy_json(&view));
            }
            // Changed addresses: delete and recreate (toxics drop by
            // construction), matching observed oracle replace behavior.
            let _ = self.state.delete_proxy(&name).await;
        }
        let mut spec = ProxySpec::new(name.clone(), listen, upstream);
        spec.enabled = enabled;
        if enabled {
            match self.state.create_proxy(spec).await {
                Ok((view, _)) => Some(proxy_json(&view)),
                Err(_) => None,
            }
        } else {
            match self.state.import_definition(spec).await {
                Ok(_) => self.proxy_json(&name).await,
                Err(_) => None,
            }
        }
    }

    /// Add a toxic through native fault CRUD. Names are unique proxy-wide
    /// (both directions); the name defaults to `<type>_<stream>`.
    pub async fn add_toxic(&self, proxy: &str, toxic: Toxic) -> Result<Value, CompatError> {
        if self.state.get(proxy).await.is_none() {
            return Err(CompatError::proxy_not_found());
        }
        let direction = parse_stream(&toxic.stream)?;
        let kind = kind_from_attrs(&toxic.r#type, &toxic.attributes)?;
        // Empty-string names auto-generate like absent names: the pinned Go
        // client serializes an unset name as `""`, not null.
        let name = toxic
            .name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("{}_{}", toxic.r#type, direction.as_str()));
        if self.state.get_fault(proxy, &name).await.is_some() {
            return Err(CompatError::toxic_exists());
        }
        let id =
            FaultId::new(name.clone()).map_err(|error| CompatError::new(400, error.to_string()))?;
        let probability = Probability::new(clamp_toxicity(toxic.toxicity)?)
            .map_err(|error| CompatError::new(400, error.to_string()))?;
        match self
            .state
            .add_fault(
                proxy,
                FaultUpsert {
                    direction,
                    id: id.as_str().to_owned(),
                    probability: probability.get(),
                    kind,
                },
            )
            .await
        {
            Ok((direction, spec, _)) => Ok(fault_to_toxic(direction, &spec)),
            Err(ControlError::NotFound(_)) => Err(CompatError::proxy_not_found()),
            Err(ControlError::Conflict(_)) => Err(CompatError::toxic_exists()),
            Err(ControlError::Invalid(message)) => Err(CompatError::new(400, message)),
            Err(error) => Err(CompatError::new(400, error.to_string())),
        }
    }

    /// List toxics from live native snapshots (upstream faults then
    /// downstream faults; differential comparison sorts by name).
    pub async fn list_toxics(&self, proxy: &str) -> Result<Vec<Value>, CompatError> {
        let Some((upstream, downstream)) = self.state.list_faults(proxy).await else {
            return Err(CompatError::proxy_not_found());
        };
        let mut toxics = Vec::with_capacity(upstream.len() + downstream.len());
        for fault in &upstream {
            toxics.push(fault_to_toxic(Direction::Upstream, fault));
        }
        for fault in &downstream {
            toxics.push(fault_to_toxic(Direction::Downstream, fault));
        }
        Ok(toxics)
    }

    /// Get one toxic from live native snapshots.
    pub async fn get_toxic(&self, proxy: &str, name: &str) -> Result<Value, CompatError> {
        match self.state.get_fault(proxy, name).await {
            Some((direction, spec)) => Ok(fault_to_toxic(direction, &spec)),
            None => {
                if self.state.get(proxy).await.is_none() {
                    Err(CompatError::proxy_not_found())
                } else {
                    Err(CompatError::toxic_not_found())
                }
            }
        }
    }

    /// Update toxicity and same-type attributes; type and stream are
    /// immutable and ignored, matching the oracle.
    pub async fn update_toxic(
        &self,
        proxy: &str,
        name: &str,
        patch: ToxicUpdate,
    ) -> Result<Value, CompatError> {
        let Some((_direction, existing)) = self.state.get_fault(proxy, name).await else {
            if self.state.get(proxy).await.is_none() {
                return Err(CompatError::proxy_not_found());
            }
            return Err(CompatError::toxic_not_found());
        };
        let (kind_name, base_attrs) = attrs_from_kind(&existing.kind);
        let merged = match patch.attributes {
            Some(attrs) => merge_attributes(kind_name, &base_attrs, &attrs),
            None => base_attrs,
        };
        let kind = kind_from_attrs(kind_name, &merged)?;
        let probability = match patch.toxicity {
            Some(toxicity) => clamp_toxicity(toxicity)?,
            None => existing.probability.get(),
        };
        match self
            .state
            .update_fault(
                proxy,
                name,
                FaultPatch {
                    probability: Some(probability),
                    kind: Some(kind),
                },
            )
            .await
        {
            Ok((direction, spec, _)) => Ok(fault_to_toxic(direction, &spec)),
            Err(ControlError::NotFound(_)) => Err(CompatError::toxic_not_found()),
            Err(ControlError::Invalid(message)) => Err(CompatError::new(400, message)),
            Err(error) => Err(CompatError::new(400, error.to_string())),
        }
    }

    /// Remove a toxic through native fault CRUD.
    pub async fn remove_toxic(&self, proxy: &str, name: &str) -> Result<(), CompatError> {
        match self.state.remove_fault(proxy, name).await {
            Ok(_) => Ok(()),
            Err(ControlError::NotFound(_)) => {
                if self.state.get(proxy).await.is_none() {
                    Err(CompatError::proxy_not_found())
                } else {
                    Err(CompatError::toxic_not_found())
                }
            }
            Err(error) => Err(CompatError::new(400, error.to_string())),
        }
    }

    /// Reset through the native authority: re-enable every proxy and clear
    /// all fault plans. Always reports success once committed.
    pub async fn reset(&self) -> Result<(), CompatError> {
        self.state
            .reset()
            .await
            .map(|_| ())
            .map_err(|error| CompatError::new(500, error.to_string()))
    }

    /// Return the oracle-exact version identifier. Eggchaos identity belongs
    /// in native surfaces, not in this compatibility field.
    pub fn version() -> &'static str {
        "2.12.0"
    }
}

/// A v2.12-compatible HTTP listener backed by [`ToxiproxyAdapter`].
pub struct ToxiproxyHttp;

/// Handle for a compatibility listener.
pub struct ToxiproxyHttpHandle {
    local_addr: std::net::SocketAddr,
    server: Option<eggserve_server::ServerHandle>,
}

impl ToxiproxyHttpHandle {
    /// Return the actual bound address.
    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.local_addr
    }
    /// Request shutdown.
    pub fn shutdown(&self) {
        if let Some(server) = &self.server {
            server.shutdown();
        }
    }
    /// Wait for shutdown.
    pub async fn wait(mut self) {
        if let Some(server) = self.server.take() {
            server.wait().await;
        }
    }
}

impl ToxiproxyHttp {
    /// Start the compatibility route family on a loopback-by-default listener.
    pub async fn start(
        bind: std::net::SocketAddr,
        adapter: ToxiproxyAdapter,
    ) -> Result<ToxiproxyHttpHandle, CompatError> {
        let listener = tokio::net::TcpListener::bind(bind)
            .await
            .map_err(|error| CompatError::new(500, error.to_string()))?;
        let local_addr = listener
            .local_addr()
            .map_err(|error| CompatError::new(500, error.to_string()))?;
        let runtime = eggserve_server::RuntimeConfig {
            max_request_body_bytes: 1024 * 1024,
            max_connections: 128,
            ..eggserve_server::RuntimeConfig::default()
        };
        let service = eggserve_server::service_fn_with_policy(
            move |request: eggserve_primitives::Request| {
                let adapter = adapter.clone();
                async move { compatibility_request(request, adapter).await }
            },
            eggserve_primitives::RequestBodyPolicy::Buffer {
                max_bytes: 1024 * 1024,
            },
        );
        let server = eggserve_server::Server::builder()
            .runtime(runtime)
            .from_listener(listener)
            .build()
            .map_err(|error| CompatError::new(500, error.to_string()))?;
        let handle = server
            .start_with_service(service)
            .await
            .map_err(|error| CompatError::new(500, error.to_string()))?;
        Ok(ToxiproxyHttpHandle {
            local_addr,
            server: Some(handle),
        })
    }
}

/// Map a numeric status into the primitive status code.
fn status(code: u16) -> eggserve_primitives::StatusCode {
    match code {
        200 => eggserve_primitives::StatusCode::OK,
        201 => eggserve_primitives::StatusCode::CREATED,
        204 => eggserve_primitives::StatusCode::NO_CONTENT,
        400 => eggserve_primitives::StatusCode::BAD_REQUEST,
        404 => eggserve_primitives::StatusCode::NOT_FOUND,
        500 => eggserve_primitives::StatusCode::INTERNAL_SERVER_ERROR,
        other => eggserve_primitives::StatusCode::new(other)
            .unwrap_or(eggserve_primitives::StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// Success JSON with `application/json`.
fn json_value(code: u16, value: &Value) -> eggserve_primitives::Response {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    eggserve_primitives::Response::builder()
        .status(status(code))
        .header("content-type", "application/json")
        .unwrap()
        .body(eggserve_primitives::ResponseBody::Bytes(body))
        .unwrap()
}

/// Compatibility error: JSON `{"error","status"}` body served as
/// `text/plain`, matching the oracle's error content type.
fn error_response(error: CompatError) -> eggserve_primitives::Response {
    let body = serde_json::to_vec(&json!({"error": error.message, "status": error.status}))
        .unwrap_or_else(|_| b"{}".to_vec());
    eggserve_primitives::Response::builder()
        .status(status(error.status))
        .header("content-type", "text/plain; charset=utf-8")
        .unwrap()
        .body(eggserve_primitives::ResponseBody::Bytes(body))
        .unwrap()
}

/// Empty success (204/reset/delete paths).
fn empty_response(code: u16) -> eggserve_primitives::Response {
    eggserve_primitives::Response::builder()
        .status(status(code))
        .body(eggserve_primitives::ResponseBody::Bytes(Vec::new()))
        .unwrap()
}

/// Unknown routes: plain-text `404 page not found`, oracle-verbatim.
fn unknown_route() -> eggserve_primitives::Response {
    eggserve_primitives::Response::builder()
        .status(status(404))
        .header("content-type", "text/plain; charset=utf-8")
        .unwrap()
        .body(eggserve_primitives::ResponseBody::Bytes(
            b"404 page not found\n".to_vec(),
        ))
        .unwrap()
}

fn parse_json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, CompatError> {
    serde_json::from_slice(bytes).map_err(CompatError::bad_body)
}

async fn compatibility_request(
    request: eggserve_primitives::Request,
    adapter: ToxiproxyAdapter,
) -> Result<eggserve_primitives::Response, eggserve_server::ServiceError> {
    let (head, body, _) = request.into_parts();
    let method = head.method().as_str().to_owned();
    let path = head.target().path().to_owned();
    let bytes = body
        .read_all()
        .await
        .map_err(|e| eggserve_server::ServiceError::rejected(413, e.to_string()))?;
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    let response = match (method.as_str(), segments.as_slice()) {
        ("GET", ["version"]) => {
            let body = serde_json::to_vec(&json!({"version": ToxiproxyAdapter::version()}))
                .unwrap_or_else(|_| b"{}".to_vec());
            eggserve_primitives::Response::builder()
                .status(status(200))
                .header("content-type", "application/json;charset=utf-8")
                .unwrap()
                .body(eggserve_primitives::ResponseBody::Bytes(body))
                .unwrap()
        }
        ("GET", ["proxies"]) => json_value(
            200,
            &Value::Object(
                adapter
                    .list_json()
                    .await
                    .into_iter()
                    .collect::<serde_json::Map<String, Value>>(),
            ),
        ),
        ("POST", ["proxies"]) => {
            let input: ProxyInput = match parse_json(&bytes) {
                Ok(input) => input,
                Err(error) => return Ok(error_response(error)),
            };
            match adapter.create(input).await {
                Ok(value) => json_value(201, &value),
                Err(error) => error_response(error),
            }
        }
        ("POST", ["populate"]) => {
            if bytes.iter().all(|byte| byte.is_ascii_whitespace()) {
                return Ok(error_response(CompatError::bad_body("EOF")));
            }
            let inputs: Vec<ProxyInput> = match parse_json(&bytes) {
                Ok(inputs) => inputs,
                Err(error) => return Ok(error_response(error)),
            };
            if inputs.is_empty() {
                return Ok(json_value(201, &json!({"proxies": Value::Null})));
            }
            match adapter.populate(inputs).await {
                Ok(value) => json_value(201, &value),
                Err(error) => {
                    if error.status == 400 && error.message.contains("at proxy") {
                        // Oracle-exact missing-name shape carries "proxies":null.
                        json_value(
                            400,
                            &json!({"error": error.message, "status": 400, "proxies": Value::Null}),
                        )
                    } else {
                        error_response(error)
                    }
                }
            }
        }
        ("GET", ["proxies", name]) => match adapter.proxy_json(name).await {
            Some(value) => json_value(200, &value),
            None => error_response(CompatError::proxy_not_found()),
        },
        ("POST", ["proxies", name]) | ("PATCH", ["proxies", name]) => {
            let patch: ProxyUpdate = match parse_json(&bytes) {
                Ok(patch) => patch,
                Err(error) => return Ok(error_response(error)),
            };
            match adapter.update(name, patch).await {
                Ok(value) => json_value(200, &value),
                Err(error) => error_response(error),
            }
        }
        ("DELETE", ["proxies", name]) => match adapter.delete(name).await {
            Ok(()) => empty_response(204),
            Err(error) => error_response(error),
        },
        ("GET", ["proxies", proxy, "toxics"]) => match adapter.list_toxics(proxy).await {
            Ok(toxics) => json_value(200, &Value::Array(toxics)),
            Err(error) => error_response(error),
        },
        ("POST", ["proxies", proxy, "toxics"]) => {
            let toxic: Toxic = match parse_json(&bytes) {
                Ok(toxic) => toxic,
                Err(error) => return Ok(error_response(error)),
            };
            match adapter.add_toxic(proxy, toxic).await {
                Ok(value) => json_value(200, &value),
                Err(error) => error_response(error),
            }
        }
        ("GET", ["proxies", proxy, "toxics", name]) => match adapter.get_toxic(proxy, name).await {
            Ok(value) => json_value(200, &value),
            Err(error) => error_response(error),
        },
        ("POST", ["proxies", proxy, "toxics", name])
        | ("PATCH", ["proxies", proxy, "toxics", name]) => {
            let patch: ToxicUpdate = match parse_json(&bytes) {
                Ok(patch) => patch,
                Err(error) => return Ok(error_response(error)),
            };
            match adapter.update_toxic(proxy, name, patch).await {
                Ok(value) => json_value(200, &value),
                Err(error) => error_response(error),
            }
        }
        ("DELETE", ["proxies", proxy, "toxics", name]) => {
            match adapter.remove_toxic(proxy, name).await {
                Ok(()) => empty_response(204),
                Err(error) => error_response(error),
            }
        }
        ("POST", ["reset"]) => match adapter.reset().await {
            Ok(()) => empty_response(204),
            Err(error) => error_response(error),
        },
        _ => unknown_route(),
    };
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_all_v212_toxics_and_defaults_downstream() {
        for (kind, attributes) in [
            (
                "latency",
                ToxicAttributes {
                    latency: Some(10),
                    ..Default::default()
                },
            ),
            (
                "bandwidth",
                ToxicAttributes {
                    rate: Some(10),
                    ..Default::default()
                },
            ),
            (
                "slow_close",
                ToxicAttributes {
                    delay: Some(10),
                    ..Default::default()
                },
            ),
            ("timeout", ToxicAttributes::default()),
            ("reset_peer", ToxicAttributes::default()),
            (
                "slicer",
                ToxicAttributes {
                    average_size: Some(10),
                    ..Default::default()
                },
            ),
            (
                "limit_data",
                ToxicAttributes {
                    bytes: Some(10),
                    ..Default::default()
                },
            ),
        ] {
            let toxic = Toxic {
                name: None,
                r#type: kind.into(),
                stream: "downstream".into(),
                toxicity: 1.0,
                attributes,
            };
            assert_eq!(toxic.to_fault().unwrap().0, Direction::Downstream);
        }
    }

    #[test]
    fn rejects_invalid_stream_before_type() {
        let toxic = Toxic {
            name: Some("x".into()),
            r#type: "nope".into(),
            stream: "sideways".into(),
            toxicity: 1.0,
            attributes: ToxicAttributes::default(),
        };
        let error = toxic.to_fault().unwrap_err();
        assert!(
            matches!(error, CompatibilityError::Invalid(_))
                && error.to_string().contains("stream was invalid")
        );
    }

    #[test]
    fn defaults_name_stream_and_clamps_toxicity() {
        let toxic = Toxic {
            name: None,
            r#type: "latency".into(),
            stream: "Upstream".into(),
            toxicity: 2.5,
            attributes: ToxicAttributes::default(),
        };
        let (direction, spec) = toxic.to_fault().unwrap();
        assert_eq!(direction, Direction::Upstream);
        // Auto-name uses the normalized lowercase stream.
        assert_eq!(spec.id.as_str(), "latency_upstream");
        assert_eq!(spec.probability.get(), 1.0);
        // Reverse translation echoes the normalized stream and zero-filled
        // attributes.
        let rendered = fault_to_toxic(Direction::Upstream, &spec);
        assert_eq!(rendered["stream"], json!("upstream"));
        assert_eq!(rendered["attributes"], json!({"latency": 0, "jitter": 0}));
    }

    #[test]
    fn timeout_zero_is_indefinite_and_round_trips() {
        for timeout in [None, Some(0)] {
            let toxic = Toxic {
                name: Some("t".into()),
                r#type: "timeout".into(),
                stream: "downstream".into(),
                toxicity: 1.0,
                attributes: ToxicAttributes {
                    timeout,
                    ..Default::default()
                },
            };
            let (_, spec) = toxic.to_fault().unwrap();
            assert!(matches!(
                spec.kind,
                FaultKind::Blackhole(BlackholeConfig { close_after: None })
            ));
            let rendered = fault_to_toxic(Direction::Downstream, &spec);
            assert_eq!(rendered["attributes"], json!({"timeout": 0}));
        }
    }

    #[test]
    fn zero_valued_numerics_coalesce_to_minimum() {
        let cases = [
            (
                "bandwidth",
                ToxicAttributes {
                    rate: Some(0),
                    ..Default::default()
                },
            ),
            (
                "slicer",
                ToxicAttributes {
                    average_size: Some(0),
                    ..Default::default()
                },
            ),
            (
                "limit_data",
                ToxicAttributes {
                    bytes: Some(0),
                    ..Default::default()
                },
            ),
        ];
        for (kind, attributes) in cases {
            let toxic = Toxic {
                name: Some(format!("{kind}-zero")),
                r#type: kind.into(),
                stream: "downstream".into(),
                toxicity: 1.0,
                attributes,
            };
            assert!(toxic.to_fault().is_ok(), "{kind} zero must coalesce");
        }
    }

    #[test]
    fn cross_type_attributes_do_not_leak_into_update() {
        let base = ToxicAttributes {
            latency: Some(10),
            jitter: Some(1),
            ..Default::default()
        };
        let patch = ToxicAttributes {
            rate: Some(99),
            latency: Some(20),
            ..Default::default()
        };
        let merged = merge_attributes("latency", &base, &patch);
        assert_eq!(merged.latency, Some(20));
        assert_eq!(merged.jitter, Some(1));
        assert_eq!(merged.rate, None);
    }

    #[tokio::test]
    async fn compatibility_http_exposes_version_route() {
        let adapter = ToxiproxyAdapter::new(ControlState::default());
        let handle = ToxiproxyHttp::start("127.0.0.1:0".parse().unwrap(), adapter)
            .await
            .unwrap();
        let client = eggfetch_core::Client::builder().build();
        let mut response = client
            .get(&format!("http://{}/version", handle.local_addr()))
            .unwrap()
            .send()
            .await
            .unwrap();
        let body = response.bytes().await.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({"version": "2.12.0"})
        );
        handle.shutdown();
        handle.wait().await;
    }

    #[tokio::test]
    async fn compatibility_http_create_list_and_toxic_routes_share_native_state() {
        let adapter = ToxiproxyAdapter::new(ControlState::default());
        let handle = ToxiproxyHttp::start("127.0.0.1:0".parse().unwrap(), adapter)
            .await
            .unwrap();
        let client = eggfetch_core::Client::builder().build();
        let proxy = serde_json::json!({"name":"echo","listen":"127.0.0.1:0","upstream":"127.0.0.1:1","enabled":true,"toxics":[]});
        let created = client
            .post(&format!("http://{}/proxies", handle.local_addr()))
            .unwrap()
            .json(&proxy)
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(created.status().as_u16(), 201);
        let toxic = serde_json::json!({"name":"delay","type":"latency","stream":"downstream","toxicity":1.0,"attributes":{"latency":10}});
        let added = client
            .post(&format!(
                "http://{}/proxies/echo/toxics",
                handle.local_addr()
            ))
            .unwrap()
            .json(&toxic)
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(added.status().as_u16(), 200);
        let mut listed = client
            .get(&format!(
                "http://{}/proxies/echo/toxics",
                handle.local_addr()
            ))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&listed.bytes().await.unwrap()).contains("latency"));
        handle.shutdown();
        handle.wait().await;
    }

    #[tokio::test]
    async fn compatibility_http_matches_oracle_shapes_and_errors() {
        let adapter = ToxiproxyAdapter::new(ControlState::default());
        let handle = ToxiproxyHttp::start("127.0.0.1:0".parse().unwrap(), adapter)
            .await
            .unwrap();
        let client = eggfetch_core::Client::builder().build();
        let base = format!("http://{}", handle.local_addr());
        let get_json = |path: &str| {
            let client = eggfetch_core::Client::builder().build();
            let url = format!("{base}{path}");
            async move {
                let mut response = client.get(&url).unwrap().send().await.unwrap();
                let status = response.status().as_u16();
                let body = response.bytes().await.unwrap();
                (status, serde_json::from_slice::<Value>(&body).unwrap())
            }
        };

        // Empty list is a map.
        let (status, body) = get_json("/proxies").await;
        assert_eq!((status, body), (200, json!({})));

        // Missing proxy: oracle-exact 404 envelope.
        let (status, body) = get_json("/proxies/nope").await;
        assert_eq!(
            (status, body),
            (404, json!({"error": "proxy not found", "status": 404}))
        );

        // Missing upstream on create.
        let missing = client
            .post(&format!("{base}/proxies"))
            .unwrap()
            .json(&json!({"name": "m"}))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(missing.status().as_u16(), 400);

        // Create disabled: no listener, configured listen echoed.
        let created = client
            .post(&format!("{base}/proxies"))
            .unwrap()
            .json(&json!({"name":"off","listen":"127.0.0.1:0","upstream":"127.0.0.1:1","enabled":false}))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(created.status().as_u16(), 201);

        // Duplicate create: oracle-exact 409 envelope.
        let mut dup = client
            .post(&format!("{base}/proxies"))
            .unwrap()
            .json(&json!({"name":"off","listen":"127.0.0.1:0","upstream":"127.0.0.1:1"}))
            .unwrap()
            .send()
            .await
            .unwrap();
        let dup_body = dup.bytes().await.unwrap();
        assert_eq!(dup.status().as_u16(), 409);
        assert_eq!(
            serde_json::from_slice::<Value>(&dup_body).unwrap(),
            json!({"error": "proxy already exists", "status": 409})
        );

        // Populate empty list echoes null.
        let mut empty = client
            .post(&format!("{base}/populate"))
            .unwrap()
            .body("[]")
            .send()
            .await
            .unwrap();
        let empty_body = empty.bytes().await.unwrap();
        assert_eq!(empty.status().as_u16(), 201);
        assert_eq!(
            serde_json::from_slice::<Value>(&empty_body).unwrap(),
            json!({"proxies": null})
        );

        // Toxic on missing proxy.
        let mut missing_toxic = client
            .post(&format!("{base}/proxies/nope/toxics"))
            .unwrap()
            .json(&json!({"name":"t","type":"latency","stream":"downstream"}))
            .unwrap()
            .send()
            .await
            .unwrap();
        let missing_body = missing_toxic.bytes().await.unwrap();
        assert_eq!(missing_toxic.status().as_u16(), 404);
        assert_eq!(
            serde_json::from_slice::<Value>(&missing_body).unwrap(),
            json!({"error": "proxy not found", "status": 404})
        );

        handle.shutdown();
        handle.wait().await;
    }

    #[tokio::test]
    async fn compatibility_http_reset_clears_and_reenables() {
        let adapter = ToxiproxyAdapter::new(ControlState::default());
        let handle = ToxiproxyHttp::start("127.0.0.1:0".parse().unwrap(), adapter)
            .await
            .unwrap();
        let client = eggfetch_core::Client::builder().build();
        let base = format!("http://{}", handle.local_addr());
        client
            .post(&format!("{base}/proxies"))
            .unwrap()
            .json(&json!({"name":"r","listen":"127.0.0.1:0","upstream":"127.0.0.1:1"}))
            .unwrap()
            .send()
            .await
            .unwrap();
        client
            .post(&format!("{base}/proxies/r/toxics"))
            .unwrap()
            .json(&json!({"name":"t","type":"latency","stream":"downstream"}))
            .unwrap()
            .send()
            .await
            .unwrap();
        client
            .post(&format!("{base}/proxies/r"))
            .unwrap()
            .json(&json!({"enabled": false}))
            .unwrap()
            .send()
            .await
            .unwrap();
        let reset = client
            .post(&format!("{base}/reset"))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(reset.status().as_u16(), 204);
        let mut view = client
            .get(&format!("{base}/proxies/r"))
            .unwrap()
            .send()
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&view.bytes().await.unwrap()).unwrap();
        assert_eq!(body["enabled"], json!(true));
        assert_eq!(body["toxics"], json!([]));
        handle.shutdown();
        handle.wait().await;
    }
}
