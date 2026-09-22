use std::{io, net::SocketAddr, sync::Arc};

use eggserve_primitives::{Request, RequestBodyPolicy, Response, ResponseBody, StatusCode};
use eggserve_server::{service_fn_with_policy, RuntimeConfig, Server, ServerHandle, ServiceError};
use serde::Serialize;
use thiserror::Error;
use tokio::net::TcpListener;

use crate::{ControlError, ControlState, NativeConfigError};

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
        &ErrorEnvelope {
            error: ErrorBody {
                code: error.code(),
                message: &error.to_string(),
            },
        },
    )
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
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    let response = match route(&method, &segments, &body, &state).await {
        RouteOutcome::Response(response) => response,
        RouteOutcome::ControlError(error) => control_error_response(&error),
        RouteOutcome::BadJson(error) => json_response(
            StatusCode::BAD_REQUEST,
            &ErrorEnvelope {
                error: ErrorBody {
                    code: "invalid_json",
                    message: &error,
                },
            },
        ),
        RouteOutcome::NotFound => json_response(
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

enum RouteOutcome {
    Response(Response),
    ControlError(ControlError),
    BadJson(String),
    NotFound,
}

fn parse_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, RouteOutcome> {
    serde_json::from_slice(body).map_err(|error| RouteOutcome::BadJson(error.to_string()))
}

async fn route(method: &str, segments: &[&str], body: &[u8], state: &ControlState) -> RouteOutcome {
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
        ("POST", ["v1", "reset"]) => match state.reset().await {
            Ok(report) => RouteOutcome::Response(json_response(StatusCode::OK, &report)),
            Err(error) => RouteOutcome::ControlError(error),
        },
        ("GET", ["v1", "proxies"]) => {
            RouteOutcome::Response(json_response(StatusCode::OK, &state.list().await))
        }
        ("POST", ["v1", "proxies"]) => {
            let spec: crate::ProxySpec = match parse_json(body) {
                Ok(spec) => spec,
                Err(outcome) => return outcome,
            };
            match state.create_proxy(spec).await {
                Ok((view, generation)) => RouteOutcome::Response(json_response(
                    StatusCode::CREATED,
                    &serde_json::json!({"proxy": view, "generation": generation}),
                )),
                Err(error) => RouteOutcome::ControlError(error),
            }
        }
        ("GET", ["v1", "proxies", name]) => match state.get(name).await {
            Some(view) => RouteOutcome::Response(json_response(StatusCode::OK, &view)),
            None => RouteOutcome::ControlError(ControlError::NotFound((*name).to_owned())),
        },
        ("PATCH", ["v1", "proxies", name]) => {
            let patch: crate::ProxyPatch = match parse_json(body) {
                Ok(patch) => patch,
                Err(outcome) => return outcome,
            };
            match state.update_proxy(name, patch).await {
                Ok((view, generation)) => RouteOutcome::Response(json_response(
                    StatusCode::OK,
                    &serde_json::json!({"proxy": view, "generation": generation}),
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
                &serde_json::json!({"upstream": upstream, "downstream": downstream}),
            )),
            None => RouteOutcome::ControlError(ControlError::NotFound((*name).to_owned())),
        },
        ("POST", ["v1", "proxies", name, "faults"]) => {
            let upsert: crate::FaultUpsert = match parse_json(body) {
                Ok(upsert) => upsert,
                Err(outcome) => return outcome,
            };
            match state.add_fault(name, upsert).await {
                Ok((direction, fault, generation)) => RouteOutcome::Response(json_response(
                    StatusCode::CREATED,
                    &serde_json::json!({
                        "direction": direction.as_str(),
                        "fault": fault,
                        "generation": generation,
                    }),
                )),
                Err(error) => RouteOutcome::ControlError(error),
            }
        }
        ("GET", ["v1", "proxies", name, "faults", id]) => match state.get_fault(name, id).await {
            Some((direction, fault)) => RouteOutcome::Response(json_response(
                StatusCode::OK,
                &serde_json::json!({"direction": direction.as_str(), "fault": fault}),
            )),
            None => {
                RouteOutcome::ControlError(ControlError::NotFound(format!("fault {id} on {name}")))
            }
        },
        ("PATCH", ["v1", "proxies", name, "faults", id]) => {
            let patch: crate::FaultPatch = match parse_json(body) {
                Ok(patch) => patch,
                Err(outcome) => return outcome,
            };
            match state.update_fault(name, id, patch).await {
                Ok((direction, fault, generation)) => RouteOutcome::Response(json_response(
                    StatusCode::OK,
                    &serde_json::json!({
                        "direction": direction.as_str(),
                        "fault": fault,
                        "generation": generation,
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
            Ok(id) => match state.get_connection(id).await {
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
                if state.kill(id).await {
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
        ("POST", ["v1", "scenarios", "apply"]) => {
            match serde_json::from_slice::<crate::Scenario>(body) {
                Ok(scenario) => match state.start_scenario(scenario).await {
                    Ok(record) => RouteOutcome::Response(json_response(
                        StatusCode::new(202).expect("accepted status"),
                        &serde_json::json!({
                            "run_id": record.run_id,
                            "seed": record.seed,
                            "status": record.status,
                        }),
                    )),
                    Err(error) => {
                        RouteOutcome::ControlError(crate::ControlError::Invalid(error.to_string()))
                    }
                },
                Err(error) => RouteOutcome::BadJson(error.to_string()),
            }
        }
        ("GET", ["v1", "scenarios", id]) => match id.parse::<u64>() {
            Ok(run_id) => match state.get_scenario(run_id).await {
                Some(record) => RouteOutcome::Response(json_response(StatusCode::OK, &record)),
                None => RouteOutcome::ControlError(crate::ControlError::NotFound(format!(
                    "scenario run {run_id}"
                ))),
            },
            Err(_) => RouteOutcome::ControlError(crate::ControlError::Invalid(
                "scenario run id must be an integer".into(),
            )),
        },
        ("DELETE", ["v1", "scenarios", id]) => match id.parse::<u64>() {
            Ok(run_id) => match state.cancel_scenario(run_id).await {
                Some(record) => RouteOutcome::Response(json_response(StatusCode::OK, &record)),
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
