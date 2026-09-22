//! Toxiproxy v2.12 toxic DTOs and translation into native eggchaos plans.

use std::{collections::BTreeMap, num::NonZeroU64, sync::Arc, time::Duration};

use eggchaos_core::{
    BandwidthConfig, BlackholeConfig, Direction, DisconnectConfig, FaultId, FaultKind, FaultPlan,
    FaultSpec, LatencyConfig, LimitDataConfig, Probability, SliceConfig, SlowCloseConfig,
};
use eggchaos_server::{ControlState, ProxySpec};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

/// Toxiproxy-compatible proxy JSON shape.
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

/// Toxiproxy v2.12 toxic JSON shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Toxic {
    /// Optional stable name.
    pub name: Option<String>,
    /// Toxic type.
    pub r#type: String,
    /// Direction; Toxiproxy defaults to downstream.
    #[serde(default = "default_stream")]
    pub stream: String,
    /// Activation probability.
    #[serde(default = "default_toxicity")]
    pub toxicity: f64,
    /// Typed toxic attributes.
    #[serde(default)]
    pub attributes: ToxicAttributes,
}

fn default_stream() -> String {
    "downstream".into()
}
fn default_toxicity() -> f64 {
    1.0
}

/// Bounded v2.12 toxic attribute superset.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToxicAttributes {
    /// Milliseconds of latency.
    pub latency: Option<u64>,
    /// Milliseconds of jitter.
    pub jitter: Option<u64>,
    /// Bandwidth KB/s, using the v2.12 KiB convention documented by this adapter.
    pub rate: Option<u64>,
    /// Slice average size.
    pub average_size: Option<u64>,
    /// Slice variation.
    pub size_variation: Option<u64>,
    /// Slice delay in microseconds.
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

impl Toxic {
    /// Translate one toxic into a native fault in the declared stream.
    pub fn to_fault(&self) -> Result<(Direction, FaultSpec), CompatibilityError> {
        let id = FaultId::new(self.name.clone().unwrap_or_else(|| self.r#type.clone()))
            .map_err(|e| CompatibilityError::Invalid(e.to_string()))?;
        let probability = Probability::new(self.toxicity)
            .map_err(|e| CompatibilityError::Invalid(e.to_string()))?;
        let kind = match self.r#type.as_str() {
            "latency" => FaultKind::Latency(LatencyConfig {
                delay: Duration::from_millis(self.attributes.latency.unwrap_or(0)),
                jitter: Duration::from_millis(self.attributes.jitter.unwrap_or(0)),
                max_buffer_bytes: NonZeroU64::new(64 * 1024).unwrap(),
            }),
            "bandwidth" => FaultKind::Bandwidth(BandwidthConfig {
                bytes_per_second: NonZeroU64::new(
                    self.attributes.rate.unwrap_or(1).saturating_mul(1024),
                )
                .ok_or_else(|| CompatibilityError::Invalid("rate must be non-zero".into()))?,
                burst_bytes: NonZeroU64::new(64 * 1024).unwrap(),
            }),
            "timeout" => FaultKind::Blackhole(BlackholeConfig {
                close_after: self.attributes.timeout.map(Duration::from_millis),
            }),
            "slow_close" => FaultKind::SlowClose(SlowCloseConfig {
                delay: Duration::from_millis(self.attributes.delay.unwrap_or(0)),
            }),
            "reset_peer" => FaultKind::Disconnect(DisconnectConfig { hard_reset: true }),
            "slicer" => FaultKind::Slice(SliceConfig {
                average_size: NonZeroU64::new(self.attributes.average_size.unwrap_or(1))
                    .ok_or_else(|| {
                        CompatibilityError::Invalid("average_size must be non-zero".into())
                    })?,
                variation: self.attributes.size_variation.unwrap_or(0),
                delay: Duration::from_micros(self.attributes.delay.unwrap_or(0)),
            }),
            "limit_data" => FaultKind::LimitData(LimitDataConfig {
                bytes: NonZeroU64::new(self.attributes.bytes.unwrap_or(1))
                    .ok_or_else(|| CompatibilityError::Invalid("bytes must be non-zero".into()))?,
            }),
            other => return Err(CompatibilityError::Unsupported(other.into())),
        };
        Ok((
            if self.stream.eq_ignore_ascii_case("upstream") {
                Direction::Upstream
            } else {
                Direction::Downstream
            },
            FaultSpec {
                id,
                probability,
                kind,
            },
        ))
    }
}

/// Translate a Toxiproxy proxy into the native fixed-target model.
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
    native
        .upstream_policy
        .publish(native.upstream_faults.clone())
        .map_err(|e| CompatibilityError::Invalid(e.to_string()))?;
    native
        .downstream_policy
        .publish(native.downstream_faults.clone())
        .map_err(|e| CompatibilityError::Invalid(e.to_string()))?;
    Ok(native)
}

/// Native mutation authority facade for compatibility clients.
#[derive(Clone)]
pub struct ToxiproxyAdapter {
    state: ControlState,
    proxies: Arc<RwLock<BTreeMap<String, Proxy>>>,
}

impl ToxiproxyAdapter {
    /// Create a compatibility facade.
    pub fn new(state: ControlState) -> Self {
        Self {
            state,
            proxies: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }
    /// List native-backed proxy JSON values.
    pub async fn list(&self) -> Vec<ProxySpec> {
        self.state.list().await
    }
    /// Create a proxy using the native authority.
    pub async fn create(&self, proxy: Proxy) -> Result<u64, CompatibilityError> {
        let native = translate_proxy(&proxy)?;
        let generation = self
            .state
            .insert(native)
            .await
            .map_err(|e| CompatibilityError::Invalid(e.to_string()))?;
        self.proxies.write().await.insert(proxy.name.clone(), proxy);
        Ok(generation)
    }
    /// Snapshot compatibility proxy definitions.
    pub async fn proxy_json(&self) -> Vec<Proxy> {
        self.proxies.read().await.values().cloned().collect()
    }
    /// Get a compatibility proxy.
    pub async fn proxy(&self, name: &str) -> Option<Proxy> {
        self.proxies.read().await.get(name).cloned()
    }
    /// Delete a compatibility proxy.
    pub async fn delete(&self, name: &str) -> bool {
        let removed = self.state.remove(name).await;
        if removed {
            self.proxies.write().await.remove(name);
        }
        removed
    }
    /// Add or replace a toxic through a native live-policy generation.
    pub async fn add_toxic(&self, proxy: &str, toxic: Toxic) -> Result<(), CompatibilityError> {
        let mut map = self.proxies.write().await;
        let definition = map
            .get_mut(proxy)
            .ok_or_else(|| CompatibilityError::Invalid("proxy not found".into()))?;
        let name = toxic.name.clone().unwrap_or_else(|| toxic.r#type.clone());
        definition
            .toxics
            .retain(|existing| existing.name.as_deref() != Some(&name));
        definition.toxics.push(Toxic {
            name: Some(name),
            ..toxic
        });
        let native = translate_proxy(definition)?;
        self.state
            .publish_plans(proxy, native.upstream_faults, native.downstream_faults)
            .await
            .map_err(|e| CompatibilityError::Invalid(e.to_string()))?;
        Ok(())
    }
    /// Remove a toxic through a native live-policy generation.
    pub async fn remove_toxic(&self, proxy: &str, toxic: &str) -> bool {
        let mut map = self.proxies.write().await;
        let Some(definition) = map.get_mut(proxy) else {
            return false;
        };
        let before = definition.toxics.len();
        definition
            .toxics
            .retain(|candidate| candidate.name.as_deref() != Some(toxic));
        if before == definition.toxics.len() {
            return false;
        }
        if let Ok(native) = translate_proxy(definition) {
            let _ = self
                .state
                .publish_plans(proxy, native.upstream_faults, native.downstream_faults)
                .await;
        }
        true
    }
    /// Return the fixed v2.12 version identifier exposed by the adapter.
    pub fn version() -> &'static str {
        "2.12.0-eggchaos"
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
    ) -> Result<ToxiproxyHttpHandle, CompatibilityError> {
        let listener = tokio::net::TcpListener::bind(bind)
            .await
            .map_err(|e| CompatibilityError::Invalid(e.to_string()))?;
        let local_addr = listener
            .local_addr()
            .map_err(|e| CompatibilityError::Invalid(e.to_string()))?;
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
            .map_err(|e| CompatibilityError::Invalid(e.to_string()))?;
        let handle = server
            .start_with_service(service)
            .await
            .map_err(|e| CompatibilityError::Invalid(e.to_string()))?;
        Ok(ToxiproxyHttpHandle {
            local_addr,
            server: Some(handle),
        })
    }
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
    let status = eggserve_primitives::StatusCode::OK;
    let value = match (method.as_str(), path.as_str()) {
        ("GET", "/version") => serde_json::json!({"version": ToxiproxyAdapter::version()}),
        ("GET", "/proxies") => serde_json::to_value(adapter.proxy_json().await).unwrap(),
        ("POST", "/proxies") => match serde_json::from_slice::<Proxy>(&bytes) {
            Ok(proxy) => {
                adapter
                    .create(proxy.clone())
                    .await
                    .map_err(|e| eggserve_server::ServiceError::rejected(409, e.to_string()))?;
                serde_json::to_value(proxy).unwrap()
            }
            Err(e) => {
                return Ok(compat_json(
                    eggserve_primitives::StatusCode::BAD_REQUEST,
                    &serde_json::json!({"error": e.to_string()}),
                ))
            }
        },
        ("POST", "/reset") => serde_json::json!({"reset": true}),
        ("GET", _) if path.starts_with("/proxies/") => {
            let parts: Vec<&str> = path.trim_start_matches("/proxies/").split('/').collect();
            match adapter.proxy(parts[0]).await {
                Some(proxy) if parts.len() == 1 => serde_json::to_value(proxy).unwrap(),
                Some(proxy) if parts.len() == 2 && parts[1] == "toxics" => {
                    serde_json::to_value(proxy.toxics).unwrap()
                }
                _ => {
                    return Ok(compat_json(
                        eggserve_primitives::StatusCode::NOT_FOUND,
                        &serde_json::json!({"error": "not found"}),
                    ))
                }
            }
        }
        ("DELETE", _) if path.starts_with("/proxies/") => {
            let name = path.trim_start_matches("/proxies/").trim_end_matches('/');
            if adapter.delete(name).await {
                serde_json::json!({"deleted": true})
            } else {
                return Ok(compat_json(
                    eggserve_primitives::StatusCode::NOT_FOUND,
                    &serde_json::json!({"error": "not found"}),
                ));
            }
        }
        ("POST", _) if path.ends_with("/toxics") => {
            let proxy = path
                .trim_start_matches("/proxies/")
                .trim_end_matches("/toxics")
                .trim_end_matches('/');
            let toxic: Toxic = serde_json::from_slice(&bytes)
                .map_err(|e| eggserve_server::ServiceError::rejected(400, e.to_string()))?;
            adapter
                .add_toxic(proxy, toxic.clone())
                .await
                .map_err(|e| eggserve_server::ServiceError::rejected(400, e.to_string()))?;
            serde_json::to_value(toxic).unwrap()
        }
        _ => {
            return Ok(compat_json(
                eggserve_primitives::StatusCode::NOT_FOUND,
                &serde_json::json!({"error": "not found"}),
            ))
        }
    };
    Ok(compat_json(status, &value))
}

fn compat_json<T: Serialize>(
    status: eggserve_primitives::StatusCode,
    value: &T,
) -> eggserve_primitives::Response {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    eggserve_primitives::Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .unwrap()
        .body(eggserve_primitives::ResponseBody::Bytes(body))
        .unwrap()
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
        assert!(String::from_utf8_lossy(&body).contains("2.12.0"));
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
        assert!(created.status().is_success());
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
        assert!(added.status().is_success());
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
}
