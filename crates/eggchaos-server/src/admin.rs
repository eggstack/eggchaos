use std::{
    collections::{HashMap, HashSet},
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use eggserve_primitives::{Request, RequestBodyPolicy, Response, ResponseBody, StatusCode};
use eggserve_server::{service_fn_with_policy, RuntimeConfig, Server, ServerHandle, ServiceError};
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::net::TcpListener;

use crate::{ControlError, ControlState, NativeConfigError};
use eggchaos_core::Direction;
use eggchaos_protocol::ErrorEnvelopeV1;

/// Native admin listener security settings.
#[derive(Clone)]
pub struct AdminConfig {
    /// Bind address.
    pub bind: SocketAddr,
    /// Explicit non-loopback opt-in.
    pub public_admin: bool,
    /// Required bearer token for non-loopback operation.
    pub auth_token: Option<String>,
}

impl std::fmt::Debug for AdminConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminConfig")
            .field("bind", &self.bind)
            .field("public_admin", &self.public_admin)
            .field(
                "auth_token",
                &self.auth_token.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8475".parse().expect("loopback admin address"),
            public_admin: false,
            auth_token: None,
        }
    }
}

/// Errors from native admin startup and request processing.
#[derive(Debug, Error)]
pub enum AdminError {
    /// Non-loopback operation was not explicitly secured.
    #[error("public admin requires public_admin=true and auth_token")]
    InsecurePublicBind,
    /// Listener failed to bind.
    #[error("admin bind: {0}")]
    Bind(#[from] io::Error),
    /// `EggServe` startup failed.
    #[error("admin server: {0}")]
    Server(String),
    /// Mutation conflicts with current state.
    #[error("conflict: {0}")]
    Conflict(String),
    /// Invalid request/configuration.
    #[error("invalid request: {0}")]
    Invalid(String),
}

const MAX_ALLOWED_HOSTS: usize = 64;
const AUTH_FAILURE_BURST: u8 = 5;
const AUTH_FAILURE_REFILL: Duration = Duration::from_secs(1);
const AUTH_FAILURE_PEER_CAPACITY: usize = 1024;
const AUTH_FAILURE_PEER_TTL: Duration = Duration::from_secs(300);
const AUTH_FAILURE_FALLBACK_DELAY: Duration = Duration::from_millis(50);

#[derive(Debug)]
struct PeerFailureState {
    tokens: u8,
    last_refill: Instant,
    last_seen: Instant,
}

#[derive(Debug, Default)]
struct AuthFailureBudget {
    peers: HashMap<IpAddr, PeerFailureState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthFailureDisposition {
    Reject,
    Throttled,
    FallbackDelay,
}

impl AuthFailureBudget {
    fn record_failure(&mut self, peer: Option<IpAddr>) -> AuthFailureDisposition {
        let Some(peer) = peer else {
            return AuthFailureDisposition::FallbackDelay;
        };
        let now = Instant::now();
        if !self.peers.contains_key(&peer) {
            if self.peers.len() >= AUTH_FAILURE_PEER_CAPACITY {
                self.peers.retain(|_, state| {
                    now.saturating_duration_since(state.last_seen) < AUTH_FAILURE_PEER_TTL
                });
            }
            if self.peers.len() >= AUTH_FAILURE_PEER_CAPACITY {
                return AuthFailureDisposition::FallbackDelay;
            }
            self.peers.insert(
                peer,
                PeerFailureState {
                    tokens: AUTH_FAILURE_BURST,
                    last_refill: now,
                    last_seen: now,
                },
            );
        }
        let state = self.peers.get_mut(&peer).expect("peer was inserted");
        let elapsed = now.saturating_duration_since(state.last_refill);
        let refill = u8::try_from(elapsed.as_secs().min(u64::from(AUTH_FAILURE_BURST)))
            .unwrap_or(AUTH_FAILURE_BURST);
        if refill > 0 {
            state.tokens = state.tokens.saturating_add(refill).min(AUTH_FAILURE_BURST);
            state.last_refill += AUTH_FAILURE_REFILL * u32::from(refill);
        }
        state.last_seen = now;
        if state.tokens == 0 {
            AuthFailureDisposition::Throttled
        } else {
            state.tokens -= 1;
            AuthFailureDisposition::Reject
        }
    }
}

/// Returns whether a token contains at least 32 non-whitespace bytes.
pub(crate) fn has_public_token_strength(token: &str) -> bool {
    token
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .count()
        >= 32
}

/// Validate the optional explicit host allowlist at both config and startup boundaries.
pub(crate) fn validate_allowed_hosts(hosts: Option<&[String]>) -> Result<(), String> {
    let Some(hosts) = hosts else {
        return Ok(());
    };
    if hosts.is_empty() {
        return Err("must contain at least one authority when specified".into());
    }
    if hosts.len() > MAX_ALLOWED_HOSTS {
        return Err(format!(
            "must contain at most {MAX_ALLOWED_HOSTS} authorities"
        ));
    }
    for host in hosts {
        if normalize_authority(host).is_none() {
            return Err(
                "entries must be valid host authorities without wildcards, schemes, or paths"
                    .into(),
            );
        }
    }
    Ok(())
}

fn normalize_authority(value: &str) -> Option<String> {
    if value.is_empty()
        || value.len() > 512
        || !value.is_ascii()
        || value.bytes().any(|byte| {
            byte.is_ascii_whitespace()
                || byte.is_ascii_control()
                || matches!(byte, b'/' | b'\\' | b'?' | b'#' | b'@' | b'*' | b'%')
        })
    {
        return None;
    }

    let (host, port) = if let Some(rest) = value.strip_prefix('[') {
        let close = rest.find(']')?;
        let address = rest[..close].parse::<Ipv6Addr>().ok()?;
        let suffix = &rest[close + 1..];
        let port = if suffix.is_empty() {
            None
        } else {
            Some(parse_authority_port(suffix.strip_prefix(':')?)?)
        };
        (format!("[{address}]"), port)
    } else {
        if value.contains('[') || value.contains(']') {
            return None;
        }
        let (host, port) = match value.rsplit_once(':') {
            Some((host, port)) => {
                if host.contains(':') {
                    return None;
                }
                (host, Some(parse_authority_port(port)?))
            }
            None => (value, None),
        };
        if host.is_empty() {
            return None;
        }
        let normalized_host = if let Ok(address) = host.parse::<Ipv4Addr>() {
            address.to_string()
        } else {
            if host.len() > 253
                || host.starts_with('.')
                || host.ends_with('.')
                || host
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'.')
            {
                return None;
            }
            for label in host.split('.') {
                if label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                {
                    return None;
                }
            }
            host.to_ascii_lowercase()
        };
        (normalized_host, port)
    };

    Some(match port {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

fn parse_authority_port(suffix: &str) -> Option<u16> {
    let port = suffix.parse::<u16>().ok()?;
    (port != 0).then_some(port)
}

fn socket_authority(address: SocketAddr) -> String {
    match address.ip() {
        IpAddr::V4(ip) => format!("{ip}:{}", address.port()),
        IpAddr::V6(ip) => format!("[{ip}]:{}", address.port()),
    }
}

fn default_allowed_hosts(address: SocketAddr) -> HashSet<String> {
    let port = address.port();
    let mut hosts = HashSet::from([socket_authority(address)]);
    if address.ip().is_loopback() {
        hosts.extend([
            format!("localhost:{port}"),
            format!("127.0.0.1:{port}"),
            format!("[::1]:{port}"),
        ]);
    }
    hosts
}

fn origin_authority(value: &str) -> Option<String> {
    let (scheme, authority) = value.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    normalize_authority(authority)
}

/// EggServe-backed native admin server.
pub struct NativeAdmin;

/// Control handle for a native admin listener.
pub struct AdminHandle {
    local_addr: SocketAddr,
    server: Option<ServerHandle>,
}

impl AdminHandle {
    /// Actual bound address.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
    /// Request shutdown.
    pub fn shutdown(&self) {
        if let Some(server) = &self.server {
            server.shutdown();
        }
    }
    /// Wait for shutdown completion.
    pub async fn wait(&mut self) {
        if let Some(server) = self.server.take() {
            server.wait().await;
        }
    }
}

impl NativeAdmin {
    /// Start a bounded versioned native admin API using `EggServe`'s H1 leaf runtime.
    pub async fn start(
        config: AdminConfig,
        state: ControlState,
    ) -> Result<AdminHandle, AdminError> {
        Self::start_with_allowed_hosts(config, state, None).await
    }

    /// Start the native admin API with additional accepted Host/Origin authorities.
    ///
    /// This additive entry point preserves the existing [`AdminConfig`] shape.
    pub async fn start_with_allowed_hosts(
        config: AdminConfig,
        state: ControlState,
        allowed_hosts: Option<Vec<String>>,
    ) -> Result<AdminHandle, AdminError> {
        validate_allowed_hosts(allowed_hosts.as_deref())
            .map_err(|message| AdminError::Invalid(format!("admin.allowed_hosts {message}")))?;
        if !config.bind.ip().is_loopback()
            && (!config.public_admin
                || !config
                    .auth_token
                    .as_deref()
                    .is_some_and(has_public_token_strength))
        {
            return Err(AdminError::InsecurePublicBind);
        }
        if config.bind.ip().is_loopback()
            && config
                .auth_token
                .as_deref()
                .is_some_and(|token| !has_public_token_strength(token))
        {
            tracing::warn!("loopback admin bearer token is shorter than 32 non-whitespace bytes");
        }
        let listener = TcpListener::bind(config.bind).await?;
        let runtime = RuntimeConfig {
            max_request_body_bytes: 1024 * 1024,
            max_connections: 128,
            ..RuntimeConfig::default()
        };
        let local_addr = listener.local_addr()?;
        let mut allowed_host_set = default_allowed_hosts(local_addr);
        for host in allowed_hosts.as_deref().unwrap_or_default() {
            allowed_host_set.insert(normalize_authority(host).expect("allowlist was validated"));
        }
        let allowed_hosts = Arc::new(allowed_host_set);
        let auth = config.auth_token.map(Arc::<str>::from);
        let auth_failures = Arc::new(Mutex::new(AuthFailureBudget::default()));
        let service = service_fn_with_policy(
            move |request: Request| {
                let state = state.clone();
                let auth = auth.clone();
                let allowed_hosts = allowed_hosts.clone();
                let auth_failures = auth_failures.clone();
                async move { handle_request(request, state, auth, allowed_hosts, auth_failures).await }
            },
            RequestBodyPolicy::Buffer {
                max_bytes: 1024 * 1024,
            },
        );
        let server = Server::builder()
            .runtime(runtime)
            .from_listener(listener)
            .build()
            .map_err(|error| AdminError::Server(error.to_string()))?;
        let handle = server
            .start_with_service(service)
            .await
            .map_err(|error| AdminError::Server(error.to_string()))?;
        Ok(AdminHandle {
            local_addr,
            server: Some(handle),
        })
    }
}

/// Bounded native error envelope shared with `eggchaos-protocol`.
fn envelope(code: &str, message: &str) -> ErrorEnvelopeV1 {
    ErrorEnvelopeV1::new(code, message)
}

fn control_status(error: &ControlError) -> StatusCode {
    match error {
        ControlError::NotFound(_) => StatusCode::NOT_FOUND,
        ControlError::Conflict(_) | ControlError::BindFailed { .. } => {
            StatusCode::new(409).expect("conflict status")
        }
        ControlError::Invalid(_) => StatusCode::BAD_REQUEST,
        ControlError::RestartFailed { .. } => StatusCode::new(500).expect("server-error status"),
    }
}

fn control_error_response(error: &ControlError) -> Response {
    json_response(
        control_status(error),
        &envelope(error.code(), &error.to_string()),
    )
}

fn host_rejection(host: Option<&str>, allowed_hosts: &HashSet<String>) -> Option<Response> {
    let host = host.and_then(normalize_authority);
    host.as_ref()
        .is_none_or(|host| !allowed_hosts.contains(host))
        .then(|| {
            json_response(
                StatusCode::FORBIDDEN,
                &envelope("invalid_host", "request host is not allowed"),
            )
        })
}

fn body_or_origin_rejection(
    method: &str,
    has_body: bool,
    content_type: Option<&str>,
    origin: Option<&str>,
    origin_present: bool,
    allowed_hosts: &HashSet<String>,
) -> Option<Response> {
    if has_body && !content_type.is_some_and(is_json_media_type) {
        return Some(json_response(
            StatusCode::new(415).expect("unsupported media type status"),
            &envelope(
                "unsupported_media_type",
                "non-empty admin request bodies require application/json",
            ),
        ));
    }
    if !matches!(method, "GET" | "HEAD" | "OPTIONS") && origin_present {
        let origin = origin.and_then(|origin| {
            if origin.eq_ignore_ascii_case("null") {
                None
            } else {
                origin_authority(origin)
            }
        });
        if origin
            .as_ref()
            .is_none_or(|origin| !allowed_hosts.contains(origin))
        {
            return Some(json_response(
                StatusCode::FORBIDDEN,
                &envelope("invalid_origin", "request origin is not allowed"),
            ));
        }
    }
    None
}

async fn handle_request(
    request: Request,
    state: ControlState,
    auth: Option<Arc<str>>,
    allowed_hosts: Arc<HashSet<String>>,
    auth_failures: Arc<Mutex<AuthFailureBudget>>,
) -> Result<Response, ServiceError> {
    let (head, body, connection) = request.into_parts();
    if let Some(response) = host_rejection(
        head.headers()
            .get_first("host")
            .and_then(|value| value.to_str().ok()),
        &allowed_hosts,
    ) {
        return Ok(response);
    }

    if let Some(expected) = auth.as_deref() {
        let supplied = head
            .headers()
            .get_first("authorization")
            .and_then(|value| value.to_str().ok());
        let token = supplied.and_then(parse_bearer_token);
        if token.is_none_or(|token| {
            expected.is_empty() || !constant_time_equal(token.as_bytes(), expected.as_bytes())
        }) {
            let disposition = auth_failures
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .record_failure(connection.remote_addr.map(|address| address.ip()));
            if disposition == AuthFailureDisposition::FallbackDelay {
                tokio::time::sleep(AUTH_FAILURE_FALLBACK_DELAY).await;
            }
            return Ok(authorization_failure_response(
                disposition == AuthFailureDisposition::Throttled,
            ));
        }
    }

    let method = head.method().as_str().to_owned();
    let path = head.target().path().to_owned();
    let body = body.read_all().await.map_err(|error| {
        // Typed limit check: only oversize bodies are 413; other
        // I/O/internal errors are 400. Never match message text.
        if error.is_limit_exceeded() {
            ServiceError::rejected(413, error.to_string())
        } else {
            ServiceError::rejected(400, error.to_string())
        }
    })?;
    let origin_header = head.headers().get_first("origin");
    if let Some(response) = body_or_origin_rejection(
        &method,
        !body.is_empty(),
        head.headers()
            .get_first("content-type")
            .and_then(|value| value.to_str().ok()),
        origin_header.and_then(|value| value.to_str().ok()),
        origin_header.is_some(),
        &allowed_hosts,
    ) {
        return Ok(response);
    }
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let decoded_id = if segments.len() == 5
        && segments[0] == "v1"
        && (segments[1] == "proxies" || segments[1] == "datagram-proxies")
        && segments[3] == "faults"
    {
        match decode_path_component(segments[4]) {
            Ok(id) => Some(id),
            Err(()) => {
                return Ok(json_response(
                    StatusCode::BAD_REQUEST,
                    &envelope("invalid", "invalid path component"),
                ));
            }
        }
    } else {
        None
    };
    let decoded_segments: Vec<&str> = segments
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            if index == 4 {
                decoded_id.as_deref().unwrap_or(segment)
            } else {
                segment
            }
        })
        .collect();
    let response = match route(&method, &decoded_segments, &body, &state).await {
        RouteOutcome::Response(response) => response,
        RouteOutcome::ControlError(error) => control_error_response(&error),
        RouteOutcome::BadJson(error) => {
            json_response(StatusCode::BAD_REQUEST, &envelope("invalid_json", &error))
        }
        RouteOutcome::NotFound => json_response(
            StatusCode::NOT_FOUND,
            &envelope("not_found", "route not found"),
        ),
    };
    Ok(response)
}

fn parse_bearer_token(value: &str) -> Option<&str> {
    let separator = value.find(' ')?;
    if !value[..separator].eq_ignore_ascii_case("bearer") {
        return None;
    }
    Some(&value[separator + 1..])
}

fn is_json_media_type(value: &str) -> bool {
    value
        .split(';')
        .next()
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"))
}

fn authorization_failure_response(throttled: bool) -> Response {
    let status = if throttled {
        StatusCode::new(429).expect("too many requests status")
    } else {
        StatusCode::FORBIDDEN
    };
    let body = serde_json::to_vec(&envelope("unauthorized", "authorization required"))
        .unwrap_or_else(|_| b"{\"error\":{\"code\":\"unauthorized\"}}".to_vec());
    let builder = Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .expect("static header is valid");
    let builder = if throttled {
        builder
            .header("retry-after", "1")
            .expect("static retry header is valid")
    } else {
        builder
    };
    builder
        .body(ResponseBody::Bytes(body))
        .expect("status and body are valid")
}

fn decode_path_component(input: &str) -> Result<String, ()> {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(());
            }
            let hi = hex_value(bytes[i + 1]).ok_or(())?;
            let lo = hex_value(bytes[i + 2]).ok_or(())?;
            decoded.push((hi << 4) | lo);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| ())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

enum RouteOutcome {
    Response(Response),
    ControlError(ControlError),
    BadJson(String),
    NotFound,
}

fn parse_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, RouteOutcome> {
    serde_json::from_slice(body).map_err(|error| RouteOutcome::BadJson(error.to_string()))
}

// M059: single native route table; splitting would fork the
// method/segment dispatch contract.
#[allow(clippy::too_many_lines)]
async fn route(method: &str, segments: &[&str], body: &[u8], state: &ControlState) -> RouteOutcome {
    use crate::operations as ops;
    match (method, segments) {
        ("GET", ["v1", "health"]) => RouteOutcome::Response(json_response(
            StatusCode::OK,
            &serde_json::json!({"running": true, "generation": state.generation()}),
        )),
        ("GET", ["v1", "version"]) => RouteOutcome::Response(json_response(
            StatusCode::OK,
            &serde_json::json!({"version": env!("CARGO_PKG_VERSION"), "api": "v1"}),
        )),
        ("GET", ["metrics"]) => RouteOutcome::Response(text_response(state.metrics_text().await)),
        ("POST", ["v1", "reset"]) => match ops::reset_service(state).await {
            Ok(report) => RouteOutcome::Response(json_response(StatusCode::OK, &report)),
            Err(error) => RouteOutcome::ControlError(error),
        },
        ("GET", ["v1", "datagram-proxies"]) => RouteOutcome::Response(json_response(
            StatusCode::OK,
            &state
                .datagram_proxies()
                .await
                .into_iter()
                .map(crate::NativeDatagramProxyViewV1::from)
                .collect::<Vec<_>>(),
        )),
        ("POST", ["v1", "datagram-proxies"]) => {
            let request: crate::NativeDatagramProxyRequestV1 = match parse_json(body) {
                Ok(value) => value,
                Err(error) => return error,
            };
            match ops::apply_datagram_proxy_request(state, request).await {
                Ok(outcome) => RouteOutcome::Response(json_response(
                    StatusCode::CREATED,
                    &serde_json::json!({"proxy": outcome.proxy, "generation": outcome.generation}),
                )),
                Err(error) => RouteOutcome::ControlError(error),
            }
        }
        ("GET", ["v1", "datagram-proxies", name]) => match state.get_datagram_proxy(name).await {
            Some(view) => RouteOutcome::Response(json_response(
                StatusCode::OK,
                &crate::NativeDatagramProxyViewV1::from(view),
            )),
            None => RouteOutcome::ControlError(ControlError::NotFound((*name).into())),
        },
        ("PATCH", ["v1", "datagram-proxies", name]) => {
            let patch: crate::NativeDatagramProxyPatchV1 = match parse_json(body) {
                Ok(value) => value,
                Err(error) => return error,
            };
            match ops::apply_datagram_proxy_patch(state, name, patch).await {
                Ok(outcome) => RouteOutcome::Response(json_response(
                    StatusCode::OK,
                    &serde_json::json!({"proxy": outcome.proxy, "generation": outcome.generation}),
                )),
                Err(error) => RouteOutcome::ControlError(error),
            }
        }
        ("DELETE", ["v1", "datagram-proxies", name]) => {
            match state.delete_datagram_proxy(name).await {
                Ok(generation) => RouteOutcome::Response(json_response(
                    StatusCode::OK,
                    &serde_json::json!({"generation": generation, "deleted": true}),
                )),
                Err(error) => RouteOutcome::ControlError(error),
            }
        }
        ("GET", ["v1", "datagram-proxies", name, "faults"]) => {
            let upstream = state.get_datagram_plan(name, Direction::Upstream).await;
            let downstream = state.get_datagram_plan(name, Direction::Downstream).await;
            match (upstream, downstream) {
                (Ok((up, ug, us)), Ok((down, dg, ds))) => RouteOutcome::Response(json_response(
                    StatusCode::OK,
                    &serde_json::json!({
                        "upstream": {"generation": ug, "seed_namespace": us, "faults": up.faults().iter().cloned().map(crate::DatagramFaultSpecV1::from).collect::<Vec<_>>()},
                        "downstream": {"generation": dg, "seed_namespace": ds, "faults": down.faults().iter().cloned().map(crate::DatagramFaultSpecV1::from).collect::<Vec<_>>()}
                    }),
                )),
                (Err(error), _) | (_, Err(error)) => RouteOutcome::ControlError(error),
            }
        }
        ("POST", ["v1", "datagram-proxies", name, "faults"]) => {
            let upsert: crate::DatagramFaultUpsertV1 = match parse_json(body) {
                Ok(value) => value,
                Err(error) => return error,
            };
            match ops::apply_datagram_fault_upsert(state, name, upsert).await {
                Ok(outcome) => RouteOutcome::Response(json_response(
                    StatusCode::CREATED,
                    &serde_json::json!({"direction": outcome.direction.as_str(), "fault": outcome.fault, "generation": outcome.generation}),
                )),
                Err(error) => RouteOutcome::ControlError(error),
            }
        }
        ("GET", ["v1", "datagram-proxies", name, "faults", id]) => {
            match state.get_datagram_fault(name, id).await {
                Some((direction, fault)) => RouteOutcome::Response(json_response(
                    StatusCode::OK,
                    &serde_json::json!({"direction": direction.as_str(), "fault": crate::DatagramFaultSpecV1::from(fault)}),
                )),
                None => RouteOutcome::ControlError(ControlError::NotFound(format!(
                    "fault {id} on {name}"
                ))),
            }
        }
        ("PATCH", ["v1", "datagram-proxies", name, "faults", id]) => {
            let patch: crate::DatagramFaultPatchV1 = match parse_json(body) {
                Ok(value) => value,
                Err(error) => return error,
            };
            match ops::apply_datagram_fault_patch(state, name, id, patch).await {
                Ok(outcome) => RouteOutcome::Response(json_response(
                    StatusCode::OK,
                    &serde_json::json!({"direction": outcome.direction.as_str(), "fault": outcome.fault, "generation": outcome.generation}),
                )),
                Err(error) => RouteOutcome::ControlError(error),
            }
        }
        ("DELETE", ["v1", "datagram-proxies", name, "faults", id]) => {
            match state.remove_datagram_fault(name, id).await {
                Ok((_, generation)) => RouteOutcome::Response(json_response(
                    StatusCode::OK,
                    &serde_json::json!({"generation": generation, "deleted": true}),
                )),
                Err(error) => RouteOutcome::ControlError(error),
            }
        }
        ("GET", ["v1", "datagram-associations"]) => RouteOutcome::Response(json_response(
            StatusCode::OK,
            &state
                .datagram_associations()
                .await
                .into_iter()
                .map(crate::NativeDatagramAssociationViewV1::from)
                .collect::<Vec<_>>(),
        )),
        ("GET", ["v1", "datagram-associations", id]) => match id.parse::<u64>() {
            Ok(id) => match state.get_datagram_association(id).await {
                Some(snapshot) => RouteOutcome::Response(json_response(
                    StatusCode::OK,
                    &crate::NativeDatagramAssociationViewV1::from(snapshot),
                )),
                None => RouteOutcome::ControlError(ControlError::NotFound(format!(
                    "datagram association {id}"
                ))),
            },
            Err(_) => RouteOutcome::ControlError(ControlError::Invalid(
                "datagram association id must be an integer".into(),
            )),
        },
        ("DELETE", ["v1", "datagram-associations", id]) => match id.parse::<u64>() {
            Ok(id) if ops::kill_datagram_association(state, id).await => {
                RouteOutcome::Response(json_response(
                    StatusCode::OK,
                    &serde_json::json!({"id": id, "terminated": true}),
                ))
            }
            Ok(id) => RouteOutcome::ControlError(ControlError::NotFound(format!(
                "datagram association {id}"
            ))),
            Err(_) => RouteOutcome::ControlError(ControlError::Invalid(
                "datagram association id must be an integer".into(),
            )),
        },
        ("GET", ["v1", "proxies"]) => {
            let views: Vec<crate::NativeProxyViewV1> = state
                .list()
                .await
                .into_iter()
                .map(crate::NativeProxyViewV1::from)
                .collect();
            RouteOutcome::Response(json_response(StatusCode::OK, &views))
        }
        ("POST", ["v1", "proxies"]) => {
            let request: crate::NativeProxyRequestV1 = match parse_json(body) {
                Ok(spec) => spec,
                Err(outcome) => return outcome,
            };
            match ops::apply_proxy_request(state, request).await {
                Ok(outcome) => RouteOutcome::Response(json_response(
                    StatusCode::CREATED,
                    &serde_json::json!({"proxy": outcome.proxy, "generation": outcome.generation}),
                )),
                Err(error) => RouteOutcome::ControlError(error),
            }
        }
        ("GET", ["v1", "proxies", name]) => match state.get(name).await {
            Some(view) => RouteOutcome::Response(json_response(
                StatusCode::OK,
                &crate::NativeProxyViewV1::from(view),
            )),
            None => RouteOutcome::ControlError(ControlError::NotFound((*name).to_owned())),
        },
        ("PATCH", ["v1", "proxies", name]) => {
            let patch: crate::NativeProxyPatchV1 = match parse_json(body) {
                Ok(patch) => patch,
                Err(outcome) => return outcome,
            };
            match ops::apply_proxy_patch(state, name, patch).await {
                Ok(outcome) => RouteOutcome::Response(json_response(
                    StatusCode::OK,
                    &serde_json::json!({"proxy": outcome.proxy, "generation": outcome.generation}),
                )),
                Err(error) => RouteOutcome::ControlError(error),
            }
        }
        ("DELETE", ["v1", "proxies", name]) => match state.delete_proxy(name).await {
            Ok(generation) => RouteOutcome::Response(json_response(
                StatusCode::OK,
                &serde_json::json!({"generation": generation, "deleted": true}),
            )),
            Err(error) => RouteOutcome::ControlError(error),
        },
        ("GET", ["v1", "proxies", name, "faults"]) => match state.list_faults(name).await {
            Some((upstream, downstream)) => RouteOutcome::Response(json_response(
                StatusCode::OK,
                &serde_json::json!({
                    "upstream": upstream.iter().map(crate::NativeFaultViewV1::from).collect::<Vec<_>>(),
                    "downstream": downstream.iter().map(crate::NativeFaultViewV1::from).collect::<Vec<_>>(),
                }),
            )),
            None => RouteOutcome::ControlError(ControlError::NotFound((*name).to_owned())),
        },
        ("POST", ["v1", "proxies", name, "faults"]) => {
            let upsert: crate::FaultUpsertV1 = match parse_json(body) {
                Ok(upsert) => upsert,
                Err(outcome) => return outcome,
            };
            match ops::apply_stream_fault_upsert(state, name, upsert).await {
                Ok(outcome) => RouteOutcome::Response(json_response(
                    StatusCode::CREATED,
                    &serde_json::json!({
                        "direction": outcome.direction.as_str(),
                        "fault": outcome.fault,
                        "generation": outcome.generation,
                    }),
                )),
                Err(error) => RouteOutcome::ControlError(error),
            }
        }
        ("GET", ["v1", "proxies", name, "faults", id]) => match state.get_fault(name, id).await {
            Some((direction, fault)) => RouteOutcome::Response(json_response(
                StatusCode::OK,
                &serde_json::json!({"direction": direction.as_str(), "fault": crate::NativeFaultViewV1::from(&fault)}),
            )),
            None => {
                RouteOutcome::ControlError(ControlError::NotFound(format!("fault {id} on {name}")))
            }
        },
        ("PATCH", ["v1", "proxies", name, "faults", id]) => {
            let patch: crate::FaultPatchV1 = match parse_json(body) {
                Ok(patch) => patch,
                Err(outcome) => return outcome,
            };
            match ops::apply_stream_fault_patch(state, name, id, patch).await {
                Ok(outcome) => RouteOutcome::Response(json_response(
                    StatusCode::OK,
                    &serde_json::json!({
                        "direction": outcome.direction.as_str(),
                        "fault": outcome.fault,
                        "generation": outcome.generation,
                    }),
                )),
                Err(error) => RouteOutcome::ControlError(error),
            }
        }
        ("DELETE", ["v1", "proxies", name, "faults", id]) => {
            match state.remove_fault(name, id).await {
                Ok(generation) => RouteOutcome::Response(json_response(
                    StatusCode::OK,
                    &serde_json::json!({"generation": generation, "deleted": true}),
                )),
                Err(error) => RouteOutcome::ControlError(error),
            }
        }
        ("GET", ["v1", "connections"]) => {
            RouteOutcome::Response(json_response(StatusCode::OK, &state.connections().await))
        }
        ("GET", ["v1", "connections", id]) => match id.parse::<u64>() {
            Ok(id) => match ops::get_connection(state, id).await {
                Some(snapshot) => RouteOutcome::Response(json_response(StatusCode::OK, &snapshot)),
                None => {
                    RouteOutcome::ControlError(ControlError::NotFound(format!("connection {id}")))
                }
            },
            Err(_) => RouteOutcome::ControlError(ControlError::Invalid(
                "connection id must be an integer".into(),
            )),
        },
        ("DELETE", ["v1", "connections", id]) => match id.parse::<u64>() {
            Ok(id) => {
                if ops::kill_connection(state, id).await {
                    RouteOutcome::Response(json_response(
                        StatusCode::OK,
                        &serde_json::json!({"id": id, "terminated": true}),
                    ))
                } else {
                    RouteOutcome::ControlError(ControlError::NotFound(format!("connection {id}")))
                }
            }
            Err(_) => RouteOutcome::ControlError(ControlError::Invalid(
                "connection id must be an integer".into(),
            )),
        },
        ("GET", ["v1", "history"]) => {
            RouteOutcome::Response(json_response(StatusCode::OK, &state.history().await))
        }
        ("POST", ["v1", "scenarios", "apply"]) => match version_of(body) {
            Some(2) => match serde_json::from_slice::<crate::ScenarioScheduleV2Dto>(body) {
                Ok(dto) => match ops::apply_scenario_v2_dto(state, dto).await {
                    Ok(record) => RouteOutcome::Response(json_response(
                        StatusCode::new(202).expect("accepted status"),
                        &record,
                    )),
                    Err(error) => RouteOutcome::ControlError(error),
                },
                Err(error) => RouteOutcome::BadJson(error.to_string()),
            },
            _ => match serde_json::from_slice::<crate::ScenarioV1>(body) {
                Ok(scenario) => match ops::apply_scenario_v1(state, scenario).await {
                    Ok(record) => RouteOutcome::Response(json_response(
                        StatusCode::new(202).expect("accepted status"),
                        &record,
                    )),
                    Err(error) => RouteOutcome::ControlError(error),
                },
                Err(error) => RouteOutcome::BadJson(error.to_string()),
            },
        },
        ("POST", ["v1", "scenarios", "validate"]) => {
            match serde_json::from_slice::<crate::ScenarioScheduleV2Dto>(body) {
                Ok(dto) => match ops::validate_scenario_v2_dto(dto) {
                    Ok(value) => RouteOutcome::Response(json_response(StatusCode::OK, &value)),
                    Err(error) => RouteOutcome::ControlError(error),
                },
                Err(error) => RouteOutcome::BadJson(error.to_string()),
            }
        }
        ("POST", ["v1", "scenarios", "compile"]) => {
            match serde_json::from_slice::<crate::ScenarioScheduleV2Dto>(body) {
                Ok(dto) => match ops::compile_scenario_v2_dto(dto) {
                    Ok(value) => RouteOutcome::Response(json_response(StatusCode::OK, &value)),
                    Err(error) => RouteOutcome::ControlError(error),
                },
                Err(error) => RouteOutcome::BadJson(error.to_string()),
            }
        }
        ("GET", ["v1", "scenarios", id]) => match id.parse::<u64>() {
            Ok(run_id) => match ops::get_scenario_run(state, run_id).await {
                Some(record) => RouteOutcome::Response(json_response(
                    StatusCode::OK,
                    &match record {
                        crate::ScenarioRunLookup::V1(record) => {
                            serde_json::json!(record)
                        }
                        crate::ScenarioRunLookup::V2(record) => {
                            serde_json::json!(record)
                        }
                    },
                )),
                None => RouteOutcome::ControlError(crate::ControlError::NotFound(format!(
                    "scenario run {run_id}"
                ))),
            },
            Err(_) => RouteOutcome::ControlError(crate::ControlError::Invalid(
                "scenario run id must be an integer".into(),
            )),
        },
        ("DELETE", ["v1", "scenarios", id]) => match id.parse::<u64>() {
            Ok(run_id) => match ops::cancel_scenario_run(state, run_id).await {
                Some(record) => RouteOutcome::Response(json_response(
                    StatusCode::OK,
                    &match record {
                        crate::ScenarioRunLookup::V1(record) => {
                            serde_json::json!(record)
                        }
                        crate::ScenarioRunLookup::V2(record) => {
                            serde_json::json!(record)
                        }
                    },
                )),
                None => RouteOutcome::ControlError(crate::ControlError::NotFound(format!(
                    "scenario run {run_id}"
                ))),
            },
            Err(_) => RouteOutcome::ControlError(crate::ControlError::Invalid(
                "scenario run id must be an integer".into(),
            )),
        },
        _ => RouteOutcome::NotFound,
    }
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let left = Sha256::digest(left);
    let right = Sha256::digest(right);
    left.iter()
        .zip(right.iter())
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

/// Peek the `version` field of a scenario apply body without fully
/// parsing it, so version-aware dispatch can route v1 documents to
/// the v1 path and v2 schedules to the v2 path. Returns `None` when
/// the body is not JSON or carries no numeric version.
fn version_of(body: &[u8]) -> Option<u32> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    value.get("version")?.as_u64()?.try_into().ok()
}

fn json_response<T: Serialize>(status: StatusCode, value: &T) -> Response {
    let body = serde_json::to_vec(value)
        .unwrap_or_else(|_| b"{\"error\":{\"code\":\"serialization\"}}".to_vec());
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .expect("static header is valid")
        .body(ResponseBody::Bytes(body))
        .expect("status and body are valid")
}

fn text_response(body: String) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain; version=0.0.4")
        .expect("static header is valid")
        .body(ResponseBody::Bytes(body.into_bytes()))
        .expect("status and body are valid")
}

/// Keep this import in the public dependency audit: configuration errors are
/// intentionally translated at the file boundary, never leaked as debug maps.
#[allow(dead_code)]
// M059: by-value error keeps the translation call sites infallible without
// borrowing the consumed outcome value.
#[allow(clippy::needless_pass_by_value)]
fn _config_error_kind(error: NativeConfigError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggfetch_core::Client;
    use std::time::Duration;

    #[test]
    fn host_authorities_are_normalized_and_reject_ambiguous_forms() {
        assert_eq!(
            normalize_authority("LOCALHOST:08475"),
            Some("localhost:8475".into())
        );
        assert_eq!(
            normalize_authority("[0:0:0:0:0:0:0:1]:8475"),
            Some("[::1]:8475".into())
        );
        for invalid in [
            "",
            "*",
            "https://localhost:8475",
            "localhost/path",
            "user@localhost:8475",
            "localhost:0",
            "[::1",
            "127.0.0.999",
        ] {
            assert_eq!(
                normalize_authority(invalid),
                None,
                "invalid authority {invalid:?}"
            );
        }
    }

    #[test]
    fn allowlist_validation_bounds_and_rejects_invalid_entries() {
        assert!(validate_allowed_hosts(None).is_ok());
        assert!(validate_allowed_hosts(Some(&["api.example:443".into()])).is_ok());
        assert!(validate_allowed_hosts(Some(&[])).is_err());
        assert!(validate_allowed_hosts(Some(&["https://api.example".into()])).is_err());
        let too_many = vec!["api.example".to_owned(); MAX_ALLOWED_HOSTS + 1];
        assert!(validate_allowed_hosts(Some(&too_many)).is_err());
    }

    #[test]
    fn public_token_strength_counts_non_whitespace_bytes() {
        assert!(!has_public_token_strength("short"));
        assert!(!has_public_token_strength(
            "                                "
        ));
        assert!(has_public_token_strength(
            "12345678901234567890123456789012"
        ));
        assert!(has_public_token_strength(
            "12345678901234567890123456789012 "
        ));
    }

    #[test]
    fn auth_failure_budget_is_per_ip_bounded_and_has_no_shared_fallback_lockout() {
        let peer: IpAddr = "198.51.100.1".parse().unwrap();
        let mut budget = AuthFailureBudget::default();
        for _ in 0..AUTH_FAILURE_BURST {
            assert_eq!(
                budget.record_failure(Some(peer)),
                AuthFailureDisposition::Reject
            );
        }
        assert_eq!(
            budget.record_failure(Some(peer)),
            AuthFailureDisposition::Throttled
        );
        assert_eq!(
            budget.record_failure(None),
            AuthFailureDisposition::FallbackDelay
        );
        assert_eq!(
            budget.record_failure(Some("198.51.100.2".parse().unwrap()),),
            AuthFailureDisposition::Reject
        );

        let mut full = AuthFailureBudget::default();
        for index in 1..=AUTH_FAILURE_PEER_CAPACITY {
            let peer = IpAddr::V4(Ipv4Addr::from(
                u32::try_from(index).expect("test index fits in an IPv4 address"),
            ));
            assert_eq!(
                full.record_failure(Some(peer)),
                AuthFailureDisposition::Reject
            );
        }
        assert_eq!(full.peers.len(), AUTH_FAILURE_PEER_CAPACITY);
        assert_eq!(
            full.record_failure(Some("203.0.113.5".parse().unwrap())),
            AuthFailureDisposition::FallbackDelay
        );
        assert_eq!(full.peers.len(), AUTH_FAILURE_PEER_CAPACITY);
    }

    #[test]
    fn native_fault_path_components_decode_once_and_reject_malformed_input() {
        assert_eq!(decode_path_component("a%2Fb%25").unwrap(), "a/b%");
        assert_eq!(decode_path_component("%252F").unwrap(), "%2F");
        assert!(decode_path_component("bad%2").is_err());
        assert!(decode_path_component("%FF").is_err());
    }

    #[test]
    fn admin_config_debug_redacts_tokens() {
        let config = AdminConfig {
            auth_token: Some("admin-secret".into()),
            ..AdminConfig::default()
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("admin-secret"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn health_route_uses_eggserve_and_json_contract() {
        let mut handle = NativeAdmin::start(
            AdminConfig {
                bind: "127.0.0.1:0".parse().unwrap(),
                ..AdminConfig::default()
            },
            ControlState::default(),
        )
        .await
        .unwrap();
        let client = Client::builder().build();
        let mut response = client
            .get(&format!("http://{}/v1/health", handle.local_addr()))
            .unwrap()
            .send()
            .await
            .unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert_eq!(value["running"], true);
        handle.shutdown();
        handle.wait().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    // M059: end-to-end datagram HTTP lifecycle; splitting would fork the
    // shared-runtime assertion sequence. The 0.5 assertion is the exact
    // configured loss probability round-trip.
    #[allow(clippy::too_many_lines, clippy::float_cmp)]
    async fn datagram_http_resources_mutate_the_shared_runtime_and_reset_globally() {
        let target = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let target_addr = target.local_addr().unwrap();
        let state = ControlState::default();
        let mut admin = NativeAdmin::start(
            AdminConfig {
                bind: "127.0.0.1:0".parse().unwrap(),
                ..AdminConfig::default()
            },
            state.clone(),
        )
        .await
        .unwrap();
        let client = Client::builder().build();
        let base = format!("http://{}", admin.local_addr());
        let mut response = client
            .post(&format!("{base}/v1/datagram-proxies"))
            .unwrap()
            .json(&serde_json::json!({"name":"dns","listen":"127.0.0.1:0","upstream":target_addr}))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
        let created: serde_json::Value =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert!(created["proxy"]["running"].as_bool().unwrap());
        let udp_client = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        udp_client
            .send_to(
                b"metadata-only",
                created["proxy"]["bound_addr"].as_str().unwrap(),
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if !state.datagram_associations().await.is_empty() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let mut response = client
            .get(&format!("{base}/v1/datagram-associations"))
            .unwrap()
            .send()
            .await
            .unwrap();
        let associations: serde_json::Value =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        let association_id = associations[0]["id"].as_u64().unwrap();
        let response = client
            .get(&format!("{base}/v1/datagram-associations/{association_id}"))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let response = client
            .delete(&format!("{base}/v1/datagram-associations/{association_id}"))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let response = client.post(&format!("{base}/v1/datagram-proxies/dns/faults")).unwrap()
            .json(&serde_json::json!({"direction":"upstream","id":"loss","probability":1.0,"kind":{"type":"loss"}})).unwrap().send().await.unwrap();
        assert_eq!(response.status(), 201);
        assert_eq!(
            state
                .get_datagram_plan("dns", Direction::Upstream)
                .await
                .unwrap()
                .0
                .faults()
                .len(),
            1
        );
        let response = client
            .patch(&format!("{base}/v1/datagram-proxies/dns/faults/loss"))
            .unwrap()
            .json(&serde_json::json!({}))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
        let response = client
            .patch(&format!("{base}/v1/datagram-proxies/dns/faults/loss"))
            .unwrap()
            .json(&serde_json::json!({"probability":0.5}))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            state
                .get_datagram_plan("dns", Direction::Upstream)
                .await
                .unwrap()
                .0
                .faults()[0]
                .probability
                .get(),
            0.5
        );
        let response = client
            .get(&format!("{base}/v1/datagram-proxies/dns/faults/loss"))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let response = client
            .delete(&format!("{base}/v1/datagram-proxies/dns/faults/loss"))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(state
            .get_datagram_plan("dns", Direction::Upstream)
            .await
            .unwrap()
            .0
            .faults()
            .is_empty());
        let response = client
            .patch(&format!("{base}/v1/datagram-proxies/dns"))
            .unwrap()
            .json(&serde_json::json!({"enabled":false}))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(!state.get_datagram_proxy("dns").await.unwrap().running);
        let mut response = client
            .get(&format!("{base}/metrics"))
            .unwrap()
            .send()
            .await
            .unwrap();
        let metrics = String::from_utf8(response.bytes().await.unwrap().to_vec()).unwrap();
        assert!(metrics.contains("eggchaos_datagram_associations_active"));
        assert!(!metrics.contains("association=\""));
        assert!(!metrics.contains("client=\""));
        let response = client
            .post(&format!("{base}/v1/reset"))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(state.get_datagram_proxy("dns").await.unwrap().running);
        assert!(state
            .get_datagram_plan("dns", Direction::Upstream)
            .await
            .unwrap()
            .0
            .faults()
            .is_empty());
        let response = client
            .delete(&format!("{base}/v1/datagram-proxies/dns"))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        admin.shutdown();
        admin.wait().await;
        state.shutdown_and_join().await;
    }
}
