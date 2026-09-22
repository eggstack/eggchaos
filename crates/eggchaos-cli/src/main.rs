#![forbid(unsafe_code)]
use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use eggchaos_server::{AdminConfig, NativeAdmin, NativeConfig, ServiceBuilder};
use eggfetch_core::Client;

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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let result = dispatch(cli.command, &cli.admin, cli.json).await;
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
        Command::Reset => request(admin, "POST", "/v1/reset", None, json).await,
        Command::Proxy { command } => match command {
            ProxyCommand::List => request(admin, "GET", "/v1/proxies", None, json).await,
            ProxyCommand::Get { name } => {
                request(admin, "GET", &format!("/v1/proxies/{name}"), None, json).await
            }
            ProxyCommand::Add {
                name,
                listen,
                upstream,
                max_connections,
                connect_timeout_ms,
                disabled,
            } => {
                let mut body = serde_json::json!({
                    "name": name,
                    "listen": listen,
                    "upstream": upstream,
                    "enabled": !disabled,
                    "connect_timeout": connect_timeout_ms.unwrap_or(5000),
                });
                if let Some(limit) = max_connections {
                    body["max_connections"] = limit.into();
                }
                request(admin, "POST", "/v1/proxies", Some(body), json).await
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
                    "PATCH",
                    &format!("/v1/proxies/{name}"),
                    Some(patch.into()),
                    json,
                )
                .await
            }
            ProxyCommand::Remove { name } => {
                request(admin, "DELETE", &format!("/v1/proxies/{name}"), None, json).await
            }
            ProxyCommand::Enable { name } => {
                request(
                    admin,
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
                    "GET",
                    &format!("/v1/proxies/{proxy}/faults/{id}"),
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
                    "PATCH",
                    &format!("/v1/proxies/{proxy}/faults/{id}"),
                    Some(patch.into()),
                    json,
                )
                .await
            }
            FaultCommand::Remove { proxy, id } => {
                request(
                    admin,
                    "DELETE",
                    &format!("/v1/proxies/{proxy}/faults/{id}"),
                    None,
                    json,
                )
                .await
            }
        },
        Command::Connection { command } => match command {
            ConnectionCommand::List => request(admin, "GET", "/v1/connections", None, json).await,
            ConnectionCommand::Get { id } => {
                request(admin, "GET", &format!("/v1/connections/{id}"), None, json).await
            }
            ConnectionCommand::Kill { id } => {
                request(
                    admin,
                    "DELETE",
                    &format!("/v1/connections/{id}"),
                    None,
                    json,
                )
                .await
            }
        },
    }
}

fn check_direction(direction: &str) -> Result<&str, Box<dyn std::error::Error>> {
    match direction {
        "upstream" | "downstream" => Ok(direction),
        other => Err(format!("direction must be upstream or downstream, got {other:?}").into()),
    }
}

fn duration_ms_json(millis: u64) -> serde_json::Value {
    serde_json::json!({"secs": millis / 1000, "nanos": (millis % 1000) * 1_000_000})
}

fn required<T: Copy>(name: &str, value: Option<T>) -> Result<T, Box<dyn std::error::Error>> {
    value.ok_or_else(|| format!("--kind requires --{name}").into())
}

fn build_kind(
    kind: &str,
    params: &FaultParams,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    match kind {
        "latency" => Ok(serde_json::json!({"Latency": {
            "delay": duration_ms_json(required("delay-ms", params.delay_ms)?),
            "jitter": duration_ms_json(params.jitter_ms.unwrap_or(0)),
            "max_buffer_bytes": params.max_buffer_bytes.unwrap_or(64 * 1024),
        }})),
        "bandwidth" | "bw" => Ok(serde_json::json!({"Bandwidth": {
            "bytes_per_second": required("bytes-per-second", params.bytes_per_second)?,
            "burst_bytes": required("burst-bytes", params.burst_bytes)?,
        }})),
        "blackhole" | "hole" => Ok(serde_json::json!({"Blackhole": {
            "close_after": params.close_after_ms.map(duration_ms_json),
        }})),
        "limit-data" | "limit" => Ok(serde_json::json!({"LimitData": {
            "bytes": required("bytes", params.bytes)?,
        }})),
        "slow-close" | "slowclose" => Ok(serde_json::json!({"SlowClose": {
            "delay": duration_ms_json(required("delay-ms", params.delay_ms)?),
        }})),
        "slice" => Ok(serde_json::json!({"Slice": {
            "average_size": required("average-size", params.average_size)?,
            "variation": params.variation.unwrap_or(0),
            "delay": duration_ms_json(params.delay_ms.unwrap_or(0)),
        }})),
        "disconnect" => Ok(serde_json::json!({"Disconnect": {
            "after": duration_ms_json(params.after_ms.unwrap_or(0)),
            "hard_reset": params.hard_reset,
        }})),
        other => Err(format!(
            "unknown --kind {other:?}: latency, bandwidth, blackhole, limit-data, slow-close, slice, disconnect"
        )
        .into()),
    }
}

async fn serve(path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = NativeConfig::load(path).await?;
    let proxies = config.compile_proxies()?;
    let service = ServiceBuilder::new(config.seed)
        .proxy_all(proxies.clone())
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
    let mut response = builder.send().await?;
    let status = response.status();
    let data = response.bytes().await?;
    if json {
        let value: serde_json::Value = serde_json::from_slice(&data)
            .unwrap_or_else(|_| serde_json::json!({"body": String::from_utf8_lossy(&data)}));
        print_value(&value, true);
    } else {
        println!("{}", String::from_utf8_lossy(&data));
    }
    if !status.is_success() {
        return Err(format!("admin returned {status}").into());
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
