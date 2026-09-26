//! Datagram runtime regression suite: isolation, lifecycle, capacity,
//! multi-response routing, idle expiry, duplication, rollback, IPv6, and the
//! M024 setup-race guarantees.
use std::{
    io,
    net::SocketAddr,
    num::NonZeroU64,
    sync::{atomic::Ordering, Arc, Mutex},
    time::Duration,
};

use eggchaos_core::{DatagramPlan, DatagramQueueLimits};
use tokio::{net::UdpSocket, time::timeout};
use tokio_util::sync::CancellationToken;

use super::{
    association::{
        resolve_association, Association, AssociationSlot, SetupHook, StartingReservation,
    },
    registry::ProxyState,
    DatagramProxySpec, DatagramRuntime, DatagramRuntimeError, DatagramRuntimeLimits,
};

fn limits() -> DatagramQueueLimits {
    DatagramQueueLimits {
        max_queued_datagrams: NonZeroU64::new(16).unwrap(),
        max_queued_bytes: NonZeroU64::new(65_536).unwrap(),
        max_datagram_bytes: NonZeroU64::new(65_535).unwrap(),
    }
}

async fn echo_target() -> (SocketAddr, CancellationToken) {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    let cancel = CancellationToken::new();
    let done = cancel.clone();
    tokio::spawn(async move {
        let mut buf = [0u8; 65_536];
        loop {
            tokio::select! { _ = done.cancelled() => break, result = socket.recv_from(&mut buf) => if let Ok((len, peer)) = result { let _ = socket.send_to(&buf[..len], peer).await; } }
        }
    });
    (addr, cancel)
}

async fn direct_setup(
    runtime_limits: DatagramRuntimeLimits,
    proxy_limit: usize,
) -> (
    DatagramRuntime,
    Arc<ProxyState>,
    Arc<UdpSocket>,
    UdpSocket,
    SocketAddr,
    SetupHook,
    CancellationToken,
) {
    let (target, stop_target) = echo_target().await;
    let runtime = DatagramRuntime::new(runtime_limits).unwrap();
    let mut spec =
        DatagramProxySpec::new("direct", "127.0.0.1:0".parse().unwrap(), target, limits()).unwrap();
    spec.max_associations = proxy_limit;
    runtime.create_proxy(spec).await.unwrap();
    let state = runtime.state_for_test("direct").await.unwrap();
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = client.local_addr().unwrap();
    let hook = SetupHook::new();
    runtime.install_setup_hook("direct", hook.clone()).await;
    (
        runtime,
        state,
        socket,
        client,
        client_addr,
        hook,
        stop_target,
    )
}

async fn starting_reservation(state: &ProxyState, client: SocketAddr) -> Arc<StartingReservation> {
    state
        .associations
        .read()
        .expect("datagram association registry")
        .get(&client)
        .and_then(|slot| match slot {
            AssociationSlot::Starting { reservation } => Some(reservation.clone()),
            AssociationSlot::Active(_) => None,
        })
        .expect("starting association reservation")
}

async fn join_associations(
    tasks: &mut tokio::task::JoinSet<Result<Arc<Association>, DatagramRuntimeError>>,
) -> Vec<Arc<Association>> {
    let mut associations = Vec::new();
    while let Some(result) = tasks.join_next().await {
        associations.push(result.unwrap().unwrap());
    }
    associations
}

fn spawn_resolve(
    socket: Arc<UdpSocket>,
    state: Arc<ProxyState>,
    client: SocketAddr,
) -> tokio::task::JoinHandle<Result<Arc<Association>, DatagramRuntimeError>> {
    tokio::spawn(async move { resolve_association(socket, &state, client).await })
}

async fn multi_response_target() -> (SocketAddr, CancellationToken, Arc<Mutex<Vec<SocketAddr>>>) {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    let cancel = CancellationToken::new();
    let done = cancel.clone();
    let peers = Arc::new(Mutex::new(Vec::new()));
    let known_peers = peers.clone();
    tokio::spawn(async move {
        let mut buf = [0u8; 65_536];
        loop {
            tokio::select! {
                _ = done.cancelled() => break,
                result = socket.recv_from(&mut buf) => if let Ok((len, peer)) = result {
                    if &buf[..len] == b"push" {
                        let recipients = known_peers.lock().expect("peer list").clone();
                        for recipient in recipients { let _ = socket.send_to(b"unsolicited", recipient).await; }
                    } else {
                        known_peers.lock().expect("peer list").push(peer);
                        let first = [&buf[..len], b":one"].concat();
                        let second = [&buf[..len], b":two"].concat();
                        let _ = socket.send_to(&first, peer).await;
                        let _ = socket.send_to(&second, peer).await;
                    }
                }
            }
        }
    });
    (addr, cancel, peers)
}

#[tokio::test]
async fn fixed_target_isolation_and_multiple_clients() {
    let (target, stop_target) = echo_target().await;
    let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default()).unwrap();
    let spec =
        DatagramProxySpec::new("echo", "127.0.0.1:0".parse().unwrap(), target, limits()).unwrap();
    let view = runtime.create_proxy(spec).await.unwrap();
    let listen = view.bound_addr.unwrap();
    let c1 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let c2 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    c1.send_to(b"client-one", listen).await.unwrap();
    c2.send_to(b"client-two", listen).await.unwrap();
    let mut b1 = [0u8; 64];
    let mut b2 = [0u8; 64];
    let (n1, _) = timeout(Duration::from_secs(2), c1.recv_from(&mut b1))
        .await
        .unwrap()
        .unwrap();
    let (n2, _) = timeout(Duration::from_secs(2), c2.recv_from(&mut b2))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&b1[..n1], b"client-one");
    assert_eq!(&b2[..n2], b"client-two");
    let associations = runtime.associations().await;
    assert_eq!(associations.len(), 2);
    assert_ne!(associations[0].id, associations[1].id);
    assert!(associations.iter().all(|a| a.egress_datagrams == 2));
    runtime.delete_proxy("echo").await.unwrap();
    stop_target.cancel();
    assert!(runtime.associations().await.is_empty());
}

#[tokio::test]
async fn explicit_kill_records_admin_discard_and_disable_reenable_is_clean() {
    let (target, stop_target) = echo_target().await;
    let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default()).unwrap();
    let spec =
        DatagramProxySpec::new("echo", "127.0.0.1:0".parse().unwrap(), target, limits()).unwrap();
    let delay = eggchaos_core::DatagramFaultSpec {
        id: eggchaos_core::FaultId::new("delay").unwrap(),
        probability: eggchaos_core::Probability::new(1.0).unwrap(),
        kind: eggchaos_core::DatagramFaultKind::Delay {
            delay: Duration::from_secs(30),
            jitter: Duration::ZERO,
        },
    };
    spec.upstream_policy
        .publish(DatagramPlan::new(vec![delay]).unwrap(), 0)
        .unwrap();
    let view = runtime.create_proxy(spec).await.unwrap();
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client
        .send_to(b"ping", view.bound_addr.unwrap())
        .await
        .unwrap();
    let pending = timeout(Duration::from_secs(2), async {
        loop {
            if let Some(association) = runtime.associations().await.first() {
                if association.upstream_evidence.queued_datagrams == 1 {
                    break association.id;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let id = pending;
    assert!(runtime.kill_association(id).await);
    let final_view = runtime.association(id).await.unwrap();
    assert_eq!(final_view.administrative_discards, 1);
    assert_eq!(final_view.upstream_evidence.configured_loss, 0);
    runtime.disable_proxy("echo").await.unwrap();
    assert!(!runtime.proxies().await[0].running);
    let restarted = runtime.enable_proxy("echo").await.unwrap();
    assert!(restarted.running);
    assert_ne!(restarted.bound_addr.unwrap().port(), 0);
    runtime.delete_proxy("echo").await.unwrap();
    stop_target.cancel();
}

#[tokio::test]
async fn capacity_rejects_clients_and_oversize_is_classified_after_receive() {
    let (target, stop_target) = echo_target().await;
    let runtime = DatagramRuntime::new(DatagramRuntimeLimits {
        max_associations: 1,
        ..DatagramRuntimeLimits::default()
    })
    .unwrap();
    let queue_limits = DatagramQueueLimits {
        max_datagram_bytes: NonZeroU64::new(4).unwrap(),
        ..limits()
    };
    let mut spec =
        DatagramProxySpec::new("echo", "127.0.0.1:0".parse().unwrap(), target, queue_limits)
            .unwrap();
    spec.max_associations = 1;
    let view = runtime.create_proxy(spec).await.unwrap();
    let c1 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let c2 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    c1.send_to(b"ok", view.bound_addr.unwrap()).await.unwrap();
    let mut buf = [0u8; 64];
    let _ = timeout(Duration::from_secs(2), c1.recv_from(&mut buf))
        .await
        .unwrap()
        .unwrap();
    c2.send_to(b"12345678", view.bound_addr.unwrap())
        .await
        .unwrap();
    c2.send_to(b"z", view.bound_addr.unwrap()).await.unwrap();
    assert_eq!(runtime.associations().await.len(), 1);
    let view = timeout(Duration::from_secs(2), async {
        loop {
            let view = runtime.proxies().await.remove(0);
            if view.oversize_datagrams == 1 && view.association_capacity_rejections == 1 {
                break view;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(view.oversize_datagrams, 1);
    assert_eq!(view.association_capacity_rejections, 1);
    runtime.delete_proxy("echo").await.unwrap();
    stop_target.cancel();
}

#[tokio::test]
async fn association_routes_multiple_and_unsolicited_replies_and_keeps_source_port() {
    let (target, stop_target, peers) = multi_response_target().await;
    let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default()).unwrap();
    let spec =
        DatagramProxySpec::new("multi", "127.0.0.1:0".parse().unwrap(), target, limits()).unwrap();
    let view = runtime.create_proxy(spec).await.unwrap();
    let listen = view.bound_addr.unwrap();
    let c1 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let c2 = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    c1.send_to(b"a", listen).await.unwrap();
    c2.send_to(b"b", listen).await.unwrap();
    let mut buf = [0u8; 64];
    let mut a = Vec::new();
    let mut b = Vec::new();
    for _ in 0..2 {
        let (n, _) = timeout(Duration::from_secs(2), c1.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        a.push(buf[..n].to_vec());
    }
    for _ in 0..2 {
        let (n, _) = timeout(Duration::from_secs(2), c2.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        b.push(buf[..n].to_vec());
    }
    assert!(a.iter().all(|x| x.starts_with(b"a:")));
    assert!(b.iter().all(|x| x.starts_with(b"b:")));
    let first_peers = peers.lock().unwrap().clone();
    assert_eq!(first_peers.len(), 2);
    assert_ne!(first_peers[0], first_peers[1]);
    c1.send_to(b"a2", listen).await.unwrap();
    for _ in 0..2 {
        let _ = timeout(Duration::from_secs(2), c1.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(peers.lock().unwrap()[2], first_peers[0]);
    let push = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    push.send_to(b"push", listen).await.unwrap();
    let (n1, _) = timeout(Duration::from_secs(2), c1.recv_from(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&buf[..n1], b"unsolicited");
    let (n2, _) = timeout(Duration::from_secs(2), c2.recv_from(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&buf[..n2], b"unsolicited");
    runtime.shutdown().await;
    assert!(runtime.proxies().await.is_empty());
    stop_target.cancel();
}

#[tokio::test]
async fn delayed_datagram_prevents_idle_association_expiry() {
    let (target, stop_target) = echo_target().await;
    let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default()).unwrap();
    let mut spec =
        DatagramProxySpec::new("delay", "127.0.0.1:0".parse().unwrap(), target, limits()).unwrap();
    spec.association_idle_timeout = Duration::from_millis(30);
    let fault = eggchaos_core::DatagramFaultSpec {
        id: eggchaos_core::FaultId::new("delay").unwrap(),
        probability: eggchaos_core::Probability::new(1.0).unwrap(),
        kind: eggchaos_core::DatagramFaultKind::Delay {
            delay: Duration::from_millis(180),
            jitter: Duration::ZERO,
        },
    };
    spec.upstream_policy
        .publish(DatagramPlan::new(vec![fault]).unwrap(), 0)
        .unwrap();
    let view = runtime.create_proxy(spec).await.unwrap();
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client
        .send_to(b"held", view.bound_addr.unwrap())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(90)).await;
    assert_eq!(runtime.associations().await.len(), 1);
    let mut buf = [0u8; 64];
    let (size, _) = timeout(Duration::from_secs(2), client.recv_from(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&buf[..size], b"held");
    runtime.shutdown().await;
    stop_target.cancel();
}

#[tokio::test]
async fn duplicate_policy_is_embedded_in_both_directions() {
    let (target, stop_target) = echo_target().await;
    let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default()).unwrap();
    let spec = DatagramProxySpec::new(
        "duplicate",
        "127.0.0.1:0".parse().unwrap(),
        target,
        limits(),
    )
    .unwrap();
    let duplicate = eggchaos_core::DatagramFaultSpec {
        id: eggchaos_core::FaultId::new("duplicate").unwrap(),
        probability: eggchaos_core::Probability::new(1.0).unwrap(),
        kind: eggchaos_core::DatagramFaultKind::Duplicate {
            additional_copies: 1,
        },
    };
    let plan = DatagramPlan::new(vec![duplicate]).unwrap();
    spec.upstream_policy.publish(plan.clone(), 0).unwrap();
    spec.downstream_policy.publish(plan, 0).unwrap();
    let view = runtime.create_proxy(spec).await.unwrap();
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client
        .send_to(b"twice", view.bound_addr.unwrap())
        .await
        .unwrap();
    let mut buf = [0u8; 32];
    for _ in 0..4 {
        let (size, _) = timeout(Duration::from_secs(2), client.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf[..size], b"twice");
    }
    let snapshot = runtime.associations().await.remove(0);
    assert_eq!(snapshot.upstream_evidence.duplicated_copies, 1);
    assert_eq!(snapshot.downstream_evidence.duplicated_copies, 2);
    runtime.shutdown().await;
    stop_target.cancel();
}

#[tokio::test]
async fn restart_patch_bind_failure_keeps_old_udp_listener_serving() {
    let (target, stop_target) = echo_target().await;
    let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default()).unwrap();
    let spec = DatagramProxySpec::new("rollback", "127.0.0.1:0".parse().unwrap(), target, limits())
        .unwrap();
    let original = runtime.create_proxy(spec).await.unwrap();
    let blocker = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let conflict_addr = blocker.local_addr().unwrap();
    let result = runtime
        .update_proxy("rollback", Some(conflict_addr), None, None, None, None)
        .await;
    assert!(matches!(result, Err(DatagramRuntimeError::Bind(_))));
    let current = runtime.proxy("rollback").await.unwrap();
    assert_eq!(current.bound_addr, original.bound_addr);
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client
        .send_to(b"still-serving", current.bound_addr.unwrap())
        .await
        .unwrap();
    let mut buf = [0u8; 32];
    let (size, _) = timeout(Duration::from_secs(2), client.recv_from(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&buf[..size], b"still-serving");
    runtime.shutdown().await;
    stop_target.cancel();
}

#[tokio::test]
async fn ipv6_loopback_works_when_host_capability_is_available() {
    let target_socket = match UdpSocket::bind("[::1]:0").await {
        Ok(socket) => socket,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::AddrNotAvailable | io::ErrorKind::Unsupported
            ) =>
        {
            println!("SKIP IPv6 UDP loopback unavailable: {error}");
            return;
        }
        Err(error) => panic!("IPv6 loopback bind failed unexpectedly: {error}"),
    };
    let target = target_socket.local_addr().unwrap();
    let target_cancel = CancellationToken::new();
    let target_done = target_cancel.clone();
    let target_task = tokio::spawn(async move {
        let mut buf = [0u8; 65_536];
        loop {
            tokio::select! {
                _ = target_done.cancelled() => break,
                result = target_socket.recv_from(&mut buf) => if let Ok((size, peer)) = result { let _ = target_socket.send_to(&buf[..size], peer).await; }
            }
        }
    });
    let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default()).unwrap();
    let spec = DatagramProxySpec::new("v6", "[::1]:0".parse().unwrap(), target, limits()).unwrap();
    let proxy = match runtime.create_proxy(spec).await {
        Ok(proxy) => proxy,
        Err(DatagramRuntimeError::Bind(error))
            if matches!(
                error.kind(),
                io::ErrorKind::AddrNotAvailable | io::ErrorKind::Unsupported
            ) =>
        {
            println!("SKIP IPv6 UDP listener unavailable: {error}");
            target_cancel.cancel();
            target_task.await.unwrap();
            return;
        }
        Err(error) => panic!("unexpected IPv6 proxy error: {error}"),
    };
    let client = match UdpSocket::bind("[::1]:0").await {
        Ok(client) => client,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::AddrNotAvailable | io::ErrorKind::Unsupported
            ) =>
        {
            println!("SKIP IPv6 UDP client unavailable: {error}");
            runtime.shutdown().await;
            target_cancel.cancel();
            target_task.await.unwrap();
            return;
        }
        Err(error) => panic!("IPv6 client bind failed unexpectedly: {error}"),
    };
    client
        .send_to(b"v6", proxy.bound_addr.unwrap())
        .await
        .unwrap();
    let mut buf = [0u8; 8];
    let (size, _) = timeout(Duration::from_secs(2), client.recv_from(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&buf[..size], b"v6");
    runtime.shutdown().await;
    target_cancel.cancel();
    target_task.await.unwrap();
    println!("PASS IPv6 UDP loopback relay");
}

#[tokio::test]
async fn concurrent_first_datagrams_for_one_client_create_one_association() {
    let (target, stop_target) = echo_target().await;
    // A deep test-only ingress bound keeps all 32 racing datagrams; the
    // point here is setup convergence, not the bounded-ingress drop path.
    let runtime = DatagramRuntime::new(DatagramRuntimeLimits {
        ingress_per_association: 64,
        ..DatagramRuntimeLimits::default()
    })
    .unwrap();
    let spec =
        DatagramProxySpec::new("race", "127.0.0.1:0".parse().unwrap(), target, limits()).unwrap();
    let view = runtime.create_proxy(spec).await.unwrap();
    let listen = view.bound_addr.unwrap();
    let client = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..32u8 {
        let client = client.clone();
        let seen = seen.clone();
        tasks.spawn(async move {
            let payload = [index; 8];
            client.send_to(&payload, listen).await.unwrap();
            // Tasks share one socket, so each receives an arbitrary reply
            // from the racing wave; the set of all replies must match.
            let mut buf = [0u8; 32];
            let (size, _) = timeout(Duration::from_secs(5), client.recv_from(&mut buf))
                .await
                .unwrap()
                .unwrap();
            seen.lock().expect("seen set").push(buf[..size].to_vec());
        });
    }
    while let Some(outcome) = tasks.join_next().await {
        outcome.unwrap();
    }
    let mut seen = seen.lock().expect("seen set").clone();
    seen.sort();
    let mut expected = Vec::new();
    for index in 0..32u8 {
        expected.push(vec![index; 8]);
    }
    assert_eq!(seen, expected);
    // Exactly one association owns this client address: racing setups must
    // converge rather than publish duplicates or leak reservations.
    let associations = runtime.associations().await;
    assert_eq!(associations.len(), 1);
    assert_eq!(associations[0].ingress_datagrams, 32);
    assert_eq!(associations[0].egress_datagrams, 64);
    let view = runtime.proxy("race").await.unwrap();
    assert_eq!(view.active_associations, 1);
    runtime.delete_proxy("race").await.unwrap();
    assert!(runtime.associations().await.is_empty());
    stop_target.cancel();
}

#[tokio::test]
async fn simultaneous_new_clients_respect_global_and_proxy_caps() {
    let (target, stop_target) = echo_target().await;
    let runtime = DatagramRuntime::new(DatagramRuntimeLimits {
        max_associations: 4,
        ..DatagramRuntimeLimits::default()
    })
    .unwrap();
    let mut spec =
        DatagramProxySpec::new("caps", "127.0.0.1:0".parse().unwrap(), target, limits()).unwrap();
    spec.max_associations = 4;
    let view = runtime.create_proxy(spec).await.unwrap();
    let listen = view.bound_addr.unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..8 {
        tasks.spawn(async move {
            let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            client.send_to(b"hello", listen).await.unwrap();
            let mut buf = [0u8; 32];
            let _ = timeout(Duration::from_secs(3), client.recv_from(&mut buf)).await;
        });
    }
    while let Some(outcome) = tasks.join_next().await {
        outcome.unwrap();
    }
    // Capacity holds exactly: no duplicate slots, no over-admission, and
    // rejected clients are counted rather than silently dropped.
    let associations = runtime.associations().await;
    assert_eq!(associations.len(), 4);
    let view = timeout(Duration::from_secs(2), async {
        loop {
            let view = runtime.proxy("caps").await.unwrap();
            if view.association_capacity_rejections == 4 {
                break view;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(view.association_capacity_rejections, 4);
    // Killing every live association releases global capacity exactly, so
    // a full new wave fits again with no leaked reservation.
    for association in runtime.associations().await {
        assert!(runtime.kill_association(association.id).await);
    }
    for _ in 0..4 {
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.send_to(b"again", listen).await.unwrap();
        let mut buf = [0u8; 32];
        let (size, _) = timeout(Duration::from_secs(2), client.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf[..size], b"again");
    }
    assert_eq!(runtime.associations().await.len(), 4);
    runtime.delete_proxy("caps").await.unwrap();
    stop_target.cancel();
}

#[tokio::test]
async fn delete_during_setup_storm_leaves_no_leaked_capacity() {
    let (target, stop_target) = echo_target().await;
    let runtime = DatagramRuntime::new(DatagramRuntimeLimits {
        max_associations: 8,
        ..DatagramRuntimeLimits::default()
    })
    .unwrap();
    let mut spec =
        DatagramProxySpec::new("storm", "127.0.0.1:0".parse().unwrap(), target, limits()).unwrap();
    spec.max_associations = 8;
    let view = runtime.create_proxy(spec).await.unwrap();
    let listen = view.bound_addr.unwrap();
    // Blast distinct new clients while deleting the proxy mid-storm: setup
    // reservations abandoned by the drain must release global capacity.
    let sender = tokio::spawn(async move {
        for _ in 0..64 {
            if let Ok(client) = UdpSocket::bind("127.0.0.1:0").await {
                let _ = client.send_to(b"x", listen).await;
            }
            tokio::task::yield_now().await;
        }
    });
    tokio::task::yield_now().await;
    runtime.delete_proxy("storm").await.unwrap();
    sender.await.unwrap();
    // A fresh proxy on the same runtime can still use the full global
    // budget, proving no Starting reservation or task leaked.
    let mut spec = DatagramProxySpec::new(
        "storm-after",
        "127.0.0.1:0".parse().unwrap(),
        target,
        limits(),
    )
    .unwrap();
    spec.max_associations = 8;
    let view = runtime.create_proxy(spec).await.unwrap();
    let listen = view.bound_addr.unwrap();
    for _ in 0..8 {
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.send_to(b"clean", listen).await.unwrap();
        let mut buf = [0u8; 32];
        let (size, _) = timeout(Duration::from_secs(2), client.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf[..size], b"clean");
    }
    assert_eq!(runtime.associations().await.len(), 8);
    runtime.shutdown().await;
    assert!(runtime.associations().await.is_empty());
    stop_target.cancel();
}

#[tokio::test]
async fn starting_publication_wakes_all_waiters_with_one_association() {
    let (runtime, state, socket, _client, client_addr, hook, stop_target) =
        direct_setup(DatagramRuntimeLimits::default(), 256).await;
    hook.hold_before_bind();
    let owner = spawn_resolve(socket.clone(), state.clone(), client_addr);
    hook.wait_before_bind().await;
    let reservation = starting_reservation(&state, client_addr).await;
    let mut waiters = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let state = state.clone();
        let socket = socket.clone();
        waiters.spawn(async move { resolve_association(socket, &state, client_addr).await });
    }
    reservation.wait_for_waiters(16).await;
    hook.release_before_bind();
    let owner_association = owner.await.unwrap().unwrap();
    let waiter_associations = join_associations(&mut waiters).await;
    assert!(waiter_associations
        .iter()
        .all(|association| Arc::ptr_eq(association, &owner_association)));
    assert_eq!(state.proxy_active.load(Ordering::Acquire), 1);
    assert_eq!(state.global_active.load(Ordering::Acquire), 1);
    assert_eq!(runtime.associations().await.len(), 1);
    runtime.delete_proxy("direct").await.unwrap();
    assert_eq!(state.proxy_active.load(Ordering::Acquire), 0);
    assert_eq!(state.global_active.load(Ordering::Acquire), 0);
    stop_target.cancel();
}

#[tokio::test]
async fn starting_failure_wakes_waiters_and_allows_one_takeover() {
    let (runtime, state, socket, _client, client_addr, hook, stop_target) =
        direct_setup(DatagramRuntimeLimits::default(), 256).await;
    hook.hold_before_bind();
    hook.fail_next();
    let owner = spawn_resolve(socket.clone(), state.clone(), client_addr);
    hook.wait_before_bind().await;
    let reservation = starting_reservation(&state, client_addr).await;
    let mut waiters = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let state = state.clone();
        let socket = socket.clone();
        waiters.spawn(async move { resolve_association(socket, &state, client_addr).await });
    }
    reservation.wait_for_waiters(8).await;
    hook.release_before_bind();
    assert!(matches!(
        owner.await.unwrap(),
        Err(DatagramRuntimeError::Bind(_))
    ));
    let associations = join_associations(&mut waiters).await;
    assert_eq!(associations.len(), 8);
    assert!(associations
        .iter()
        .all(|association| Arc::ptr_eq(association, &associations[0])));
    assert_eq!(runtime.associations().await.len(), 1);
    assert_eq!(state.proxy_active.load(Ordering::Acquire), 1);
    runtime.delete_proxy("direct").await.unwrap();
    assert_eq!(state.global_active.load(Ordering::Acquire), 0);
    stop_target.cancel();
}

#[tokio::test]
async fn starting_transition_is_retained_for_late_wait_registration() {
    let (runtime, state, socket, _client, client_addr, hook, stop_target) =
        direct_setup(DatagramRuntimeLimits::default(), 256).await;
    hook.hold_before_bind();
    let owner = spawn_resolve(socket.clone(), state.clone(), client_addr);
    hook.wait_before_bind().await;
    let reservation = starting_reservation(&state, client_addr).await;
    assert!(super::association::drain_associations(&state, false)
        .await
        .is_empty());
    assert!(super::association::drain_associations(&state, false)
        .await
        .is_empty());
    reservation.wait_for_terminal().await;
    assert_eq!(state.proxy_active.load(Ordering::Acquire), 0);
    hook.release_before_bind();
    assert!(matches!(
        owner.await.unwrap(),
        Err(DatagramRuntimeError::Conflict(_))
    ));
    assert_eq!(state.global_active.load(Ordering::Acquire), 0);
    runtime.delete_proxy("direct").await.unwrap();
    stop_target.cancel();
}

#[tokio::test]
async fn concurrent_clients_reserve_global_and_proxy_capacity_once() {
    let (runtime, state, socket, _client, client_addr, hook, stop_target) = direct_setup(
        DatagramRuntimeLimits {
            max_associations: 1,
            ..DatagramRuntimeLimits::default()
        },
        1,
    )
    .await;
    hook.hold_before_bind();
    let owner = spawn_resolve(socket.clone(), state.clone(), client_addr);
    hook.wait_before_bind().await;
    let other_client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let other_addr = other_client.local_addr().unwrap();
    assert!(matches!(
        resolve_association(socket.clone(), &state, other_addr).await,
        Err(DatagramRuntimeError::AssociationLimit)
    ));
    hook.release_before_bind();
    owner.await.unwrap().unwrap();
    assert_eq!(state.proxy_active.load(Ordering::Acquire), 1);
    assert_eq!(state.global_active.load(Ordering::Acquire), 1);
    runtime.delete_proxy("direct").await.unwrap();
    assert_eq!(state.proxy_active.load(Ordering::Acquire), 0);
    assert_eq!(state.global_active.load(Ordering::Acquire), 0);
    stop_target.cancel();
}

#[tokio::test]
async fn waiter_cancellation_does_not_cancel_setup_owner() {
    let (runtime, state, socket, _client, client_addr, hook, stop_target) =
        direct_setup(DatagramRuntimeLimits::default(), 256).await;
    hook.hold_before_bind();
    let owner = spawn_resolve(socket.clone(), state.clone(), client_addr);
    hook.wait_before_bind().await;
    let reservation = starting_reservation(&state, client_addr).await;
    let waiter = spawn_resolve(socket.clone(), state.clone(), client_addr);
    reservation.wait_for_waiters(1).await;
    waiter.abort();
    assert!(matches!(waiter.await, Err(error) if error.is_cancelled()));
    hook.release_before_bind();
    let association = owner.await.unwrap().unwrap();
    let active = runtime.associations().await;
    assert_eq!(active.len(), 1);
    assert_eq!(association.id, active[0].id);
    runtime.delete_proxy("direct").await.unwrap();
    assert_eq!(state.global_active.load(Ordering::Acquire), 0);
    stop_target.cancel();
}

#[tokio::test]
async fn aborted_setup_owner_releases_reservation_and_worker() {
    let (runtime, state, socket, _client, client_addr, hook, stop_target) =
        direct_setup(DatagramRuntimeLimits::default(), 256).await;
    hook.hold_before_publish();
    let owner = spawn_resolve(socket, state.clone(), client_addr);
    hook.wait_before_publish().await;
    owner.abort();
    assert!(matches!(owner.await, Err(error) if error.is_cancelled()));
    assert_eq!(state.proxy_active.load(Ordering::Acquire), 0);
    assert_eq!(state.global_active.load(Ordering::Acquire), 0);
    assert!(state
        .associations
        .read()
        .expect("datagram association registry")
        .get(&client_addr)
        .is_none());
    runtime.delete_proxy("direct").await.unwrap();
    stop_target.cancel();
}

#[tokio::test]
async fn update_drain_releases_reservation_and_stale_owner_cannot_publish() {
    let (runtime, state, socket, _client, client_addr, hook, stop_target) =
        direct_setup(DatagramRuntimeLimits::default(), 256).await;
    let (new_target, stop_new_target) = echo_target().await;
    hook.hold_before_bind();
    let owner = spawn_resolve(socket.clone(), state.clone(), client_addr);
    hook.wait_before_bind().await;
    let reservation = starting_reservation(&state, client_addr).await;
    let mut waiters = tokio::task::JoinSet::new();
    let waiter_state = state.clone();
    let waiter_socket = socket.clone();
    waiters
        .spawn(async move { resolve_association(waiter_socket, &waiter_state, client_addr).await });
    reservation.wait_for_waiters(1).await;
    runtime
        .update_proxy("direct", None, Some(new_target), None, None, None)
        .await
        .unwrap();
    hook.release_before_bind();
    assert!(matches!(
        owner.await.unwrap(),
        Err(DatagramRuntimeError::Conflict(_))
    ));
    let associations = join_associations(&mut waiters).await;
    assert_eq!(associations.len(), 1);
    assert_eq!(runtime.associations().await[0].upstream, new_target);
    assert_eq!(state.proxy_active.load(Ordering::Acquire), 1);
    runtime.delete_proxy("direct").await.unwrap();
    assert_eq!(state.global_active.load(Ordering::Acquire), 0);
    stop_target.cancel();
    stop_new_target.cancel();
}

#[tokio::test]
async fn disable_drains_an_unpublished_setup_worker() {
    let (runtime, state, socket, _client, client_addr, hook, stop_target) =
        direct_setup(DatagramRuntimeLimits::default(), 256).await;
    hook.hold_before_publish();
    let owner = spawn_resolve(socket.clone(), state.clone(), client_addr);
    hook.wait_before_publish().await;
    runtime.disable_proxy("direct").await.unwrap();
    assert_eq!(state.proxy_active.load(Ordering::Acquire), 0);
    assert_eq!(state.global_active.load(Ordering::Acquire), 0);
    hook.release_before_publish();
    assert!(matches!(
        owner.await.unwrap(),
        Err(DatagramRuntimeError::Conflict(_))
    ));
    assert!(state
        .associations
        .read()
        .expect("datagram association registry")
        .get(&client_addr)
        .is_none());
    runtime.enable_proxy("direct").await.unwrap();
    let association = resolve_association(socket, &state, client_addr)
        .await
        .unwrap();
    assert_eq!(state.proxy_active.load(Ordering::Acquire), 1);
    assert_eq!(association.id, runtime.associations().await[0].id);
    runtime.delete_proxy("direct").await.unwrap();
    assert_eq!(state.global_active.load(Ordering::Acquire), 0);
    stop_target.cancel();
}

#[tokio::test]
async fn delete_drain_releases_capacity_without_double_release() {
    let (runtime, state, socket, _client, client_addr, hook, stop_target) =
        direct_setup(DatagramRuntimeLimits::default(), 256).await;
    hook.hold_before_publish();
    let owner = spawn_resolve(socket.clone(), state.clone(), client_addr);
    hook.wait_before_publish().await;
    assert!(runtime.delete_proxy("direct").await.unwrap());
    assert_eq!(state.proxy_active.load(Ordering::Acquire), 0);
    assert_eq!(state.global_active.load(Ordering::Acquire), 0);
    hook.release_before_publish();
    assert!(matches!(
        owner.await.unwrap(),
        Err(DatagramRuntimeError::Conflict(_))
    ));
    assert_eq!(state.global_active.load(Ordering::Acquire), 0);
    stop_target.cancel();
}
