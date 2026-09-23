use std::net::SocketAddr;
use std::process::Command;
use std::time::Duration;

use eggchaos_server::{AdminConfig, ControlState, NativeAdmin, RuntimeParams};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn cli(admin: SocketAddr, args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_eggchaos"))
        .env_remove("EGGCHAOS_ADMIN_TOKEN")
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

fn cli_token(admin: SocketAddr, token: &str, args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_eggchaos"))
        .env("EGGCHAOS_ADMIN_TOKEN", "wrong-secret")
        .arg("--admin")
        .arg(format!("http://{admin}"))
        .arg("--admin-token")
        .arg(token)
        .arg("--json")
        .args(args)
        .output()
        .expect("cli binary runs");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
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

    let history_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let (ok, out) = cli(admin_addr, &["history"]);
        assert!(ok, "{out}");
        if !serde_json::from_str::<serde_json::Value>(&out)
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
        {
            break;
        }
        assert!(
            tokio::time::Instant::now() < history_deadline,
            "history record appears"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let (ok, out) = cli(admin_addr, &["metrics"]);
    assert!(ok, "{out}");
    let metrics: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(metrics["body"].as_str().unwrap().contains("# HELP"));

    let scenario_path =
        std::env::temp_dir().join(format!("eggchaos-scenario-{}.json", std::process::id()));
    std::fs::write(&scenario_path, r#"{"version":1,"seed":7,"events":[]}"#).unwrap();
    let scenario_path_str = scenario_path.to_str().unwrap();
    let (ok, out) = cli(admin_addr, &["scenario", "apply", scenario_path_str]);
    let _ = std::fs::remove_file(&scenario_path);
    assert!(ok, "{out}");
    let applied: serde_json::Value = serde_json::from_str(&out).unwrap();
    let run_id = applied["run_id"].as_u64().unwrap().to_string();
    for args in [
        vec!["scenario", "get", &run_id],
        vec!["scenario", "cancel", &run_id],
    ] {
        let (ok, out) = cli(admin_addr, &args);
        assert!(ok, "{out}");
        let record: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(record["run_id"], run_id.parse::<u64>().unwrap());
    }

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_authenticates_and_round_trips_opaque_fault_ids() {
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let admin = NativeAdmin::start(
        AdminConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            auth_token: Some("right-secret".into()),
            ..AdminConfig::default()
        },
        ControlState::default(),
    )
    .await
    .unwrap();
    let address = admin.local_addr();
    let (no_token, no_token_out) = cli(address, &["proxy", "list"]);
    assert!(!no_token);
    let _: serde_json::Value =
        serde_json::from_str(&no_token_out).expect("one JSON error document");
    let (ok, out, err) = cli_token(address, "wrong-secret", &["proxy", "list"]);
    assert!(!ok);
    let _: serde_json::Value = serde_json::from_str(&out).expect("one JSON error document");
    assert!(!out.contains("wrong-secret") && !err.contains("wrong-secret"));
    assert!(!out.contains("right-secret") && !err.contains("right-secret"));
    let (ok, out, _) = cli_token(
        address,
        "right-secret",
        &[
            "proxy",
            "add",
            "p1",
            "--listen",
            "127.0.0.1:0",
            "--upstream",
            &origin.local_addr().unwrap().to_string(),
        ],
    );
    assert!(ok, "{out}");
    let id = "part/percent%? ü";
    let (ok, out, _) = cli_token(
        address,
        "right-secret",
        &[
            "fault",
            "add",
            "p1",
            id,
            "--kind",
            "latency",
            "--delay-ms",
            "0",
        ],
    );
    assert!(ok, "{out}");
    for args in [
        vec!["fault", "get", "p1", id],
        vec!["fault", "set", "p1", id, "--probability", "0.5"],
        vec!["fault", "remove", "p1", id],
    ] {
        let (ok, out, _) = cli_token(address, "right-secret", &args);
        assert!(ok, "{out}");
    }
    admin.shutdown();
}
