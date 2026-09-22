use std::net::SocketAddr;
use std::process::Command;
use std::time::Duration;

use eggchaos_server::{AdminConfig, ControlState, NativeAdmin, RuntimeParams};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn cli(admin: SocketAddr, args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_eggchaos"))
        .arg("--admin")
        .arg(format!("http://{admin}"))
        .arg("--json")
        .args(args)
        .output()
        .expect("cli binary runs");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

async fn find_connection(admin: SocketAddr, proxy: &str) -> u64 {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let (ok, out) = cli(admin, &["connection", "list"]);
        assert!(ok, "{out}");
        let connections: serde_json::Value = serde_json::from_str(&out).unwrap();
        if let Some(entry) = connections
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["proxy"] == proxy)
        {
            return entry["id"].as_u64().unwrap();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "connection registered"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cli_json_create_fault_kill_reset_end_to_end() {
    // Multi-thread: the synchronous child-process waits must not freeze
    // the runtime hosting the admin under test.
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin_addr = origin.local_addr().unwrap();
    let origin_task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = origin.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buffer = [0; 4096];
                loop {
                    match stream.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(n) => {
                            if stream.write_all(&buffer[..n]).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
    });
    let control = ControlState::with_params(RuntimeParams {
        seed: 7,
        ..RuntimeParams::default()
    });
    let admin = NativeAdmin::start(
        AdminConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            ..AdminConfig::default()
        },
        control,
    )
    .await
    .unwrap();
    let admin_addr = admin.local_addr();

    // Create a proxy through the CLI: one JSON document, then relay works.
    let (ok, out) = cli(
        admin_addr,
        &[
            "proxy",
            "add",
            "p1",
            "--listen",
            "127.0.0.1:0",
            "--upstream",
            &origin_addr.to_string(),
        ],
    );
    assert!(ok, "{out}");
    let created: serde_json::Value = serde_json::from_str(&out).unwrap();
    let bound: SocketAddr = created["proxy"]["bound_addr"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    // Add a fault through the CLI.
    let (ok, out) = cli(
        admin_addr,
        &[
            "fault",
            "add",
            "p1",
            "slow",
            "--direction",
            "downstream",
            "--kind",
            "latency",
            "--delay-ms",
            "50",
        ],
    );
    assert!(ok, "{out}");
    let added: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(added["direction"], "downstream");

    // Hold a live connection, then kill it through the CLI.
    let held = TcpStream::connect(bound).await.unwrap();
    let id = find_connection(admin_addr, "p1").await;
    let (ok, out) = cli(admin_addr, &["connection", "kill", &id.to_string()]);
    assert!(ok, "{out}");
    let killed: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(killed["terminated"], true);
    drop(held);

    // Reset through the CLI empties fault plans.
    let (ok, _) = cli(admin_addr, &["reset"]);
    assert!(ok);
    let (ok, out) = cli(admin_addr, &["fault", "list", "p1"]);
    assert!(ok, "{out}");
    let faults: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(faults["downstream"].as_array().unwrap().len(), 0);

    // Unknown names fail with a nonzero exit.
    let (ok, _) = cli(admin_addr, &["proxy", "get", "missing"]);
    assert!(!ok);

    admin.shutdown();
    origin_task.abort();
}
