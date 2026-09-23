use super::*;
use crate::scenario::{Scenario, ScenarioAction, ScenarioEvent, ScenarioRunStatus};
use eggchaos_core::{derive_policy_seed, FaultId, Probability};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn fault(id: &str, kind: FaultKind) -> FaultSpec {
    FaultSpec {
        id: FaultId::new(id).unwrap(),
        probability: Probability::new(1.0).unwrap(),
        kind,
    }
}

async fn echo_server() -> (SocketAddr, JoinHandle<()>) {
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = origin.local_addr().unwrap();
    let task = tokio::spawn(async move {
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
    (addr, task)
}

async fn exchange(addr: SocketAddr, message: &[u8]) -> Vec<u8> {
    let mut client = TcpStream::connect(addr).await.unwrap();
    client.write_all(message).await.unwrap();
    let mut response = vec![0; message.len()];
    tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    response
}

fn test_control() -> ControlState {
    ControlState::with_params(RuntimeParams {
        seed: 7,
        term_grace: Duration::from_millis(100),
        ..RuntimeParams::default()
    })
}

#[tokio::test]
async fn ephemeral_fixed_target_proxy_relays_and_drains() {
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin_addr = origin.local_addr().unwrap();
    let origin_task = tokio::spawn(async move {
        let (mut stream, _) = origin.accept().await.unwrap();
        let mut buf = [0; 5];
        stream.read_exact(&mut buf).await.unwrap();
        stream.write_all(&buf).await.unwrap();
    });
    let service = ServiceBuilder::new(1)
        .proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .build()
        .unwrap();
    let handle = service.start().await.unwrap();
    let addr = handle.bound_addresses().await["echo"];
    let mut client = TcpStream::connect(addr).await.unwrap();
    client.write_all(b"hello").await.unwrap();
    let mut response = [0; 5];
    tokio::time::timeout(Duration::from_secs(2), client.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&response, b"hello");
    drop(client);
    origin_task.await.unwrap();
    handle.shutdown();
    handle.wait().await;
}

#[tokio::test]
async fn per_proxy_connection_limit_is_not_global() {
    let first_origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let first_address = first_origin.local_addr().unwrap();
    let first_task = tokio::spawn(async move {
        let (_stream, _) = first_origin.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let second_origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second_address = second_origin.local_addr().unwrap();
    let second_task = tokio::spawn(async move {
        let (mut stream, _) = second_origin.accept().await.unwrap();
        let mut bytes = [0; 5];
        stream.read_exact(&mut bytes).await.unwrap();
        stream.write_all(&bytes).await.unwrap();
    });
    let service = ServiceBuilder::new(1)
        .proxy(
            ProxySpec::new("first", "127.0.0.1:0".parse().unwrap(), first_address)
                .with_max_connections(1),
        )
        .proxy(
            ProxySpec::new("second", "127.0.0.1:0".parse().unwrap(), second_address)
                .with_max_connections(1),
        )
        .build()
        .unwrap();
    let handle = service.start().await.unwrap();
    let first_client = TcpStream::connect(handle.bound_addresses().await["first"])
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    let mut second_client = TcpStream::connect(handle.bound_addresses().await["second"])
        .await
        .unwrap();
    second_client.write_all(b"hello").await.unwrap();
    let mut response = [0; 5];
    tokio::time::timeout(
        Duration::from_secs(2),
        second_client.read_exact(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(&response, b"hello");
    drop(first_client);
    drop(second_client);
    handle.shutdown();
    handle.wait().await;
    first_task.abort();
    second_task.await.unwrap();
}

#[tokio::test]
async fn create_with_port_zero_reports_actual_bound_address() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let (view, generation) = control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    assert!(generation > 1);
    let bound = view.bound_addr.expect("port 0 resolves");
    assert_ne!(bound.port(), 0);
    assert!(view.running);
    assert_eq!(exchange(bound, b"hello").await, b"hello");
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn create_bind_conflict_leaves_no_ghost_proxy() {
    let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = occupied.local_addr().unwrap();
    let control = test_control();
    let before = control.generation();
    let error = control
        .create_proxy(ProxySpec::new("ghost", addr, addr))
        .await
        .unwrap_err();
    assert!(matches!(error, ControlError::BindFailed { .. }));
    assert!(control.get("ghost").await.is_none());
    assert!(control.list().await.is_empty());
    assert_eq!(control.generation(), before);
    control.shutdown_and_join().await;
}

#[tokio::test]
async fn delete_stops_listener_and_removes_definition() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let (view, _) = control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let bound = view.bound_addr.unwrap();
    assert_eq!(exchange(bound, b"hi").await, b"hi");
    control.delete_proxy("echo").await.unwrap();
    assert!(control.get("echo").await.is_none());
    assert!(TcpStream::connect(bound).await.is_err());
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn disable_enable_lifecycle_retains_definition() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let (view, _) = control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let first_bound = view.bound_addr.unwrap();
    control
        .add_fault(
            "echo",
            FaultUpsert {
                direction: Direction::Downstream,
                id: "note".into(),
                probability: 1.0,
                kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                    delay: Duration::ZERO,
                }),
            },
        )
        .await
        .unwrap();
    control.set_enabled("echo", false).await.unwrap();
    let view = control.get("echo").await.unwrap();
    assert!(!view.running);
    assert!(view.bound_addr.is_none());
    assert!(TcpStream::connect(first_bound).await.is_err());
    control.set_enabled("echo", true).await.unwrap();
    let view = control.get("echo").await.unwrap();
    assert!(view.running);
    // Fault plans survive the disable/enable round trip.
    assert!(view.downstream_faults.get("note").is_some());
    assert_eq!(exchange(view.bound_addr.unwrap(), b"yo").await, b"yo");
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn upstream_update_redirects_new_traffic() {
    let (first_addr, first_task) = echo_server().await;
    let second_origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second_addr = second_origin.local_addr().unwrap();
    let second_task = tokio::spawn(async move {
        let (mut stream, _) = second_origin.accept().await.unwrap();
        let mut buffer = [0; 6];
        stream.read_exact(&mut buffer).await.unwrap();
        stream.write_all(b"SECOND").await.unwrap();
    });
    let control = test_control();
    let (view, _) = control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            first_addr,
        ))
        .await
        .unwrap();
    let bound = view.bound_addr.unwrap();
    assert_eq!(exchange(bound, b"hello").await, b"hello");
    let (view, _) = control
        .update_proxy(
            "echo",
            ProxyPatch {
                upstream: Some(second_addr),
                ..ProxyPatch::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(view.upstream, second_addr);
    assert_eq!(
        exchange(view.bound_addr.unwrap(), b"second").await,
        b"SECOND"
    );
    second_task.await.unwrap();
    control.shutdown_and_join().await;
    first_task.abort();
}

#[tokio::test]
async fn failed_restart_keeps_old_listener_running() {
    let (origin_addr, origin_task) = echo_server().await;
    let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let conflict = occupied.local_addr().unwrap();
    let control = test_control();
    let (view, _) = control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let old_bound = view.bound_addr.unwrap();
    let error = control
        .update_proxy(
            "echo",
            ProxyPatch {
                listen: Some(conflict),
                ..ProxyPatch::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, ControlError::RestartFailed { .. }));
    // Documented consistent state: old listener still serves, spec kept.
    let view = control.get("echo").await.unwrap();
    assert!(view.running);
    assert_eq!(view.bound_addr, Some(old_bound));
    assert_eq!(exchange(old_bound, b"ok").await, b"ok");
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn concurrent_mutations_serialize_into_unique_generations() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let mut handles = Vec::new();
    for index in 0..10 {
        let control = control.clone();
        handles.push(tokio::spawn(async move {
            control
                .create_proxy(ProxySpec::new(
                    format!("proxy-{index}"),
                    "127.0.0.1:0".parse().unwrap(),
                    origin_addr,
                ))
                .await
                .map(|(_, generation)| generation)
        }));
    }
    let mut generations = Vec::new();
    for handle in handles {
        generations.push(handle.await.unwrap().unwrap());
    }
    generations.sort_unstable();
    generations.dedup();
    assert_eq!(generations.len(), 10);
    // Concurrent fault updates on one proxy also serialize uniquely.
    let mut handles = Vec::new();
    for index in 0..5 {
        let control = control.clone();
        handles.push(tokio::spawn(async move {
            control
                .add_fault(
                    "proxy-0",
                    FaultUpsert {
                        direction: Direction::Upstream,
                        id: format!("fault-{index}"),
                        probability: 1.0,
                        kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                            delay: Duration::ZERO,
                        }),
                    },
                )
                .await
                .map(|(_, _, generation)| generation)
        }));
    }
    let mut generations = Vec::new();
    for handle in handles {
        generations.push(handle.await.unwrap().unwrap());
    }
    generations.sort_unstable();
    generations.dedup();
    assert_eq!(generations.len(), 5);
    let (upstream, _) = control.list_faults("proxy-0").await.unwrap();
    assert_eq!(upstream.len(), 5);
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn fault_crud_syncs_canonical_and_live_state() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    // Add then add again: the second publish must build on the first,
    // proving GET state and live policy never roll back to stale plans.
    control
        .add_fault(
            "echo",
            FaultUpsert {
                direction: Direction::Downstream,
                id: "first".into(),
                probability: 1.0,
                kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                    delay: Duration::ZERO,
                }),
            },
        )
        .await
        .unwrap();
    control
        .add_fault(
            "echo",
            FaultUpsert {
                direction: Direction::Downstream,
                id: "second".into(),
                probability: 0.5,
                kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                    delay: Duration::from_millis(1),
                }),
            },
        )
        .await
        .unwrap();
    let view = control.get("echo").await.unwrap();
    assert!(view.downstream_faults.get("first").is_some());
    assert!(view.downstream_faults.get("second").is_some());
    // Update preserves order and identity while changing behavior.
    let (direction, updated, _) = control
        .update_fault(
            "echo",
            "second",
            FaultPatch {
                probability: Some(1.0),
                kind: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(direction, Direction::Downstream);
    assert_eq!(updated.probability.get(), 1.0);
    // Get one fault reports its direction truthfully.
    let (direction, _) = control.get_fault("echo", "second").await.unwrap();
    assert_eq!(direction, Direction::Downstream);
    // Remove narrows the plan; unknown identities 404.
    control.remove_fault("echo", "first").await.unwrap();
    let view = control.get("echo").await.unwrap();
    assert!(view.downstream_faults.get("first").is_none());
    assert!(view.downstream_faults.get("second").is_some());
    assert!(control.remove_fault("echo", "missing").await.is_err());
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn latency_fault_engages_live_traffic_without_reconnect() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let (view, _) = control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let bound = view.bound_addr.unwrap();
    let mut client = TcpStream::connect(bound).await.unwrap();
    client.write_all(b"fast").await.unwrap();
    let mut response = [0; 4];
    tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&response, b"fast");
    // Add downstream latency on the live connection: the next exchange
    // must observe it without reconnecting.
    control
        .add_fault(
            "echo",
            FaultUpsert {
                direction: Direction::Downstream,
                id: "slow".into(),
                probability: 1.0,
                kind: FaultKind::Latency(eggchaos_core::LatencyConfig {
                    delay: Duration::from_millis(150),
                    jitter: Duration::ZERO,
                    max_buffer_bytes: std::num::NonZeroU64::new(64 * 1024).unwrap(),
                }),
            },
        )
        .await
        .unwrap();
    let start = tokio::time::Instant::now();
    client.write_all(b"slow").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&response, b"slow");
    assert!(start.elapsed() >= Duration::from_millis(100));
    // Removing the fault restores fast traffic on the same connection.
    control.remove_fault("echo", "slow").await.unwrap();
    let start = tokio::time::Instant::now();
    client.write_all(b"fast").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert!(start.elapsed() < Duration::from_millis(100));
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn reset_empties_faults_and_enables_proxies() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let (view, _) = control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let bound = view.bound_addr.unwrap();
    control
        .add_fault(
            "echo",
            FaultUpsert {
                direction: Direction::Upstream,
                id: "fault".into(),
                probability: 1.0,
                kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                    delay: Duration::ZERO,
                }),
            },
        )
        .await
        .unwrap();
    // A disabled proxy with faults is re-enabled and cleared by reset.
    let (disabled, _) = control
        .create_proxy(ProxySpec::new(
            "spare",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let _ = disabled;
    control.set_enabled("spare", false).await.unwrap();
    let before = control.generation();
    let report = control.reset().await.unwrap();
    assert!(report.reset);
    assert!(report.failed_enables.is_empty());
    assert_eq!(report.generation, before + 1);
    for name in ["echo", "spare"] {
        let view = control.get(name).await.unwrap();
        assert!(view.enabled);
        assert!(view.running);
        assert!(view.upstream_faults.is_empty());
        assert!(view.downstream_faults.is_empty());
    }
    assert_eq!(exchange(bound, b"ok").await, b"ok");
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn kill_during_relay_terminates_and_cleans_up() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let (view, _) = control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let bound = view.bound_addr.unwrap();
    let mut client = TcpStream::connect(bound).await.unwrap();
    client.write_all(b"hold").await.unwrap();
    let id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(snapshot) = control.connections().await.first().cloned() {
                break snapshot.id;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(control.kill(id).await);
    // Repeated kills are idempotent and report absence.
    assert!(!control.kill(id).await);
    // The relay ends promptly after the kill.
    let mut buffer = [0; 4];
    tokio::time::timeout(Duration::from_secs(5), client.read(&mut buffer))
        .await
        .unwrap()
        .unwrap();
    // The record is removed exactly once; metrics stay consistent.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if control.connections().await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let history = control.history().await;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].outcome, ConnectionOutcome::KilledByOperator);
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn kill_during_upstream_connect_is_not_lost() {
    // Fill a listener backlog without accepting so the upstream dial
    // stays in SYN retry instead of completing or refusing.
    let stall = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let stall_addr = stall.local_addr().unwrap();
    let mut fillers = Vec::new();
    for _ in 0..256 {
        match TcpStream::connect(stall_addr).await {
            Ok(stream) => fillers.push(stream),
            Err(_) => break,
        }
    }
    let pending: Vec<_> = (0..32)
        .map(|_| {
            let addr = stall_addr;
            tokio::spawn(async move { TcpStream::connect(addr).await })
        })
        .collect();
    let control = ControlState::with_params(RuntimeParams {
        seed: 7,
        term_grace: Duration::from_millis(100),
        ..RuntimeParams::default()
    });
    // Long dial timeout so the kill wins the race deterministically.
    let mut spec = ProxySpec::new("stall", "127.0.0.1:0".parse().unwrap(), stall_addr);
    spec.connect_timeout = Duration::from_secs(30);
    let (view, _) = control.create_proxy(spec).await.unwrap();
    let bound = view.bound_addr.unwrap();
    let _client = TcpStream::connect(bound).await.unwrap();
    // Wait until the connection sits in Connecting (dial stalled).
    let id = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let connections = control.connections().await;
            if let Some(snapshot) = connections
                .iter()
                .find(|snapshot| matches!(snapshot.state, ConnectionState::Connecting))
            {
                break snapshot.id;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(control.kill(id).await);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if control.connections().await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let history = control.history().await;
    assert!(
        history
            .iter()
            .any(|record| record.outcome == ConnectionOutcome::KilledByOperator),
        "kill during connect must be recorded, got {history:?}"
    );
    control.shutdown_and_join().await;
    drop(fillers);
    for task in pending {
        task.abort();
    }
}

#[tokio::test]
async fn shutdown_with_many_connections_leaves_zero_active() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let bound = control.get("echo").await.unwrap().bound_addr.unwrap();
    let mut clients = Vec::new();
    for _ in 0..10 {
        clients.push(TcpStream::connect(bound).await.unwrap());
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if control.connections().await.len() == 10 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    control.shutdown_and_join().await;
    assert!(control.connections().await.is_empty());
    assert_eq!(control.history().await.len(), 10);
    drop(clients);
    origin_task.abort();
}

#[tokio::test]
async fn connection_finish_under_registry_contention_removes_records() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let (view, _) = control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let bound = view.bound_addr.unwrap();
    // Drive one exchange per connection so every connection reaches
    // Relaying before the concurrent close.
    let mut clients = Vec::new();
    for _ in 0..16 {
        let mut client = TcpStream::connect(bound).await.unwrap();
        client.write_all(b"ping").await.unwrap();
        let mut response = [0; 4];
        tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        clients.push(client);
    }
    // Contend the registry while every connection finishes at once:
    // readers and kills race client-side closes on the removal path.
    let probing = tokio::spawn({
        let control = control.clone();
        async move {
            for _ in 0..200 {
                let connections = control.connections().await;
                for snapshot in connections {
                    let _ = control.kill(snapshot.id).await;
                }
            }
        }
    });
    drop(clients);
    probing.await.unwrap();
    // Every finish removes its record exactly once: the active registry
    // drains and history holds one snapshot per connection.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if control.connections().await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(control.history().await.len(), 16);
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn closed_history_honors_configured_bound() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = ControlState::with_params(RuntimeParams {
        seed: 7,
        term_grace: Duration::from_millis(50),
        limits: AdmissionLimits {
            global_connections: 64,
            history: 2,
        },
        ..RuntimeParams::default()
    });
    control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let bound = control.get("echo").await.unwrap().bound_addr.unwrap();
    for _ in 0..3 {
        assert_eq!(exchange(bound, b"ping").await, b"ping");
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if control.history().await.len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    // The bound holds even after more closures arrive.
    assert_eq!(exchange(bound, b"ping").await, b"ping");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(control.history().await.len() <= 2);
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn graceful_disconnect_terminates_with_evidence() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let plan = eggchaos_core::FaultPlan::new(vec![fault(
        "bye",
        FaultKind::Disconnect(eggchaos_core::DisconnectConfig {
            after: Duration::ZERO,
            hard_reset: false,
        }),
    )])
    .unwrap();
    let mut spec = ProxySpec::new("echo", "127.0.0.1:0".parse().unwrap(), origin_addr);
    spec.downstream_faults = plan;
    let (view, _) = control.create_proxy(spec).await.unwrap();
    let bound = view.bound_addr.unwrap();
    // The exchange completes, then the relay ends on the termination.
    let mut client = TcpStream::connect(bound).await.unwrap();
    client.write_all(b"ping").await.unwrap();
    let mut response = [0; 4];
    let read = tokio::time::timeout(Duration::from_secs(5), client.read(&mut response)).await;
    assert!(read.is_ok());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !control.history().await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let history = control.history().await;
    assert_eq!(
        history[0].outcome,
        ConnectionOutcome::GracefulTermination { drained: true }
    );
    assert_eq!(
        history[0]
            .downstream_termination
            .as_ref()
            .map(|info| info.request),
        Some(TerminationRequest::Graceful)
    );
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn hard_reset_reports_truthful_platform_outcome() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let plan = eggchaos_core::FaultPlan::new(vec![fault(
        "rst",
        FaultKind::Disconnect(eggchaos_core::DisconnectConfig {
            after: Duration::ZERO,
            hard_reset: true,
        }),
    )])
    .unwrap();
    let mut spec = ProxySpec::new("echo", "127.0.0.1:0".parse().unwrap(), origin_addr);
    spec.downstream_faults = plan;
    let (view, _) = control.create_proxy(spec).await.unwrap();
    let bound = view.bound_addr.unwrap();
    let mut client = TcpStream::connect(bound).await.unwrap();
    client.write_all(b"ping").await.unwrap();
    let mut response = [0; 4];
    let _ = tokio::time::timeout(Duration::from_secs(5), client.read(&mut response)).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !control.history().await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let history = control.history().await;
    let ConnectionOutcome::HardReset { client, upstream } = &history[0].outcome else {
        panic!("expected HardReset outcome, got {:?}", history[0].outcome);
    };
    // A concrete TCP edge always attempts the reset; only the
    // already-released race reports Unsupported.
    assert!(
        !matches!(client, ResetResult::Unsupported(_)),
        "client: {client:?}"
    );
    assert!(
        !matches!(upstream, ResetResult::Unsupported(_)),
        "upstream: {upstream:?}"
    );
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn publish_get_and_stream_observe_same_generation() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let (view, _) = control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    assert_eq!(view.downstream_generation, 1);
    control
        .add_fault(
            "echo",
            FaultUpsert {
                direction: Direction::Downstream,
                id: "note".into(),
                probability: 1.0,
                kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                    delay: Duration::ZERO,
                }),
            },
        )
        .await
        .unwrap();
    // GET reports the published plan with its generation from one
    // atomic snapshot.
    let view = control.get("echo").await.unwrap();
    assert_eq!(view.downstream_generation, 2);
    assert!(view.downstream_faults.get("note").is_some());
    // A stream accepted afterwards observes the same generation and
    // fault identity. The client stays open so the relay (and its
    // record) is alive for the assertions below.
    let mut client = TcpStream::connect(view.bound_addr.unwrap()).await.unwrap();
    client.write_all(b"yo").await.unwrap();
    let mut response = [0; 2];
    tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&response, b"yo");
    let connections = control.connections().await;
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].observed_downstream_generation, 2);
    assert_eq!(connections[0].accepted_downstream_generation, 2);
    assert!(connections[0]
        .downstream_faults
        .iter()
        .any(|fault| fault.id == "note"));
    drop(client);
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn stale_base_publication_conflicts_instead_of_overwriting() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    // Read a base, then let a concurrent publication move the
    // generation before the stale base publishes.
    let (base_upstream, base_downstream) = control.snapshot_policies("echo").await.unwrap();
    control
        .add_fault(
            "echo",
            FaultUpsert {
                direction: Direction::Downstream,
                id: "fresh".into(),
                probability: 1.0,
                kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                    delay: Duration::ZERO,
                }),
            },
        )
        .await
        .unwrap();
    let stale = (*base_downstream.plan).clone();
    let error = control
        .publish_plans_expected(
            "echo",
            ExpectedPublish {
                upstream: (*base_upstream.plan).clone(),
                downstream: stale,
                upstream_seed: base_upstream.seed_namespace,
                downstream_seed: base_downstream.seed_namespace,
                expected_upstream: base_upstream.generation,
                expected_downstream: base_downstream.generation,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, ControlError::Conflict(_)));
    // The concurrent state survived: no silent rollback.
    let view = control.get("echo").await.unwrap();
    assert!(view.downstream_faults.get("fresh").is_some());
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn scenario_remove_builds_on_current_state() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    for id in ["alpha", "beta"] {
        control
            .add_fault(
                "echo",
                FaultUpsert {
                    direction: Direction::Downstream,
                    id: id.into(),
                    probability: 1.0,
                    kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                        delay: Duration::ZERO,
                    }),
                },
            )
            .await
            .unwrap();
    }
    let record = control
        .start_scenario(Scenario {
            version: 1,
            seed: 11,
            events: vec![ScenarioEvent {
                at_ms: 0,
                action: ScenarioAction::RemoveFault {
                    proxy: "echo".into(),
                    direction: Direction::Downstream,
                    id: "alpha".into(),
                },
            }],
        })
        .await
        .unwrap();
    let terminal = wait_scenario(&control, record.run_id).await;
    assert_eq!(terminal.status, ScenarioRunStatus::Completed);
    assert_eq!(terminal.applied, 1);
    // The removal derived from live B-state: beta survives, alpha is
    // gone, and the trail names the resulting generations.
    let view = control.get("echo").await.unwrap();
    assert!(view.downstream_faults.get("alpha").is_none());
    assert!(view.downstream_faults.get("beta").is_some());
    assert_eq!(terminal.trail.len(), 1);
    assert_eq!(
        terminal.trail[0].downstream_generation,
        view.downstream_generation
    );
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn concurrent_fault_publications_serialize_monotonically() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let mut handles = Vec::new();
    for index in 0..10 {
        let control = control.clone();
        handles.push(tokio::spawn(async move {
            control
                .add_fault(
                    "echo",
                    FaultUpsert {
                        direction: Direction::Downstream,
                        id: format!("fault-{index}"),
                        probability: 1.0,
                        kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                            delay: Duration::ZERO,
                        }),
                    },
                )
                .await
                .map(|(_, _, generation)| generation)
        }));
    }
    let mut generations = Vec::new();
    for handle in handles {
        generations.push(handle.await.unwrap().unwrap());
    }
    generations.sort();
    // Ten serialized publications yield ten unique monotonic global
    // generations.
    let mut deduped = generations.clone();
    deduped.dedup();
    assert_eq!(deduped.len(), 10);
    assert!(generations.windows(2).all(|pair| pair[0] < pair[1]));
    // Canonical GET and the live snapshot agree on the winner order.
    let view = control.get("echo").await.unwrap();
    assert_eq!(view.downstream_faults.faults().len(), 10);
    assert_eq!(view.downstream_generation, 11);
    let (upstream, downstream) = control.snapshot_policies("echo").await.unwrap();
    assert_eq!(downstream.generation, 11);
    assert_eq!(*downstream.plan, view.downstream_faults);
    assert_eq!(upstream.generation, 1);
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn transition_pending_while_buffer_drains() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let (view, _) = control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let bound = view.bound_addr.unwrap();
    control
        .add_fault(
            "echo",
            FaultUpsert {
                direction: Direction::Downstream,
                id: "slow".into(),
                probability: 1.0,
                kind: FaultKind::Latency(eggchaos_core::LatencyConfig {
                    delay: Duration::from_millis(500),
                    jitter: Duration::ZERO,
                    max_buffer_bytes: std::num::NonZeroU64::new(64 * 1024).unwrap(),
                }),
            },
        )
        .await
        .unwrap();
    // Hold one connection open without exchanging: its upstream leg
    // (client to target) carries no fault and completes instantly.
    let mut client = TcpStream::connect(bound).await.unwrap();
    client.write_all(b"ping").await.unwrap();
    // Wait until the echo is accepted into the downstream latency
    // queue (still far from due): publishing before the relay
    // processes the first chunk would leave nothing to drain and no
    // observable pending window.
    let id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let connections = control.connections().await;
            if let Some(snapshot) = connections.first() {
                if snapshot.downstream_bytes.accepted == 4 {
                    break snapshot.id;
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    // Publish an empty plan while the downstream echo is still held
    // in the latency queue, then force the stream to observe it with
    // another write.
    control.remove_fault("echo", "slow").await.unwrap();
    client.write_all(b"ping").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let connections = control.connections().await;
            if let Some(snapshot) = connections.first() {
                if snapshot.pending_downstream_generation.is_some() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let snapshot = control.get_connection(id).await.unwrap();
    assert_eq!(snapshot.accepted_downstream_generation, 2);
    assert_eq!(snapshot.observed_downstream_generation, 2);
    assert_eq!(snapshot.pending_downstream_generation, Some(3));
    // After the buffer drains the transition completes observably.
    let snapshot = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = control.get_connection(id).await.unwrap();
            if snapshot.pending_downstream_generation.is_none()
                && snapshot.observed_downstream_generation == 3
            {
                break snapshot;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(snapshot.downstream_transitions, 1);
    drop(client);
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn accepted_and_current_generations_update_on_traffic() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let (view, _) = control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let bound = view.bound_addr.unwrap();
    let mut client = TcpStream::connect(bound).await.unwrap();
    client.write_all(b"fast").await.unwrap();
    let mut response = [0; 4];
    tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    control
        .add_fault(
            "echo",
            FaultUpsert {
                direction: Direction::Downstream,
                id: "slow".into(),
                probability: 1.0,
                kind: FaultKind::Latency(eggchaos_core::LatencyConfig {
                    delay: Duration::from_millis(50),
                    jitter: Duration::ZERO,
                    max_buffer_bytes: std::num::NonZeroU64::new(64 * 1024).unwrap(),
                }),
            },
        )
        .await
        .unwrap();
    // Traffic after the publication transitions the live connection.
    client.write_all(b"slow").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    let snapshot = control.connections().await.pop().unwrap();
    assert_eq!(snapshot.accepted_downstream_generation, 1);
    assert_eq!(snapshot.observed_downstream_generation, 2);
    assert_eq!(snapshot.pending_downstream_generation, None);
    assert_eq!(snapshot.downstream_transitions, 1);
    assert_eq!(snapshot.accepted_upstream_generation, 1);
    assert_eq!(snapshot.observed_upstream_generation, 1);
    drop(client);
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn scenario_seed_drives_published_namespaces() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let first = control
        .start_scenario(Scenario {
            version: 1,
            seed: 21,
            events: vec![ScenarioEvent {
                at_ms: 0,
                action: ScenarioAction::SetPlan {
                    proxy: "echo".into(),
                    direction: Direction::Downstream,
                    faults: Vec::new(),
                },
            }],
        })
        .await
        .unwrap();
    let first = wait_scenario(&control, first.run_id).await;
    assert_eq!(first.status, ScenarioRunStatus::Completed);
    let view = control.get("echo").await.unwrap();
    assert_eq!(
        view.downstream_seed_namespace,
        derive_policy_seed(21, first.run_id, 0)
    );
    // A different seed publishes a different namespace for the same
    // event identity.
    let second = control
        .start_scenario(Scenario {
            version: 1,
            seed: 22,
            events: vec![ScenarioEvent {
                at_ms: 0,
                action: ScenarioAction::SetPlan {
                    proxy: "echo".into(),
                    direction: Direction::Downstream,
                    faults: Vec::new(),
                },
            }],
        })
        .await
        .unwrap();
    let second = wait_scenario(&control, second.run_id).await;
    assert_eq!(second.status, ScenarioRunStatus::Completed);
    let view = control.get("echo").await.unwrap();
    assert_eq!(
        view.downstream_seed_namespace,
        derive_policy_seed(22, second.run_id, 0)
    );
    assert_ne!(
        derive_policy_seed(21, first.run_id, 0),
        derive_policy_seed(22, second.run_id, 0)
    );
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn scenario_namespaces_replay_independent_of_scheduling() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let document = |at_ms: u64| Scenario {
        version: 1,
        seed: 33,
        events: vec![
            ScenarioEvent {
                at_ms: 0,
                action: ScenarioAction::SetPlan {
                    proxy: "echo".into(),
                    direction: Direction::Downstream,
                    faults: Vec::new(),
                },
            },
            ScenarioEvent {
                at_ms,
                action: ScenarioAction::SetPlan {
                    proxy: "echo".into(),
                    direction: Direction::Upstream,
                    faults: Vec::new(),
                },
            },
        ],
    };
    // Same seed and event identities under different timing produce
    // the same derived namespaces.
    let first = control.start_scenario(document(50)).await.unwrap();
    let first = wait_scenario(&control, first.run_id).await;
    let second = control.start_scenario(document(5)).await.unwrap();
    let second = wait_scenario(&control, second.run_id).await;
    assert_eq!(first.status, ScenarioRunStatus::Completed);
    assert_eq!(second.status, ScenarioRunStatus::Completed);
    for (record, at_ms) in [(&first, 50), (&second, 5)] {
        assert_eq!(record.trail.len(), 2);
        assert_eq!(record.trail[0].at_ms, 0);
        assert_eq!(record.trail[1].at_ms, at_ms);
    }
    let view = control.get("echo").await.unwrap();
    assert_eq!(
        view.downstream_seed_namespace,
        derive_policy_seed(33, second.run_id, 0)
    );
    assert_eq!(
        view.upstream_seed_namespace,
        derive_policy_seed(33, second.run_id, 1)
    );
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn scenario_cancel_during_sleep_returns_boundedly() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let record = control
        .start_scenario(Scenario {
            version: 1,
            seed: 44,
            events: vec![ScenarioEvent {
                at_ms: 30_000,
                action: ScenarioAction::SetPlan {
                    proxy: "echo".into(),
                    direction: Direction::Downstream,
                    faults: Vec::new(),
                },
            }],
        })
        .await
        .unwrap();
    let start = tokio::time::Instant::now();
    let cancelled = control.cancel_scenario(record.run_id).await.unwrap();
    assert!(matches!(
        cancelled.status,
        ScenarioRunStatus::Cancelling
            | ScenarioRunStatus::Cancelled
            | ScenarioRunStatus::Running
            | ScenarioRunStatus::Pending
    ));
    let terminal = wait_scenario(&control, record.run_id).await;
    assert_eq!(terminal.status, ScenarioRunStatus::Cancelled);
    assert_eq!(terminal.applied, 0);
    assert!(start.elapsed() < Duration::from_secs(10));
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn service_shutdown_cancels_active_scenarios() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let record = control
        .start_scenario(Scenario {
            version: 1,
            seed: 55,
            events: vec![ScenarioEvent {
                at_ms: 30_000,
                action: ScenarioAction::SetPlan {
                    proxy: "echo".into(),
                    direction: Direction::Downstream,
                    faults: Vec::new(),
                },
            }],
        })
        .await
        .unwrap();
    control.shutdown_and_join().await;
    let terminal = control.get_scenario(record.run_id).await.unwrap();
    assert_eq!(terminal.status, ScenarioRunStatus::Cancelled);
    origin_task.abort();
}

#[tokio::test]
async fn scenario_failure_is_observable_not_discarded() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    // The event fires after the proxy is deleted, so validation
    // passes but application fails fast with an observable record.
    let record = control
        .start_scenario(Scenario {
            version: 1,
            seed: 66,
            events: vec![ScenarioEvent {
                at_ms: 200,
                action: ScenarioAction::SetPlan {
                    proxy: "echo".into(),
                    direction: Direction::Downstream,
                    faults: Vec::new(),
                },
            }],
        })
        .await
        .unwrap();
    control.delete_proxy("echo").await.unwrap();
    let terminal = wait_scenario(&control, record.run_id).await;
    assert_eq!(terminal.status, ScenarioRunStatus::Failed);
    assert!(terminal.failure.as_deref().unwrap().contains("echo"));
    assert_eq!(terminal.applied, 0);
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn history_bound_zero_disables_retention_but_keeps_metrics() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = ControlState::with_params(RuntimeParams {
        seed: 7,
        term_grace: Duration::from_millis(100),
        limits: AdmissionLimits {
            global_connections: 64,
            history: 0,
        },
        ..RuntimeParams::default()
    });
    control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let bound = control.get("echo").await.unwrap().bound_addr.unwrap();
    assert_eq!(exchange(bound, b"ping").await, b"ping");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if control.connections().await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(control.history().await.is_empty());
    let metrics = control.metrics_text().await;
    assert!(metrics.contains("eggchaos_connections_completed_total 1\n"));
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn metrics_reconcile_with_deterministic_fixture() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let (view, _) = control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    let bound = view.bound_addr.unwrap();
    // A zero-delay graceful disconnect publishes exactly one
    // activation per connection however the bytes fragment, keeping
    // the whole fixture deterministic.
    control
        .add_fault(
            "echo",
            FaultUpsert {
                direction: Direction::Downstream,
                id: "bye".into(),
                probability: 1.0,
                kind: FaultKind::Disconnect(eggchaos_core::DisconnectConfig {
                    after: Duration::ZERO,
                    hard_reset: false,
                }),
            },
        )
        .await
        .unwrap();
    // Three graceful echoes plus one operator kill.
    for _ in 0..3 {
        assert_eq!(exchange(bound, b"hello").await, b"hello");
    }
    // Wait until all three echoes fully closed: killing while a
    // naturally-closing connection still holds its record would race
    // the token against an already-decided close.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if control.history().await.len() == 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let held = TcpStream::connect(bound).await.unwrap();
    // Identify the held connection by peer address: `connect`
    // returning does not mean the proxy registered it yet, so a bare
    // length wait could grab a lingering echo record instead.
    let held_peer = held.local_addr().unwrap();
    let id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let connections = control.connections().await;
            if let Some(snapshot) = connections
                .iter()
                .find(|snapshot| snapshot.peer == held_peer)
            {
                break snapshot.id;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(control.kill(id).await);
    drop(held);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if control.connections().await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let metrics = control.metrics_text().await;
    // Exact global reconciliation: 4 accepted, 4 completed, 3
    // graceful drains, 1 kill, 30 bytes each way per echo leg.
    assert!(metrics.contains("eggchaos_connections_accepted_total 4\n"));
    assert!(metrics.contains("eggchaos_connections_completed_total 4\n"));
    assert!(metrics
        .contains("eggchaos_connection_outcomes_total{outcome=\"graceful_termination\"} 3\n"));
    assert!(
        metrics.contains("eggchaos_connection_outcomes_total{outcome=\"killed_by_operator\"} 1\n")
    );
    assert!(metrics.contains("eggchaos_termination_requests_total{request=\"graceful\"} 3\n"));
    assert!(metrics.contains("eggchaos_bytes_total{flow=\"accepted\"} 30\n"));
    assert!(metrics.contains("eggchaos_bytes_total{flow=\"forwarded\"} 30\n"));
    assert!(metrics.contains("eggchaos_bytes_total{flow=\"discarded\"} 0\n"));
    assert!(metrics.contains("eggchaos_proxy_connections_accepted_total{proxy=\"echo\"} 4\n"));
    assert!(metrics.contains("eggchaos_proxy_connections_completed_total{proxy=\"echo\"} 4\n"));
    // The disconnect fault activated exactly once per echo connection.
    assert!(metrics.contains(
            "eggchaos_fault_activations_total{proxy=\"echo\",direction=\"downstream\",fault_type=\"disconnect\"} 3\n"
        ));
    // History agrees with the completed counter.
    assert_eq!(control.history().await.len(), 4);
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn metrics_use_only_bounded_label_keys() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let (view, _) = control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    assert_eq!(exchange(view.bound_addr.unwrap(), b"ping").await, b"ping");
    let metrics = control.metrics_text().await;
    for line in metrics.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some(labels) = line
            .split('{')
            .nth(1)
            .and_then(|rest| rest.split('}').next())
        {
            for pair in labels.split(',') {
                let key = pair.split('=').next().unwrap_or_default();
                assert!(
                    [
                        "proxy",
                        "direction",
                        "flow",
                        "outcome",
                        "request",
                        "result",
                        "fault_type"
                    ]
                    .contains(&key),
                    "unexpected metric label {key:?} in {line:?}"
                );
            }
        }
    }
    control.shutdown_and_join().await;
    origin_task.abort();
}

#[tokio::test]
async fn connection_evidence_contains_no_payload_bytes() {
    let (origin_addr, origin_task) = echo_server().await;
    let control = test_control();
    let (view, _) = control
        .create_proxy(ProxySpec::new(
            "echo",
            "127.0.0.1:0".parse().unwrap(),
            origin_addr,
        ))
        .await
        .unwrap();
    assert_eq!(exchange(view.bound_addr.unwrap(), b"PxQ9z").await, b"PxQ9z");
    let live = serde_json::to_string(&control.connections().await).unwrap();
    assert!(!live.contains("PxQ9z"));
    assert!(!live.contains("payload"));
    let closed = serde_json::to_string(&control.history().await).unwrap();
    assert!(!closed.contains("PxQ9z"));
    assert!(!closed.contains("payload"));
    control.shutdown_and_join().await;
    origin_task.abort();
}

async fn wait_scenario(control: &ControlState, run_id: u64) -> crate::scenario::ScenarioRunRecord {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let record = control.get_scenario(run_id).await.unwrap();
            if matches!(
                record.status,
                ScenarioRunStatus::Completed
                    | ScenarioRunStatus::Cancelled
                    | ScenarioRunStatus::Failed
            ) {
                break record;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}
