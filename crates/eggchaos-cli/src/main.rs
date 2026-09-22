use std::path::PathBuf;

use clap::{Parser, Subcommand};
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
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Serve { config } => serve(config).await,
        Command::Version => {
            print_value(
                &serde_json::json!({"version": env!("CARGO_PKG_VERSION"), "api": "v1"}),
                cli.json,
            );
            Ok(())
        }
        Command::Reset => request(&cli.admin, "POST", "/v1/reset", None, cli.json).await,
        Command::Proxy { command } => match command {
            ProxyCommand::List => request(&cli.admin, "GET", "/v1/proxies", None, cli.json).await,
            ProxyCommand::Get { name } => {
                request(
                    &cli.admin,
                    "GET",
                    &format!("/v1/proxies/{name}"),
                    None,
                    cli.json,
                )
                .await
            }
        },
    };
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
    handle.wait().await?;
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
