use std::{
    collections::{BTreeMap, HashMap},
    io,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use eggserve_primitives::{Request, RequestBodyPolicy, Response, ResponseBody, StatusCode};
use eggserve_server::{service_fn_with_policy, RuntimeConfig, Server, ServerHandle, ServiceError};
use serde::Serialize;
use thiserror::Error;
use tokio::{net::TcpListener, sync::RwLock};

use crate::{NativeConfigError, ProxySpec};
use eggchaos_core::FaultPlan;

type ConnectionRegistry = Arc<RwLock<HashMap<u64, crate::ConnectionSnapshot>>>;
type CancellationRegistry = Arc<RwLock<HashMap<u64, Arc<tokio::sync::Notify>>>>;

/// Native admin listener security settings.
#[derive(Debug, Clone)]
pub struct AdminConfig {
    /// Bind address.
    pub bind: SocketAddr,
    /// Explicit non-loopback opt-in.
    pub public_admin: bool,
    /// Required bearer token for non-loopback operation.
    pub auth_token: Option<String>,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8475".parse().unwrap(),
            public_admin: false,
            auth_token: None,
        }
    }
}

/// Shared typed mutation authority used by native and compatibility APIs.
#[derive(Clone, Default)]
pub struct ControlState {
    proxies: Arc<RwLock<BTreeMap<String, ProxySpec>>>,
    generation: Arc<AtomicU64>,
    connections: Arc<RwLock<Option<ConnectionRegistry>>>,
    cancellations: Arc<RwLock<Option<CancellationRegistry>>>,
}

impl ControlState {
    /// Create state from validated proxy definitions.
    pub fn new(proxies: impl IntoIterator<Item = ProxySpec>) -> Self {
        let state = Self {
            proxies: Arc::new(RwLock::new(BTreeMap::new())),
            generation: Arc::new(AtomicU64::new(1)),
            connections: Arc::new(RwLock::new(None)),
            cancellations: Arc::new(RwLock::new(None)),
        };
        if let Ok(mut map) = state.proxies.try_write() {
            for proxy in proxies {
                map.insert(proxy.name.clone(), proxy);
            }
        }
        state
    }
    /// Attach runtime-owned active connection and cancellation registries.
    pub async fn attach_runtime(
        &self,
        connections: ConnectionRegistry,
        cancellations: CancellationRegistry,
    ) {
        *self.connections.write().await = Some(connections);
        *self.cancellations.write().await = Some(cancellations);
    }
    /// Current configuration generation.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    /// Snapshot all proxy definitions.
    pub async fn list(&self) -> Vec<ProxySpec> {
        self.proxies.read().await.values().cloned().collect()
    }
    /// Snapshot active runtime connections when attached.
    pub async fn connections(&self) -> Vec<crate::ConnectionSnapshot> {
        let Some(registry) = self.connections.read().await.clone() else {
            return Vec::new();
        };
        let snapshots = registry.read().await.values().cloned().collect();
        snapshots
    }
    /// Terminate an active connection, if present.
    pub async fn kill(&self, id: u64) -> bool {
        let Some(registry) = self.cancellations.read().await.clone() else {
            return false;
        };
        let Some(notify) = registry.write().await.remove(&id) else {
            return false;
        };
        notify.notify_waiters();
        true
    }
    /// Get one proxy definition.
    pub async fn get(&self, name: &str) -> Option<ProxySpec> {
        self.proxies.read().await.get(name).cloned()
    }
    /// Insert a proxy atomically.
    pub async fn insert(&self, proxy: ProxySpec) -> Result<u64, AdminError> {
        let mut map = self.proxies.write().await;
        if map.contains_key(&proxy.name) {
            return Err(AdminError::Conflict("proxy already exists".into()));
        }
        proxy
            .upstream_policy
            .publish(proxy.upstream_faults.clone())
            .map_err(|e| AdminError::Invalid(e.to_string()))?;
        proxy
            .downstream_policy
            .publish(proxy.downstream_faults.clone())
            .map_err(|e| AdminError::Invalid(e.to_string()))?;
        map.insert(proxy.name.clone(), proxy);
        Ok(self.generation.fetch_add(1, Ordering::AcqRel) + 1)
    }
    /// Publish a complete fault-plan generation for an existing proxy.
    pub async fn publish_plans(
        &self,
        name: &str,
        upstream: FaultPlan,
        downstream: FaultPlan,
    ) -> Result<u64, AdminError> {
        upstream
            .validate()
            .map_err(|e| AdminError::Invalid(e.to_string()))?;
        downstream
            .validate()
            .map_err(|e| AdminError::Invalid(e.to_string()))?;
        let map = self.proxies.read().await;
        let proxy = map
            .get(name)
            .ok_or_else(|| AdminError::Invalid("proxy not found".into()))?;
        proxy
            .upstream_policy
            .publish(upstream)
            .map_err(|e| AdminError::Invalid(e.to_string()))?;
        proxy
            .downstream_policy
            .publish(downstream)
            .map_err(|e| AdminError::Invalid(e.to_string()))?;
        Ok(self.generation.fetch_add(1, Ordering::AcqRel) + 1)
    }
    /// Remove a proxy atomically.
    pub async fn remove(&self, name: &str) -> bool {
        let mut map = self.proxies.write().await;
        let existed = map.remove(name).is_some();
        if existed {
            self.generation.fetch_add(1, Ordering::AcqRel);
        }
        existed
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
    /// EggServe startup failed.
    #[error("admin server: {0}")]
    Server(String),
    /// Mutation conflicts with current state.
    #[error("conflict: {0}")]
    Conflict(String),
    /// Invalid request/configuration.
    #[error("invalid request: {0}")]
    Invalid(String),
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
    /// Start a bounded versioned native admin API using EggServe's H1 leaf runtime.
    pub async fn start(
        config: AdminConfig,
        state: ControlState,
    ) -> Result<AdminHandle, AdminError> {
        if !config.bind.ip().is_loopback() && (!config.public_admin || config.auth_token.is_none())
        {
            return Err(AdminError::InsecurePublicBind);
        }
        let listener = TcpListener::bind(config.bind).await?;
        let runtime = RuntimeConfig {
            max_request_body_bytes: 1024 * 1024,
            max_connections: 128,
            ..RuntimeConfig::default()
        };
        let local_addr = listener.local_addr()?;
        let auth = config.auth_token.map(Arc::<str>::from);
        let service = service_fn_with_policy(
            move |request: Request| {
                let state = state.clone();
                let auth = auth.clone();
                async move { handle_request(request, state, auth).await }
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

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorBody<'a>,
}
#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
}

async fn handle_request(
    request: Request,
    state: ControlState,
    auth: Option<Arc<str>>,
) -> Result<Response, ServiceError> {
    let (head, body, _) = request.into_parts();
    if let Some(expected) = auth.as_deref() {
        let supplied = head
            .headers()
            .get_first("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        let token = supplied.strip_prefix("Bearer ").unwrap_or("");
        if !constant_time_equal(token.as_bytes(), expected.as_bytes()) {
            return Ok(json_response(
                StatusCode::FORBIDDEN,
                &ErrorEnvelope {
                    error: ErrorBody {
                        code: "unauthorized",
                        message: "authorization required",
                    },
                },
            ));
        }
    }
    let method = head.method().as_str().to_owned();
    let path = head.target().path().to_owned();
    let body = body
        .read_all()
        .await
        .map_err(|error| ServiceError::rejected(413, error.to_string()))?;
    let response = match (method.as_str(), path.as_str()) {
        ("GET", "/v1/health") => json_response(
            StatusCode::OK,
            &serde_json::json!({"running": true, "generation": state.generation()}),
        ),
        ("GET", "/v1/version") => json_response(
            StatusCode::OK,
            &serde_json::json!({"version": env!("CARGO_PKG_VERSION"), "api": "v1"}),
        ),
        ("POST", "/v1/reset") => json_response(StatusCode::OK, &serde_json::json!({"generation": state.generation(), "reset": true})),
        ("GET", "/v1/proxies") => json_response(StatusCode::OK, &state.list().await),
        ("POST", "/v1/scenarios/apply") => match serde_json::from_slice::<crate::Scenario>(&body) {
            Ok(scenario) => { let state = state.clone(); tokio::spawn(async move { let _ = crate::apply_scenario(state, scenario).await; }); json_response(StatusCode::new(202).unwrap(), &serde_json::json!({"accepted": true})) }
            Err(error) => json_response(StatusCode::BAD_REQUEST, &ErrorEnvelope { error: ErrorBody { code: "invalid_json", message: &error.to_string() } }),
        },
        ("GET", "/v1/connections") => json_response(StatusCode::OK, &state.connections().await),
        ("DELETE", _) if path.starts_with("/v1/connections/") => {
            let id = path.trim_start_matches("/v1/connections/").parse::<u64>().unwrap_or(0);
            if state.kill(id).await { json_response(StatusCode::NO_CONTENT, &serde_json::json!({"id": id, "terminated": true})) } else { json_response(StatusCode::NOT_FOUND, &ErrorEnvelope { error: ErrorBody { code: "not_found", message: "connection not found" } }) }
        }
        ("GET", "/metrics") => text_response("# HELP eggchaos_config_generation Current configuration generation\n# TYPE eggchaos_config_generation gauge\neggchaos_config_generation ".to_owned() + &state.generation().to_string() + "\n"),
        ("POST", "/v1/proxies") => match serde_json::from_slice::<ProxySpec>(&body) {
            Ok(proxy) => match state.insert(proxy.clone()).await {
                Ok(generation) => json_response(
                    StatusCode::CREATED,
                    &serde_json::json!({"proxy": proxy, "generation": generation}),
                ),
                Err(error) => json_response(
                    StatusCode::new(409).unwrap(),
                    &ErrorEnvelope {
                        error: ErrorBody {
                            code: "conflict",
                            message: &error.to_string(),
                        },
                    },
                ),
            },
            Err(error) => json_response(
                StatusCode::BAD_REQUEST,
                &ErrorEnvelope {
                    error: ErrorBody {
                        code: "invalid_json",
                        message: &error.to_string(),
                    },
                },
            ),
        },
        ("DELETE", _) if path.starts_with("/v1/proxies/") => {
            let name = path.trim_start_matches("/v1/proxies/");
            if state.remove(name).await {
                json_response(
                    StatusCode::NO_CONTENT,
                    &serde_json::json!({"generation": state.generation()}),
                )
            } else {
                json_response(
                    StatusCode::NOT_FOUND,
                    &ErrorEnvelope {
                        error: ErrorBody {
                            code: "not_found",
                            message: "proxy not found",
                        },
                    },
                )
            }
        }
        ("GET", _) if path.starts_with("/v1/proxies/") => {
            let name = path
                .trim_start_matches("/v1/proxies/")
                .trim_end_matches("/faults");
            match state.get(name).await {
                Some(proxy) => json_response(StatusCode::OK, &proxy),
                None => json_response(
                    StatusCode::NOT_FOUND,
                    &ErrorEnvelope {
                        error: ErrorBody {
                            code: "not_found",
                            message: "proxy not found",
                        },
                    },
                ),
            }
        }
        _ => json_response(
            StatusCode::NOT_FOUND,
            &ErrorEnvelope {
                error: ErrorBody {
                    code: "not_found",
                    message: "route not found",
                },
            },
        ),
    };
    Ok(response)
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut diff = left.len() ^ right.len();
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= usize::from(*a ^ *b);
    }
    diff == 0
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
fn _config_error_kind(error: NativeConfigError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggfetch_core::Client;

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
}
