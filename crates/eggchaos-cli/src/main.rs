#![forbid(unsafe_code)]
use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use eggchaos_server::{
    runtime_admission_limits, runtime_datagram_limits, AdminConfig, FaultKindV1, NativeAdmin,
    NativeConfig, ServiceBuilder, NATIVE_DEFAULT_BUFFER_BYTES, NATIVE_DEFAULT_PROXY_TIMEOUT_MS,
};
use eggfetch_core::Client;
use tokio::io::AsyncReadExt;

#[derive(Parser)]
#[command(
    name = "eggchaos",
    version,
    about = "Fixed-target deterministic chaos proxy"
)]
struct Cli {
    /// Native admin endpoint.
    #[arg(long, default_value = "http://127.0.0.1:8475")]
    admin: String,
    /// Bearer token for a secured native admin endpoint (overrides EGGCHAOS_ADMIN_TOKEN).
    #[arg(long, env = "EGGCHAOS_ADMIN_TOKEN")]
    admin_token: Option<String>,
    /// Emit one machine-readable JSON document.
    #[arg(long)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start a service from schema-v1 TOML.
    Serve {
        #[arg(long, default_value = "eggchaos.toml")]
        config: PathBuf,
    },
    /// Operate on registered proxies.
    Proxy {
        #[command(subcommand)]
        command: ProxyCommand,
    },
    /// Operate on proxy fault plans.
    Fault {
        #[command(subcommand)]
        command: FaultCommand,
    },
    /// Operate on live connections.
    Connection {
        #[command(subcommand)]
        command: ConnectionCommand,
    },
    /// Operate on fixed-target UDP datagram resources.
    Datagram {
        #[command(subcommand)]
        command: DatagramCommand,
    },
    /// Apply or inspect a deterministic scenario run.
    Scenario {
        #[command(subcommand)]
        command: ScenarioCommand,
    },
    /// List retained closed-connection history.
    History,
    /// Print Prometheus metrics (`--json` wraps raw text in `{"body":...}`).
    Metrics,
    /// Print version information.
    Version,
    /// Reset the native control generation.
    Reset,
}

#[derive(Subcommand)]
enum ProxyCommand {
    /// List proxies.
    List,
    /// Get one proxy.
    Get { name: String },
    /// Add a proxy and bind its listener.
    Add {
        name: String,
        #[arg(long)]
        listen: SocketAddr,
        #[arg(long)]
        upstream: SocketAddr,
        #[arg(long)]
        seed: Option<u64>,
        #[arg(long)]
        max_connections: Option<usize>,
        #[arg(long)]
        connect_timeout_ms: Option<u64>,
        /// Register the proxy without starting its listener.
        #[arg(long)]
        disabled: bool,
    },
    /// Patch proxy fields. Listen/upstream changes restart the listener.
    Set {
        name: String,
        #[arg(long)]
        listen: Option<SocketAddr>,
        #[arg(long)]
        upstream: Option<SocketAddr>,
        #[arg(long, conflicts_with = "clear_max_connections")]
        max_connections: Option<usize>,
        #[arg(long)]
        clear_max_connections: bool,
        #[arg(long)]
        connect_timeout_ms: Option<u64>,
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        #[arg(long)]
        disable: bool,
    },
    /// Remove a proxy and stop its listener.
    Remove { name: String },
    /// Start a registered listener.
    Enable { name: String },
    /// Stop a registered listener, keeping its definition.
    Disable { name: String },
}

#[derive(Args)]
struct FaultParams {
    /// Fault behavior: latency, bandwidth, blackhole, limit-data,
    /// slow-close, slice, disconnect.
    #[arg(long)]
    kind: Option<String>,
    #[arg(long)]
    delay_ms: Option<u64>,
    #[arg(long)]
    jitter_ms: Option<u64>,
    #[arg(long)]
    max_buffer_bytes: Option<u64>,
    #[arg(long)]
    bytes_per_second: Option<u64>,
    #[arg(long)]
    burst_bytes: Option<u64>,
    #[arg(long)]
    close_after_ms: Option<u64>,
    #[arg(long)]
    bytes: Option<u64>,
    #[arg(long)]
    average_size: Option<u64>,
    #[arg(long)]
    variation: Option<u64>,
    #[arg(long)]
    after_ms: Option<u64>,
    #[arg(long)]
    hard_reset: bool,
}

#[derive(Subcommand)]
enum FaultCommand {
    /// List upstream and downstream faults for a proxy.
    List { proxy: String },
    /// Get one fault.
    Get { proxy: String, id: String },
    /// Add a fault to a proxy direction.
    Add {
        proxy: String,
        id: String,
        #[arg(long, default_value = "downstream")]
        direction: String,
        #[arg(long, default_value_t = 1.0)]
        probability: f64,
        #[command(flatten)]
        params: FaultParams,
    },
    /// Patch a fault's probability and/or behavior.
    Set {
        proxy: String,
        id: String,
        #[arg(long)]
        probability: Option<f64>,
        #[command(flatten)]
        params: FaultParams,
    },
    /// Remove a fault.
    Remove { proxy: String, id: String },
}

#[derive(Subcommand)]
enum ConnectionCommand {
    /// List active connections.
    List,
    /// Get one active connection.
    Get { id: u64 },
    /// Terminate one active connection.
    Kill { id: u64 },
}

#[derive(Subcommand)]
enum DatagramCommand {
    Proxy {
        #[command(subcommand)]
        command: DatagramProxyCommand,
    },
    Fault {
        #[command(subcommand)]
        command: DatagramFaultCommand,
    },
    Association {
        #[command(subcommand)]
        command: DatagramAssociationCommand,
    },
}

#[derive(Subcommand)]
enum DatagramProxyCommand {
    List,
    Get {
        name: String,
    },
    Add {
        name: String,
        #[arg(long)]
        listen: SocketAddr,
        #[arg(long)]
        upstream: SocketAddr,
        #[arg(long, default_value_t = 256)]
        max_associations: usize,
        #[arg(long, default_value_t = 60_000)]
        association_idle_timeout_ms: u64,
        #[arg(long, default_value_t = 1024)]
        max_queued_datagrams: u64,
        #[arg(long, default_value_t = 4 * 1024 * 1024)]
        max_queued_bytes: u64,
        #[arg(long, default_value_t = 65_507)]
        max_datagram_size: u64,
        #[arg(long, default_value_t = 0)]
        seed: u64,
    },
    Set {
        name: String,
        #[arg(long)]
        listen: Option<SocketAddr>,
        #[arg(long)]
        upstream: Option<SocketAddr>,
        #[arg(long)]
        max_associations: Option<usize>,
        #[arg(long)]
        association_idle_timeout_ms: Option<u64>,
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        #[arg(long)]
        disable: bool,
    },
    Enable {
        name: String,
    },
    Disable {
        name: String,
    },
    Remove {
        name: String,
    },
}

#[derive(Subcommand)]
enum DatagramFaultCommand {
    List {
        proxy: String,
    },
    Get {
        proxy: String,
        id: String,
    },
    Add {
        proxy: String,
        id: String,
        #[arg(long, default_value = "upstream")]
        direction: String,
        #[arg(long, default_value_t = 1.0)]
        probability: f64,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        delay_ns: Option<u64>,
        #[arg(long)]
        jitter_ns: Option<u64>,
        #[arg(long)]
        additional_copies: Option<u8>,
        #[arg(long)]
        hold_ns: Option<u64>,
        #[arg(long)]
        bytes: Option<u64>,
        #[arg(long)]
        bytes_per_second: Option<u64>,
        #[arg(long)]
        burst_bytes: Option<u64>,
    },
    Set {
        proxy: String,
        id: String,
        #[arg(long)]
        probability: f64,
    },
    Remove {
        proxy: String,
        id: String,
    },
}

#[derive(Subcommand)]
enum DatagramAssociationCommand {
    List,
    Get { id: u64 },
    Kill { id: u64 },
}

#[derive(Subcommand)]
enum ScenarioCommand {
    /// Apply a scenario document from a JSON file (v1) or a JSON/TOML
    /// schedule file (v2, selected by extension).
    Apply { file: PathBuf },
    /// Validate a v2 schedule file server-side without creating a run.
    Validate { file: PathBuf },
    /// Compile a v2 schedule file server-side without creating a run.
    Compile { file: PathBuf },
    /// Get a scenario run record.
    Get { run_id: u64 },
    /// Cancel a scenario run.
    Cancel { run_id: u64 },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let result = dispatch(
        cli.command,
        &cli.admin,
        cli.admin_token.as_deref(),
        cli.json,
    )
    .await;
    if let Err(error) = result {
        if cli.json {
            print_value(
                &serde_json::json!({"error": {"code": "request_failed", "message": error.to_string()}}),
                true,
            );
        } else {
            eprintln!("eggchaos: {error}");
        }
        std::process::exit(1);
    }
}

async fn dispatch(
    command: Command,
    admin: &str,
    admin_token: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Command::Serve { config } => serve(config).await,
        Command::Version => {
            print_value(
                &serde_json::json!({"version": env!("CARGO_PKG_VERSION"), "api": "v1"}),
                json,
            );
            Ok(())
        }
        Command::Reset => request(admin, admin_token, "POST", "/v1/reset", None, json).await,
        Command::History => request(admin, admin_token, "GET", "/v1/history", None, json).await,
        Command::Metrics => request(admin, admin_token, "GET", "/metrics", None, json).await,
        Command::Scenario { command } => match command {
            ScenarioCommand::Apply { file } => {
                let document = read_scenario_document(&file).await?;
                request(
                    admin,
                    admin_token,
                    "POST",
                    "/v1/scenarios/apply",
                    Some(document),
                    json,
                )
                .await
            }
            ScenarioCommand::Validate { file } => {
                let document = read_scenario_document(&file).await?;
                request(
                    admin,
                    admin_token,
                    "POST",
                    "/v1/scenarios/validate",
                    Some(document),
                    json,
                )
                .await
            }
            ScenarioCommand::Compile { file } => {
                let document = read_scenario_document(&file).await?;
                request(
                    admin,
                    admin_token,
                    "POST",
                    "/v1/scenarios/compile",
                    Some(document),
                    json,
                )
                .await
            }
            ScenarioCommand::Get { run_id } => {
                request(
                    admin,
                    admin_token,
                    "GET",
                    &format!("/v1/scenarios/{run_id}"),
                    None,
                    json,
                )
                .await
            }
            ScenarioCommand::Cancel { run_id } => {
                request(
                    admin,
                    admin_token,
                    "DELETE",
                    &format!("/v1/scenarios/{run_id}"),
                    None,
                    json,
                )
                .await
            }
        },
        Command::Proxy { command } => match command {
            ProxyCommand::List => {
                request(admin, admin_token, "GET", "/v1/proxies", None, json).await
            }
            ProxyCommand::Get { name } => {
                request(
                    admin,
                    admin_token,
                    "GET",
                    &format!("/v1/proxies/{name}"),
                    None,
                    json,
                )
                .await
            }
            ProxyCommand::Add {
                name,
                listen,
                upstream,
                seed,
                max_connections,
                connect_timeout_ms,
                disabled,
            } => {
                let mut body = serde_json::json!({
                    "name": name,
                    "listen": listen,
                    "upstream": upstream,
                    "enabled": !disabled,
                    "connect_timeout_ms": connect_timeout_ms.unwrap_or(NATIVE_DEFAULT_PROXY_TIMEOUT_MS),
                });
                if let Some(seed) = seed {
                    body["seed"] = seed.into();
                }
                if let Some(limit) = max_connections {
                    body["max_connections"] = limit.into();
                }
                request(admin, admin_token, "POST", "/v1/proxies", Some(body), json).await
            }
            ProxyCommand::Set {
                name,
                listen,
                upstream,
                max_connections,
                clear_max_connections,
                connect_timeout_ms,
                enable,
                disable,
            } => {
                if !enable
                    && !disable
                    && listen.is_none()
                    && upstream.is_none()
                    && max_connections.is_none()
                    && !clear_max_connections
                    && connect_timeout_ms.is_none()
                {
                    return Err("proxy set requires at least one field to change".into());
                }
                let mut patch = serde_json::Map::new();
                if let Some(listen) = listen {
                    patch.insert(
                        "listen".into(),
                        serde_json::to_value(listen).map_err(|e| e.to_string())?,
                    );
                }
                if let Some(upstream) = upstream {
                    patch.insert(
                        "upstream".into(),
                        serde_json::to_value(upstream).map_err(|e| e.to_string())?,
                    );
                }
                if enable {
                    patch.insert("enabled".into(), true.into());
                }
                if disable {
                    patch.insert("enabled".into(), false.into());
                }
                if let Some(limit) = max_connections {
                    patch.insert("max_connections".into(), limit.into());
                }
                if clear_max_connections {
                    patch.insert("max_connections".into(), serde_json::Value::Null);
                }
                if let Some(timeout) = connect_timeout_ms {
                    patch.insert("connect_timeout_ms".into(), timeout.into());
                }
                request(
                    admin,
                    admin_token,
                    "PATCH",
                    &format!("/v1/proxies/{name}"),
                    Some(patch.into()),
                    json,
                )
                .await
            }
            ProxyCommand::Remove { name } => {
                request(
                    admin,
                    admin_token,
                    "DELETE",
                    &format!("/v1/proxies/{name}"),
                    None,
                    json,
                )
                .await
            }
            ProxyCommand::Enable { name } => {
                request(
                    admin,
                    admin_token,
                    "PATCH",
                    &format!("/v1/proxies/{name}"),
                    Some(serde_json::json!({"enabled": true})),
                    json,
                )
                .await
            }
            ProxyCommand::Disable { name } => {
                request(
                    admin,
                    admin_token,
                    "PATCH",
                    &format!("/v1/proxies/{name}"),
                    Some(serde_json::json!({"enabled": false})),
                    json,
                )
                .await
            }
        },
        Command::Fault { command } => match command {
            FaultCommand::List { proxy } => {
                request(
                    admin,
                    admin_token,
                    "GET",
                    &format!("/v1/proxies/{proxy}/faults"),
                    None,
                    json,
                )
                .await
            }
            FaultCommand::Get { proxy, id } => {
                request(
                    admin,
                    admin_token,
                    "GET",
                    &format!("/v1/proxies/{proxy}/faults/{}", encode_path_component(&id)),
                    None,
                    json,
                )
                .await
            }
            FaultCommand::Add {
                proxy,
                id,
                direction,
                probability,
                params,
            } => {
                let kind = params.kind.clone().ok_or("fault add requires --kind")?;
                let body = serde_json::json!({
                    "direction": check_direction(&direction)?,
                    "id": id,
                    "probability": probability,
                    "kind": build_kind(&kind, &params)?,
                });
                request(
                    admin,
                    admin_token,
                    "POST",
                    &format!("/v1/proxies/{proxy}/faults"),
                    Some(body),
                    json,
                )
                .await
            }
            FaultCommand::Set {
                proxy,
                id,
                probability,
                params,
            } => {
                if probability.is_none() && params.kind.is_none() {
                    return Err("fault set requires --probability and/or --kind".into());
                }
                let mut patch = serde_json::Map::new();
                if let Some(probability) = probability {
                    patch.insert("probability".into(), probability.into());
                }
                if let Some(kind) = &params.kind {
                    patch.insert("kind".into(), build_kind(kind, &params)?);
                }
                request(
                    admin,
                    admin_token,
                    "PATCH",
                    &format!("/v1/proxies/{proxy}/faults/{}", encode_path_component(&id)),
                    Some(patch.into()),
                    json,
                )
                .await
            }
            FaultCommand::Remove { proxy, id } => {
                request(
                    admin,
                    admin_token,
                    "DELETE",
                    &format!("/v1/proxies/{proxy}/faults/{}", encode_path_component(&id)),
                    None,
                    json,
                )
                .await
            }
        },
        Command::Connection { command } => match command {
            ConnectionCommand::List => {
                request(admin, admin_token, "GET", "/v1/connections", None, json).await
            }
            ConnectionCommand::Get { id } => {
                request(
                    admin,
                    admin_token,
                    "GET",
                    &format!("/v1/connections/{id}"),
                    None,
                    json,
                )
                .await
            }
            ConnectionCommand::Kill { id } => {
                request(
                    admin,
                    admin_token,
                    "DELETE",
                    &format!("/v1/connections/{id}"),
                    None,
                    json,
                )
                .await
            }
        },
        Command::Datagram { command } => match command {
            DatagramCommand::Proxy { command } => match command {
                DatagramProxyCommand::List => request(admin, admin_token, "GET", "/v1/datagram-proxies", None, json).await,
                DatagramProxyCommand::Get { name } => request(admin, admin_token, "GET", &format!("/v1/datagram-proxies/{name}"), None, json).await,
                DatagramProxyCommand::Add { name, listen, upstream, max_associations, association_idle_timeout_ms, max_queued_datagrams, max_queued_bytes, max_datagram_size, seed } => request(admin, admin_token, "POST", "/v1/datagram-proxies", Some(serde_json::json!({"name":name,"listen":listen,"upstream":upstream,"max_associations":max_associations,"association_idle_timeout_ms":association_idle_timeout_ms,"max_queued_datagrams":max_queued_datagrams,"max_queued_bytes":max_queued_bytes,"max_datagram_size":max_datagram_size,"seed":seed})), json).await,
                DatagramProxyCommand::Set { name, listen, upstream, max_associations, association_idle_timeout_ms, enable, disable } => {
                    let mut patch = serde_json::Map::new();
                    if let Some(value) = listen { patch.insert("listen".into(), serde_json::to_value(value)?); }
                    if let Some(value) = upstream { patch.insert("upstream".into(), serde_json::to_value(value)?); }
                    if let Some(value) = max_associations { patch.insert("max_associations".into(), value.into()); }
                    if let Some(value) = association_idle_timeout_ms { patch.insert("association_idle_timeout_ms".into(), value.into()); }
                    if enable { patch.insert("enabled".into(), true.into()); }
                    if disable { patch.insert("enabled".into(), false.into()); }
                    if patch.is_empty() { return Err("datagram proxy set requires at least one field".into()); }
                    request(admin, admin_token, "PATCH", &format!("/v1/datagram-proxies/{name}"), Some(patch.into()), json).await
                }
                DatagramProxyCommand::Enable { name } => request(admin, admin_token, "PATCH", &format!("/v1/datagram-proxies/{name}"), Some(serde_json::json!({"enabled":true})), json).await,
                DatagramProxyCommand::Disable { name } => request(admin, admin_token, "PATCH", &format!("/v1/datagram-proxies/{name}"), Some(serde_json::json!({"enabled":false})), json).await,
                DatagramProxyCommand::Remove { name } => request(admin, admin_token, "DELETE", &format!("/v1/datagram-proxies/{name}"), None, json).await,
            },
            DatagramCommand::Fault { command } => match command {
                DatagramFaultCommand::List { proxy } => request(admin, admin_token, "GET", &format!("/v1/datagram-proxies/{proxy}/faults"), None, json).await,
                DatagramFaultCommand::Get { proxy, id } => request(admin, admin_token, "GET", &format!("/v1/datagram-proxies/{proxy}/faults/{}", encode_path_component(&id)), None, json).await,
                DatagramFaultCommand::Set { proxy, id, probability } => request(admin, admin_token, "PATCH", &format!("/v1/datagram-proxies/{proxy}/faults/{}", encode_path_component(&id)), Some(serde_json::json!({"probability":probability})), json).await,
                DatagramFaultCommand::Remove { proxy, id } => request(admin, admin_token, "DELETE", &format!("/v1/datagram-proxies/{proxy}/faults/{}", encode_path_component(&id)), None, json).await,
                DatagramFaultCommand::Add { proxy, id, direction, probability, kind, delay_ns, jitter_ns, additional_copies, hold_ns, bytes, bytes_per_second, burst_bytes } => {
                    check_direction(&direction)?;
                    let behavior = match kind.as_str() {
                        "delay" => serde_json::json!({"type":"delay","delay_ns":required("delay-ns",delay_ns)?,"jitter_ns":jitter_ns.unwrap_or(0)}),
                        "loss" => serde_json::json!({"type":"loss"}),
                        "duplicate" => serde_json::json!({"type":"duplicate","additional_copies":required("additional-copies",additional_copies)?}),
                        "reorder" => serde_json::json!({"type":"reorder","hold_ns":required("hold-ns",hold_ns)?}),
                        "payload-corrupt" => serde_json::json!({"type":"payload-corrupt","bytes":required("bytes",bytes)?}),
                        "bandwidth" => serde_json::json!({"type":"bandwidth","bytes_per_second":required("bytes-per-second",bytes_per_second)?,"burst_bytes":required("burst-bytes",burst_bytes)?}),
                        _ => return Err("kind must be delay, loss, duplicate, reorder, payload-corrupt, or bandwidth".into()),
                    };
                    request(admin, admin_token, "POST", &format!("/v1/datagram-proxies/{proxy}/faults"), Some(serde_json::json!({"direction":direction,"id":id,"probability":probability,"kind":behavior})), json).await
                }
            },
            DatagramCommand::Association { command } => match command {
                DatagramAssociationCommand::List => request(admin, admin_token, "GET", "/v1/datagram-associations", None, json).await,
                DatagramAssociationCommand::Get { id } => request(admin, admin_token, "GET", &format!("/v1/datagram-associations/{id}"), None, json).await,
                DatagramAssociationCommand::Kill { id } => request(admin, admin_token, "DELETE", &format!("/v1/datagram-associations/{id}"), None, json).await,
            },
        },
    }
}

/// Read a scenario/schedule file and return the semantic JSON document
/// to send to the server-side validation/compiler authority.
///
/// Files ending in `.toml` are parsed as v2 TOML schedules through the
/// shared server DTO and re-encoded as JSON; every other file is sent
/// through as JSON unchanged (v1 scenarios and v2 JSON schedules).
/// The CLI never expands phases or derives fingerprints itself.
async fn read_scenario_document(
    path: &std::path::Path,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let file = tokio::fs::File::open(path).await?;
    let mut bytes = Vec::with_capacity(1024 * 1024 + 1);
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes).await?;
    if bytes.len() > 1024 * 1024 {
        return Err("scenario document exceeds the 1 MiB request limit".into());
    }
    let is_toml = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"));
    if is_toml {
        let text = std::str::from_utf8(&bytes)?;
        let schedule = eggchaos_server::ScenarioScheduleV2Toml::from_toml_str(text)
            .map_err(|error| format!("invalid TOML schedule: {error}"))?;
        Ok(serde_json::to_value(
            eggchaos_server::ScenarioScheduleV2Dto::from(schedule),
        )?)
    } else {
        Ok(serde_json::from_slice(&bytes)?)
    }
}

fn check_direction(direction: &str) -> Result<&str, Box<dyn std::error::Error>> {
    match direction {
        "upstream" | "downstream" => Ok(direction),
        other => Err(format!("direction must be upstream or downstream, got {other:?}").into()),
    }
}

fn encode_path_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn duration_ns(millis: u64) -> Result<u64, Box<dyn std::error::Error>> {
    millis
        .checked_mul(1_000_000)
        .ok_or_else(|| "duration in milliseconds exceeds native v1 nanosecond range".into())
}

fn required<T: Copy>(name: &str, value: Option<T>) -> Result<T, Box<dyn std::error::Error>> {
    value.ok_or_else(|| format!("--kind requires --{name}").into())
}

fn build_kind(
    kind: &str,
    params: &FaultParams,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let dto = match kind {
        "latency" => FaultKindV1::Latency {
            delay_ns: duration_ns(required("delay-ms", params.delay_ms)?)?,
            jitter_ns: duration_ns(params.jitter_ms.unwrap_or(0))?,
            max_buffer_bytes: params
                .max_buffer_bytes
                .unwrap_or(NATIVE_DEFAULT_BUFFER_BYTES),
        },
        "bandwidth" | "bw" => FaultKindV1::Bandwidth {
            bytes_per_second: required("bytes-per-second", params.bytes_per_second)?,
            burst_bytes: required("burst-bytes", params.burst_bytes)?,
        },
        "blackhole" | "hole" => FaultKindV1::Blackhole {
            close_after_ns: params.close_after_ms.map(duration_ns).transpose()?,
        },
        "limit-data" | "limit" => FaultKindV1::LimitData {
            bytes: required("bytes", params.bytes)?,
        },
        "slow-close" | "slowclose" => FaultKindV1::SlowClose {
            delay_ns: duration_ns(required("delay-ms", params.delay_ms)?)?,
        },
        "slice" => FaultKindV1::Slice {
            average_size: required("average-size", params.average_size)?,
            variation: params.variation.unwrap_or(0),
            delay_ns: duration_ns(params.delay_ms.unwrap_or(0))?,
        },
        "disconnect" => FaultKindV1::Disconnect {
            after_ns: duration_ns(params.after_ms.unwrap_or(0))?,
            hard_reset: params.hard_reset,
        },
        other => {
            return Err(format!(
                "unknown --kind {other:?}: latency, bandwidth, blackhole, limit-data, slow-close, slice, disconnect"
            )
            .into());
        }
    };
    Ok(serde_json::to_value(dto)?)
}

async fn serve(path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = NativeConfig::load(path).await?;
    let proxies = config.compile_proxies()?;
    let datagram_proxies = config.compile_datagram_proxies()?;
    let runtime = config.runtime;
    let service = ServiceBuilder::new(config.seed)
        .proxy_all(proxies.clone())
        .datagram_proxy_all(datagram_proxies)
        .limits(
            runtime_admission_limits(runtime)
                .map_err(|error| format!("invalid runtime config: {error}"))?,
        )
        .relay_buffer(
            runtime
                .relay_buffer()
                .map_err(|error| format!("invalid runtime config: {error}"))?,
        )
        .termination_grace(
            runtime
                .termination_grace()
                .map_err(|error| format!("invalid runtime config: {error}"))?,
        )
        .datagram_limits(
            runtime_datagram_limits(runtime.datagram)
                .map_err(|error| format!("invalid datagram runtime config: {error}"))?,
        )
        .build()?;
    let handle = service.start().await?;
    let mut admin = NativeAdmin::start(
        AdminConfig {
            bind: config.admin.bind,
            public_admin: config.admin.public_admin,
            auth_token: config.admin.auth_token,
        },
        handle.control_state(),
    )
    .await?;
    eprintln!("eggchaos listening; admin={}", admin.local_addr());
    tokio::signal::ctrl_c().await?;
    handle.shutdown();
    admin.shutdown();
    handle.wait().await;
    admin.wait().await;
    Ok(())
}

async fn request(
    base: &str,
    admin_token: Option<&str>,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder().build();
    let url = format!("{base}{path}");
    let mut builder = match method {
        "POST" => client.post(&url)?,
        "PATCH" => client.patch(&url)?,
        "DELETE" => client.delete(&url)?,
        _ => client.get(&url)?,
    };
    if let Some(body) = body {
        builder = builder.json(&body)?;
    }
    if let Some(token) = admin_token {
        builder = builder.header("authorization", &format!("Bearer {token}"));
    }
    let mut response = builder.send().await.map_err(|_| "admin request failed")?;
    let status = response.status();
    let data = response.bytes().await?;
    if !status.is_success() {
        return Err(format!("admin returned {status}").into());
    }
    if json {
        let value: serde_json::Value = serde_json::from_slice(&data)
            .unwrap_or_else(|_| serde_json::json!({"body": String::from_utf8_lossy(&data)}));
        print_value(&value, true);
    } else {
        println!("{}", String::from_utf8_lossy(&data));
    }
    Ok(())
}

fn print_value(value: &serde_json::Value, json: bool) {
    if json {
        println!("{}", serde_json::to_string(value).unwrap());
    } else {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    }
}
