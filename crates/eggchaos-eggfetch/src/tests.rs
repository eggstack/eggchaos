//! M029 composition, identity, evidence, and pooling tests.

use std::{
    num::NonZeroU64,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use eggchaos_core::{
    DisconnectConfig, FaultId, FaultKind, FaultPlan, FaultSpec, LatencyConfig, Probability,
};
use eggfetch_core::{DialError, DialErrorKind, DialFuture, DialStream, DialTarget, Dialer};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use super::*;

fn fault(id: &str, kind: FaultKind) -> FaultSpec {
    FaultSpec {
        id: FaultId::new(id).unwrap(),
        probability: Probability::new(1.0).unwrap(),
        kind,
    }
}

fn latency_plan(id: &str, delay_ms: u64) -> FaultPlan {
    FaultPlan::new(vec![fault(
        id,
        FaultKind::Latency(LatencyConfig {
            delay: Duration::from_millis(delay_ms),
            jitter: Duration::ZERO,
            max_buffer_bytes: NonZeroU64::new(1 << 20).unwrap(),
        }),
    )])
    .unwrap()
}

fn disconnect_plan() -> FaultPlan {
    FaultPlan::new(vec![fault(
        "bye",
        FaultKind::Disconnect(DisconnectConfig {
            after: Duration::ZERO,
            hard_reset: false,
        }),
    )])
    .unwrap()
}

/// Inner dialer that records every target and opens a real TCP connection
/// to it. The dial count proves eggchaos never redials.
#[derive(Debug, Clone, Default)]
struct RecordDialer {
    dials: Arc<AtomicU64>,
    targets: Arc<Mutex<Vec<DialTarget>>>,
}

impl RecordDialer {
    fn dial_count(&self) -> u64 {
        self.dials.load(Ordering::SeqCst)
    }

    fn targets(&self) -> Vec<DialTarget> {
        self.targets.lock().expect("targets lock").clone()
    }
}

impl Dialer for RecordDialer {
    fn dial(&self, target: DialTarget) -> DialFuture<'_> {
        let dials = self.dials.clone();
        let targets = self.targets.clone();
        Box::pin(async move {
            dials.fetch_add(1, Ordering::SeqCst);
            targets.lock().expect("targets lock").push(target.clone());
            let stream = tokio::net::TcpStream::connect((target.host(), target.port()))
                .await
                .map_err(|error| {
                    DialError::with_source(
                        DialErrorKind::Connection,
                        "record route connection failed",
                        error,
                    )
                })?;
            Ok(Box::new(stream) as DialStream)
        })
    }
}

/// Route-authoritative inner dialer: connects to a fixed test address no
/// matter which logical target was requested. This mimics a routed
/// (multi-hop) dialer and proves eggchaos neither resolves nor redials.
#[derive(Debug, Clone)]
struct RouteDialer {
    fixed: std::net::SocketAddr,
    dials: Arc<AtomicU64>,
    targets: Arc<Mutex<Vec<DialTarget>>>,
}

impl Dialer for RouteDialer {
    fn dial(&self, target: DialTarget) -> DialFuture<'_> {
        let fixed = self.fixed;
        let dials = self.dials.clone();
        let targets = self.targets.clone();
        Box::pin(async move {
            dials.fetch_add(1, Ordering::SeqCst);
            targets.lock().expect("targets lock").push(target);
            let stream = tokio::net::TcpStream::connect(fixed)
                .await
                .map_err(|error| {
                    DialError::with_source(
                        DialErrorKind::Connection,
                        "route connection failed",
                        error,
                    )
                })?;
            Ok(Box::new(stream) as DialStream)
        })
    }
}

/// Inner dialer that always fails with one configured error.
#[derive(Debug, Clone)]
struct FailDialer {
    kind: DialErrorKind,
    message: &'static str,
}

impl Dialer for FailDialer {
    fn dial(&self, _target: DialTarget) -> DialFuture<'_> {
        let (kind, message) = (self.kind, self.message);
        Box::pin(async move { Err(DialError::new(kind, message)) })
    }
}

/// Inner dialer returning an in-memory duplex stream (no network).
#[derive(Debug, Clone, Default)]
struct DuplexDialer {
    dials: Arc<AtomicU64>,
}

impl Dialer for DuplexDialer {
    fn dial(&self, _target: DialTarget) -> DialFuture<'_> {
        let dials = self.dials.clone();
        Box::pin(async move {
            dials.fetch_add(1, Ordering::SeqCst);
            let (client, _server) = tokio::io::duplex(65_536);
            Ok(Box::new(client) as DialStream)
        })
    }
}

/// Provider recording every derivation input; returns ordinal + salt.
#[derive(Debug, Default)]
struct ScriptProvider {
    salt: u64,
    seen: Mutex<Vec<(u64, DialTarget, String)>>,
}

impl ScriptProvider {
    fn seen(&self) -> Vec<(u64, DialTarget, String)> {
        self.seen.lock().expect("seen lock").clone()
    }
}

impl ConnectionKeyProvider for ScriptProvider {
    fn connection_key(&self, ctx: &ConnectionKeyContext<'_>) -> Result<u64, KeyError> {
        self.seen.lock().expect("seen lock").push((
            ctx.ordinal,
            ctx.target.clone(),
            ctx.integration_id.to_owned(),
        ));
        Ok(ctx.ordinal.wrapping_add(self.salt))
    }
}

/// Provider that always fails.
#[derive(Debug, Clone, Copy, Default)]
struct FailProvider;

impl ConnectionKeyProvider for FailProvider {
    fn connection_key(&self, _ctx: &ConnectionKeyContext<'_>) -> Result<u64, KeyError> {
        Err(KeyError::new("provider exploded"))
    }
}

fn recorder(capacity: usize) -> (Arc<RecordingObserver>, Arc<dyn ConnectionObserver>) {
    let observer = Arc::new(RecordingObserver::new(capacity));
    let sink: Arc<dyn ConnectionObserver> = observer.clone();
    (observer, sink)
}

fn target(host: &str, port: u16) -> DialTarget {
    DialTarget::new(host, port)
}

/// Read one HTTP request head (until blank line) from a server stream.
/// Returns an empty vector on EOF or error so idle keep-alive closes and
/// client disconnects never panic the server.
async fn read_request_head(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if stream.read_exact(&mut byte).await.is_err() {
            return Vec::new();
        }
        head.push(byte[0]);
        if head.len() >= 4 && &head[head.len() - 4..] == b"\r\n\r\n" {
            return head;
        }
    }
}

const HTTP_OK: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\nok";

/// Serve `requests` HTTP exchanges across accepted connections, then stop
/// accepting. Each connection is handled on its own task so one idle
/// keep-alive connection never starves another connection's accept.
async fn serve_http(listener: TcpListener, requests: usize) {
    use std::sync::atomic::AtomicUsize;
    let remaining = Arc::new(AtomicUsize::new(requests));
    while remaining.load(Ordering::SeqCst) > 0 {
        let accept = tokio::time::timeout(Duration::from_secs(30), listener.accept()).await;
        let Ok(Ok((mut stream, _))) = accept else {
            break;
        };
        let remaining = remaining.clone();
        tokio::spawn(async move {
            loop {
                if read_request_head(&mut stream).await.is_empty() {
                    break;
                }
                // Claim one budgeted response. If the budget is already
                // spent (no test should do this), still answer so no
                // request ever goes unanswered, then stop.
                let claimed = remaining
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                        (n > 0).then(|| n - 1)
                    })
                    .is_ok();
                if stream.write_all(HTTP_OK).await.is_err() {
                    break;
                }
                if !claimed {
                    break;
                }
            }
        });
    }
}

async fn http_body(client: &eggfetch_core::Client, url: &str) -> Vec<u8> {
    let mut response = client.get(url).unwrap().send().await.unwrap();
    response.bytes().await.unwrap().as_ref().to_vec()
}

#[tokio::test]
async fn custom_dialer_target_forwarded_with_single_attempt() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        stream.write_all(b"ok").await.unwrap();
    });
    let inner = RecordDialer::default();
    let dialer = ChaosDialer::wrap(inner.clone(), 42, "test");
    let mut stream = dialer
        .dial(target("127.0.0.1", address.port()))
        .await
        .unwrap();
    assert_eq!(inner.dial_count(), 1);
    assert_eq!(inner.targets(), vec![target("127.0.0.1", address.port())]);
    let mut bytes = [0; 2];
    stream.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"ok");
    task.await.unwrap();
}

#[tokio::test]
async fn routed_inner_dialer_is_never_resolved_or_redialed() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fixed = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        stream.write_all(b"ok").await.unwrap();
    });
    let inner = RouteDialer {
        fixed,
        dials: Arc::new(AtomicU64::new(0)),
        targets: Arc::new(Mutex::new(Vec::new())),
    };
    let dialer = ChaosDialer::wrap(inner.clone(), 7, "routed");
    // The requested host does not resolve to the test listener; only the
    // route-authoritative inner dialer decides where bytes go.
    let mut stream = dialer.dial(target("example.invalid", 80)).await.unwrap();
    assert_eq!(inner.dials.load(Ordering::SeqCst), 1);
    assert_eq!(
        inner.targets.lock().unwrap().clone(),
        vec![target("example.invalid", 80)]
    );
    let mut bytes = [0; 2];
    stream.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"ok");
    task.await.unwrap();
}

#[tokio::test]
async fn inner_error_kinds_pass_through_with_observer_silence() {
    for kind in [
        DialErrorKind::Connection,
        DialErrorKind::Timeout,
        DialErrorKind::Authentication,
        DialErrorKind::Rejected,
        DialErrorKind::Other,
    ] {
        let (observer, sink) = recorder(16);
        let dialer = ChaosDialer::wrap(
            FailDialer {
                kind,
                message: "boom",
            },
            1,
            "p",
        )
        .with_observer(sink);
        let error = match dialer.dial(target("127.0.0.1", 1)).await {
            Ok(_) => panic!("inner failure must fail the dial"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), kind);
        assert_eq!(error.message(), "boom");
        assert!(observer.is_empty(), "failed dials create no evidence");
    }
}

#[tokio::test]
async fn direct_convenience_mode_still_resolves_and_connects() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        stream.write_all(b"ok").await.unwrap();
    });
    let (observer, sink) = recorder(16);
    let dialer = ChaosDialer::new(42, "test").with_observer(sink);
    let mut stream = dialer
        .dial(target("127.0.0.1", address.port()))
        .await
        .unwrap();
    let mut bytes = [0; 2];
    stream.read_exact(&mut bytes).await.unwrap();
    assert_eq!(&bytes, b"ok");
    assert_eq!(observer.len(), 1);
    assert_eq!(observer.records()[0].connection_key, 1);
    task.await.unwrap();
}

#[tokio::test]
async fn provider_receives_expected_ordinal_target_and_identity() {
    let provider = Arc::new(ScriptProvider {
        salt: 1000,
        seen: Mutex::new(Vec::new()),
    });
    let key_provider: Arc<dyn ConnectionKeyProvider> = provider.clone();
    let dialer = ChaosDialer::wrap(DuplexDialer::default(), 9, "p")
        .with_integration_id("exp-1")
        .unwrap()
        .with_key_provider(key_provider);
    let _stream = dialer.dial(target("127.0.0.1", 8080)).await.unwrap();
    assert_eq!(
        provider.seen(),
        vec![(1, target("127.0.0.1", 8080), "exp-1".to_owned())]
    );
}

#[tokio::test]
async fn identical_provider_inputs_give_identical_keys() {
    let provider = DefaultConnectionKeyProvider;
    let target = target("host", 80);
    let first = provider
        .connection_key(&ConnectionKeyContext {
            ordinal: 3,
            target: &target,
            integration_id: "exp",
        })
        .unwrap();
    let second = provider
        .connection_key(&ConnectionKeyContext {
            ordinal: 3,
            target: &target,
            integration_id: "exp",
        })
        .unwrap();
    assert_eq!(first, 3);
    assert_eq!(first, second);
}

#[tokio::test]
async fn default_keys_follow_successful_dial_ordinals() {
    let (observer, sink) = recorder(16);
    let dialer = ChaosDialer::wrap(DuplexDialer::default(), 5, "p").with_observer(sink);
    // A failed dial consumes no ordinal: only successful physical dials
    // advance adapter-local identity.
    let failing = ChaosDialer::wrap(
        FailDialer {
            kind: DialErrorKind::Connection,
            message: "x",
        },
        5,
        "p",
    )
    .with_observer(observer.clone() as Arc<dyn ConnectionObserver>);
    assert!(failing.dial(target("h", 1)).await.is_err());
    let _a = dialer.dial(target("h", 1)).await.unwrap();
    let _b = dialer.dial(target("h", 1)).await.unwrap();
    let records = observer.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].ordinal, 1);
    assert_eq!(records[0].connection_key, 1);
    assert_eq!(records[1].ordinal, 2);
    assert_eq!(records[1].connection_key, 2);
}

#[tokio::test]
async fn caller_selected_collision_is_explicit_and_stable() {
    #[derive(Debug, Clone, Copy)]
    struct Constant;
    impl ConnectionKeyProvider for Constant {
        fn connection_key(&self, _ctx: &ConnectionKeyContext<'_>) -> Result<u64, KeyError> {
            Ok(77)
        }
    }
    let (observer, sink) = recorder(16);
    let dialer = ChaosDialer::wrap(DuplexDialer::default(), 5, "p")
        .with_key_provider(Arc::new(Constant))
        .with_observer(sink);
    let _a = dialer.dial(target("h", 1)).await.unwrap();
    let _b = dialer.dial(target("h", 1)).await.unwrap();
    let records = observer.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].connection_key, 77);
    assert_eq!(records[1].connection_key, 77);
    assert_ne!(records[0].ordinal, records[1].ordinal);
}

#[tokio::test]
async fn h1_keepalive_reuse_observes_one_physical_key() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { serve_http(listener, 2).await });
    let inner = RecordDialer::default();
    let (observer, sink) = recorder(16);
    let dialer = ChaosDialer::wrap(inner.clone(), 42, "test").with_observer(sink);
    let client = eggfetch_core::Client::builder().dialer(dialer).build();
    let url = format!("http://127.0.0.1:{}/", address.port());
    assert_eq!(http_body(&client, &url).await, b"ok");
    assert_eq!(http_body(&client, &url).await, b"ok");
    assert_eq!(
        inner.dial_count(),
        1,
        "keep-alive must reuse one connection"
    );
    let records = observer.records();
    assert_eq!(records.len(), 1);
    server.await.unwrap();
}

#[tokio::test]
async fn forced_separate_h1_connections_receive_distinct_keys() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { serve_http(listener, 2).await });
    let inner = RecordDialer::default();
    let (observer, sink) = recorder(16);
    let dialer = ChaosDialer::wrap(inner.clone(), 42, "test").with_observer(sink);
    // Separate client pools force separate physical connections.
    let first = eggfetch_core::Client::builder()
        .dialer(dialer.clone())
        .build();
    let second = eggfetch_core::Client::builder().dialer(dialer).build();
    let url = format!("http://127.0.0.1:{}/", address.port());
    assert_eq!(http_body(&first, &url).await, b"ok");
    assert_eq!(http_body(&second, &url).await, b"ok");
    assert_eq!(inner.dial_count(), 2);
    let records = observer.records();
    assert_eq!(records.len(), 2);
    assert_ne!(records[0].connection_key, records[1].connection_key);
    server.await.unwrap();
}

#[tokio::test]
async fn live_policy_update_reaches_pooled_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { serve_http(listener, 2).await });
    let (observer, sink) = recorder(16);
    let dialer = ChaosDialer::wrap(RecordDialer::default(), 42, "live").with_observer(sink);
    let policies = dialer.clone();
    let client = eggfetch_core::Client::builder().dialer(dialer).build();
    let url = format!("http://127.0.0.1:{}/", address.port());
    assert_eq!(http_body(&client, &url).await, b"ok");
    policies
        .publish_upstream(latency_plan("slow", 250))
        .unwrap();
    assert_eq!(http_body(&client, &url).await, b"ok");
    let records = observer.records();
    assert_eq!(records.len(), 1, "pooled connection observes live policy");
    let snapshot = records[0].evidence.snapshot();
    assert_eq!(snapshot.upstream.generation, 2);
    assert_eq!(snapshot.upstream.transitions, 1);
    assert!(snapshot.upstream.injected_delay_ms >= 250);
    assert_eq!(snapshot.upstream.active_faults.len(), 1);
    assert_eq!(snapshot.upstream.active_faults[0].id, "slow");
    server.await.unwrap();
}

#[tokio::test]
async fn evidence_records_bytes_and_survives_stream_drop() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 5];
        stream.read_exact(&mut bytes).await.unwrap();
        stream.write_all(&bytes).await.unwrap();
    });
    let (observer, sink) = recorder(16);
    let dialer = ChaosDialer::wrap(RecordDialer::default(), 3, "bytes").with_observer(sink);
    let mut stream = dialer
        .dial(target("127.0.0.1", address.port()))
        .await
        .unwrap();
    stream.write_all(b"hello").await.unwrap();
    stream.flush().await.unwrap();
    let mut out = [0; 5];
    stream.read_exact(&mut out).await.unwrap();
    assert_eq!(&out, b"hello");
    drop(stream);
    server.await.unwrap();
    let records = observer.records();
    assert_eq!(records.len(), 1);
    // Live handles stay readable after the network stream is dropped.
    let snapshot = records[0].evidence.snapshot();
    assert_eq!(snapshot.connection_key, 1);
    assert_eq!(snapshot.proxy, "bytes");
    assert_eq!(snapshot.upstream.bytes_accepted, 5);
    assert_eq!(snapshot.upstream.bytes_forwarded, 5);
    assert_eq!(snapshot.downstream.bytes_accepted, 5);
    assert_eq!(snapshot.downstream.bytes_forwarded, 5);
}

#[tokio::test]
async fn evidence_records_termination_without_payload() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 5];
        let _ = stream.read(&mut bytes).await;
    });
    let (observer, sink) = recorder(16);
    let dialer = ChaosDialer::wrap_with_policies(
        RecordDialer::default(),
        3,
        "term",
        disconnect_plan(),
        FaultPlan::empty(),
    )
    .with_observer(sink);
    let mut stream = dialer
        .dial(target("127.0.0.1", address.port()))
        .await
        .unwrap();
    // The disconnect fault accepts the open prefix, then the durable
    // termination request fails subsequent writes.
    stream.write_all(b"hello").await.unwrap();
    stream.flush().await.unwrap();
    let outcome = stream.write_all(b"more").await;
    assert!(outcome.is_err(), "disconnect must fail the write");
    drop(stream);
    server.await.unwrap();
    let snapshot = observer.records()[0].evidence.snapshot();
    assert_eq!(
        snapshot.upstream.termination,
        Some(eggchaos_core::TerminationRequest::Graceful)
    );
    assert_eq!(
        snapshot.upstream.termination_fault_id.as_deref(),
        Some("bye")
    );
    assert_eq!(snapshot.upstream.active_faults.len(), 1);
    assert_eq!(snapshot.upstream.active_faults[0].fault_type, "disconnect");
}

#[tokio::test]
async fn observer_called_once_per_wrapped_dial() {
    let (observer, sink) = recorder(16);
    let dialer = ChaosDialer::wrap(DuplexDialer::default(), 1, "p").with_observer(sink);
    let target = target("h", 9);
    let _a = dialer.dial(target.clone()).await.unwrap();
    let _b = dialer.dial(target).await.unwrap();
    let records = observer.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].ordinal, 1);
    assert_eq!(records[1].ordinal, 2);
}

#[tokio::test]
async fn provider_failure_fails_dial_with_bounded_error() {
    let (observer, sink) = recorder(16);
    let dialer = ChaosDialer::wrap(DuplexDialer::default(), 1, "p")
        .with_key_provider(Arc::new(FailProvider))
        .with_observer(sink);
    let error = match dialer.dial(target("h", 9)).await {
        Ok(_) => panic!("provider failure must fail the dial"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), DialErrorKind::Other);
    assert!(error.message().contains("provider exploded"));
    assert!(observer.is_empty());
}

#[tokio::test]
async fn recording_observer_evicts_oldest_first() {
    let (observer, sink) = recorder(2);
    let dialer = ChaosDialer::wrap(DuplexDialer::default(), 1, "p").with_observer(sink);
    for _ in 0..3 {
        let _stream = dialer.dial(target("h", 9)).await.unwrap();
    }
    let ordinals: Vec<u64> = observer
        .records()
        .iter()
        .map(|record| record.ordinal)
        .collect();
    assert_eq!(ordinals, vec![2, 3]);
}

#[test]
fn configuration_bounds_are_rejected() {
    let dialer = ChaosDialer::new(1, "p");
    assert_eq!(
        dialer.with_integration_id("x".repeat(129)).unwrap_err(),
        ChaosConfigError::IntegrationIdTooLong
    );
    assert!(ChaosDialer::new(1, "p")
        .with_integration_id("x".repeat(128))
        .is_ok());
    assert_eq!(
        DirectDialer::new()
            .with_connect_timeout(Duration::from_secs(301))
            .unwrap_err(),
        ChaosConfigError::ConnectTimeoutTooLong
    );
    assert!(DirectDialer::new()
        .with_connect_timeout(Duration::from_secs(300))
        .is_ok());
    let long = KeyError::new("y".repeat(300));
    assert_eq!(long.message().len(), MAX_KEY_ERROR_BYTES);
}

#[tokio::test]
async fn eggfetch_owns_http_over_the_physical_chaos_stream() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { serve_http(listener, 2).await });
    let dialer = ChaosDialer::new(42, "test");
    let client = eggfetch_core::Client::builder().dialer(dialer).build();
    let url = format!("http://127.0.0.1:{}/", address.port());
    assert_eq!(http_body(&client, &url).await, b"ok");
    let second = format!("http://127.0.0.1:{}/second", address.port());
    assert_eq!(http_body(&client, &second).await, b"ok");
    server.await.unwrap();
}

#[cfg(feature = "http2")]
#[tokio::test]
async fn eggfetch_owns_tls_and_http2_over_the_physical_chaos_stream() {
    use bytes::Bytes;
    use eggfetch_core::{HttpVersionPolicy, TlsConfig};
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use tokio_rustls::TlsAcceptor;

    // Every await below is bounded. An unbounded version of this test
    // once parked all three CI platforms for 4+ hours: cargo runs test
    // binaries serially, so one stuck test blocks the whole `cargo
    // test` step. Timeouts convert a future stall into a loud failure
    // naming the phase instead of silent runner burn.
    const CLIENT_PHASE: Duration = Duration::from_secs(120);
    const SERVER_JOIN: Duration = Duration::from_secs(60);

    let _ = rustls::crypto::ring::default_provider().install_default();
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(certificate.cert.der().to_vec())],
            PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
                certificate.key_pair.serialize_der(),
            )),
        )
        .unwrap();
    let mut server_config = server_config;
    server_config.alpn_protocols = vec![b"h2".to_vec()];
    let acceptor = TlsAcceptor::from(std::sync::Arc::new(server_config));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let stream = acceptor.accept(stream).await.unwrap();
        let mut connection = h2::server::handshake(stream).await.unwrap();
        for _ in 0..2 {
            let Some(Ok((_request, mut respond))) = connection.accept().await else {
                break;
            };
            let response = http::Response::new(());
            let mut send = respond.send_response(response, false).unwrap();
            send.send_data(Bytes::from_static(b"h2-ok"), true).unwrap();
        }
        // Close from the server side deterministically. `accept()`
        // returns `None` only on transport close, and a pooled h2 client
        // may hold the TCP connection open indefinitely, so never wait
        // for the peer here. A short bounded drain flushes GOAWAY/DATA;
        // afterwards dropping `connection` closes the transport
        // regardless of peer behavior.
        connection.graceful_shutdown();
        let _ = tokio::time::timeout(Duration::from_secs(5), async {
            while connection.accept().await.is_some() {}
        })
        .await;
    });

    let (observer, sink) = recorder(16);
    let dialer = ChaosDialer::new(42, "h2-test").with_observer(sink);
    let client = eggfetch_core::Client::builder()
        .dialer(dialer)
        .http_version_policy(HttpVersionPolicy::Http2Only)
        .tls_config(TlsConfig::default().danger_accept_invalid_certs(true))
        .build();
    let bodies = tokio::time::timeout(CLIENT_PHASE, async {
        let mut out = Vec::new();
        for _ in 0..2 {
            let mut response = client
                .get(&format!("https://localhost:{}/", address.port()))
                .unwrap()
                .send()
                .await
                .unwrap();
            out.push(response.bytes().await.unwrap().as_ref().to_vec());
        }
        out
    })
    .await
    .expect("client phase stalled: dial, TLS/h2 handshake, or body");
    assert_eq!(bodies, vec![b"h2-ok".to_vec(), b"h2-ok".to_vec()]);
    // Multiplexed requests share one physical connection and one key.
    assert_eq!(observer.len(), 1);
    // Drop the client before joining the server: a pooled h2 connection
    // stays open while the client lives, which would park the server's
    // post-`graceful_shutdown` accept loop forever. Client drop closes
    // pooled connections, so the join below is deterministic.
    drop(client);
    tokio::time::timeout(SERVER_JOIN, task)
        .await
        .expect("server join stalled after client drop")
        .unwrap();
}
