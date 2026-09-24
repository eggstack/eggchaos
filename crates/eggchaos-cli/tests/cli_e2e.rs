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
        // V1 wire shape is unchanged by the v2 tranche: no schedule
        // identity fields leak into v1 run records.
        assert!(record.get("schedule_fingerprint").is_none());
        assert!(record.get("execution_key").is_none());
        assert!(record.get("cleanup").is_none());
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn datagram_cli_is_json_first_and_uses_native_routes() {
    let target = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let target_addr = target.local_addr().unwrap();
    let control = ControlState::default();
    let mut admin = NativeAdmin::start(
        AdminConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            ..AdminConfig::default()
        },
        control.clone(),
    )
    .await
    .unwrap();
    let (ok, output) = cli(
        admin.local_addr(),
        &[
            "datagram",
            "proxy",
            "add",
            "dns",
            "--listen",
            "127.0.0.1:0",
            "--upstream",
            &target_addr.to_string(),
        ],
    );
    assert!(ok, "{output}");
    let created: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert!(created["proxy"]["running"].as_bool().unwrap());
    let (ok, output) = cli(
        admin.local_addr(),
        &[
            "datagram",
            "fault",
            "add",
            "dns",
            "loss",
            "--kind",
            "loss",
            "--direction",
            "upstream",
        ],
    );
    assert!(ok, "{output}");
    let fault: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(fault["fault"]["kind"]["type"], "loss");
    let (ok, output) = cli(admin.local_addr(), &["datagram", "fault", "list", "dns"]);
    assert!(ok, "{output}");
    let listed: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(listed["upstream"]["faults"].as_array().unwrap().len(), 1);
    let (ok, output) = cli(admin.local_addr(), &["datagram", "proxy", "disable", "dns"]);
    assert!(ok, "{output}");
    assert!(
        !serde_json::from_str::<serde_json::Value>(&output).unwrap()["proxy"]["running"]
            .as_bool()
            .unwrap()
    );
    let (ok, output) = cli(admin.local_addr(), &["datagram", "proxy", "remove", "dns"]);
    assert!(ok, "{output}");
    admin.shutdown();
    admin.wait().await;
    control.shutdown_and_join().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cli_scenario_v2_validate_compile_apply_json_and_toml() {
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
    let control = ControlState::default();
    let mut admin = NativeAdmin::start(
        AdminConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            ..AdminConfig::default()
        },
        control.clone(),
    )
    .await
    .unwrap();
    let admin_addr = admin.local_addr();
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

    let pid = std::process::id();
    let json_path = std::env::temp_dir().join(format!("eggchaos-schedule-v2-{pid}.json"));
    let toml_path = std::env::temp_dir().join(format!("eggchaos-schedule-v2-{pid}.toml"));
    std::fs::write(
        &json_path,
        r#"{"version":2,"seed":30,"execution_key":31,"isolation":"strict","cleanup":"restore-initial","phases":[{"name":"warmup","duration_ns":10000000,"actions":[{"type":"set-plan","proxy":"p1","direction":"downstream","faults":[{"id":"a","probability":1.0,"kind":{"type":"latency","delay_ns":5000000,"jitter_ns":0,"max_buffer_bytes":1024}}]}]}]}"#,
    )
    .unwrap();
    std::fs::write(
        &toml_path,
        "version = 2\nseed = 30\nexecution_key = 31\nisolation = \"strict\"\ncleanup = \"restore-initial\"\n\n[[phases]]\nname = \"warmup\"\nduration_ns = 10000000\n\n[[phases.actions]]\ntype = \"set-plan\"\nproxy = \"p1\"\ndirection = \"downstream\"\n\n[[phases.actions.faults]]\nid = \"a\"\nprobability = 1.0\n\n[phases.actions.faults.kind]\ntype = \"latency\"\ndelay_ns = 5000000\njitter_ns = 0\nmax_buffer_bytes = 1024\n",
    )
    .unwrap();
    let json_str = json_path.to_str().unwrap();
    let toml_str = toml_path.to_str().unwrap();

    // Validate creates no run and reports the fingerprint.
    let (ok, out) = cli(admin_addr, &["scenario", "validate", json_str]);
    assert!(ok, "{out}");
    let validated: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(validated["compiler_semantics_version"], 1);
    assert_eq!(validated["event_count"], 1);
    let fingerprint = validated["schedule_fingerprint"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(fingerprint.len(), 64);

    // TOML validates to the same fingerprint.
    let (ok, out) = cli(admin_addr, &["scenario", "validate", toml_str]);
    assert!(ok, "{out}");
    let validated_toml: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(validated_toml["schedule_fingerprint"], fingerprint);

    // Compile returns the normalized tape without creating a run.
    let (ok, out) = cli(admin_addr, &["scenario", "compile", json_str]);
    assert!(ok, "{out}");
    let compiled: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(compiled["schedule_fingerprint"], fingerprint);
    assert_eq!(compiled["events"].as_array().unwrap().len(), 1);
    assert_eq!(compiled["events"][0]["offset_ns"], 0);
    assert_eq!(compiled["events"][0]["phase"], "top/0");

    // Apply runs the schedule; the run carries stable identity plus
    // scheduled/applied timing and cleanup evidence.
    let (ok, out) = cli(admin_addr, &["scenario", "apply", json_str]);
    assert!(ok, "{out}");
    let applied: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(applied["schedule_fingerprint"], fingerprint);
    let run_id = applied["run_id"].as_u64().unwrap().to_string();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let record = loop {
        let (ok, out) = cli(admin_addr, &["scenario", "get", &run_id]);
        assert!(ok, "{out}");
        let record: serde_json::Value = serde_json::from_str(&out).unwrap();
        if record["status"] == "completed" {
            break record;
        }
        assert!(tokio::time::Instant::now() < deadline, "v2 run completes");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert_eq!(record["applied"], 1);
    assert_eq!(record["events"][0]["scheduled_offset_ns"], 0);
    assert!(record["events"][0]["applied_elapsed_ns"].as_u64().is_some());
    assert_eq!(record["cleanup"]["policy"], "restore-initial");
    assert_eq!(record["cleanup"]["resources"][0]["outcome"], "restored");

    // TOML applies to the same fingerprint.
    let (ok, out) = cli(admin_addr, &["scenario", "apply", toml_str]);
    assert!(ok, "{out}");
    let applied_toml: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(applied_toml["schedule_fingerprint"], fingerprint);

    // A v2 cancel of a finished run returns the final record unchanged.
    let (ok, out) = cli(admin_addr, &["scenario", "cancel", &run_id]);
    assert!(ok, "{out}");
    let cancelled: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(cancelled["status"], "completed");

    let _ = std::fs::remove_file(&json_path);
    let _ = std::fs::remove_file(&toml_path);
    admin.shutdown();
    admin.wait().await;
    control.shutdown_and_join().await;
    origin_task.abort();
}
