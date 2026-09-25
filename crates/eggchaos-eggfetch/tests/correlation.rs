//! M031 cross-layer correlation fixture.
//!
//! Fully in-process demonstration of the intended downstream contract
//! without importing any downstream product:
//!
//! ```text
//! custom workload
//!    -> EggFetch
//!    -> custom route-authoritative inner Dialer
//!    -> M029 chaos adapter
//!    -> local test service
//!
//! Scenario V2
//!    -> M030 prepared experiment
//!    -> shared epoch
//!    -> live policy publications
//! ```
//!
//! The final assertion correlates schedule fingerprint/seed/
//! execution key, event application/generation evidence, physical
//! connection key, active fault evidence, and the workload-observed
//! outcome. The fixture proves correlation, not exact kernel/network
//! timing replay.

use std::num::NonZeroU64;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;

use eggchaos_core::{Direction, FaultId, FaultKind, FaultSpec, LatencyConfig, Probability};
use eggchaos_eggfetch::{ChaosDialer, RecordingObserver};
use eggchaos_experiment::{
    EpochGate, IsolationPolicyV2, PreparedExperiment, ScenarioScheduleV2, SchedulePhaseV2,
    ScheduleRunStatus, StreamPolicyTarget, SCHEDULE_SCHEMA_VERSION,
};
use eggfetch_core::{DialFuture, DialStream, DialTarget, Dialer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Route-authoritative inner dialer: the logical target is recorded and
/// reported, but bytes always travel to the fixed test service. This
/// proves route and impairment stay orthogonal dimensions.
#[derive(Debug, Clone)]
struct ServiceRouteDialer {
    service: std::net::SocketAddr,
    dials: Arc<AtomicU64>,
}

impl Dialer for ServiceRouteDialer {
    fn dial(&self, target: DialTarget) -> DialFuture<'_> {
        let service = self.service;
        let dials = self.dials.clone();
        Box::pin(async move {
            dials.fetch_add(1, Ordering::SeqCst);
            let _ = target;
            let stream = tokio::net::TcpStream::connect(service)
                .await
                .map_err(|error| {
                    eggfetch_core::DialError::with_source(
                        eggfetch_core::DialErrorKind::Connection,
                        "service route connection failed",
                        error,
                    )
                })?;
            Ok(Box::new(stream) as DialStream)
        })
    }
}

fn latency(id: &str, delay_ms: u64) -> FaultSpec {
    FaultSpec {
        id: FaultId::new(id).unwrap(),
        probability: Probability::new(1.0).unwrap(),
        kind: FaultKind::Latency(LatencyConfig {
            delay: Duration::from_millis(delay_ms),
            jitter: Duration::ZERO,
            max_buffer_bytes: NonZeroU64::new(1 << 20).unwrap(),
        }),
    }
}

async fn read_request_head(stream: &mut tokio::net::TcpStream) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if stream.read_exact(&mut byte).await.is_err() {
            return;
        }
        head.push(byte[0]);
        if head.len() >= 4 && &head[head.len() - 4..] == b"\r\n\r\n" {
            return;
        }
    }
}

const HTTP_OK: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\nok";

#[tokio::test]
async fn cross_layer_experiment_correlates_schedule_and_transport() {
    // Local test service speaking keep-alive HTTP/1.1.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let service = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                for _ in 0..16 {
                    read_request_head(&mut stream).await;
                    if stream.write_all(HTTP_OK).await.is_err() {
                        break;
                    }
                }
            });
        }
    });

    // Transport: EggFetch -> route dialer -> M029 chaos adapter.
    let route = ServiceRouteDialer {
        service,
        dials: Arc::new(AtomicU64::new(0)),
    };
    let observer = Arc::new(RecordingObserver::with_default_capacity());
    let dialer = ChaosDialer::wrap(route.clone(), 0x5eed, "web-front")
        .with_integration_id("m031-correlation")
        .unwrap()
        .with_observer(observer.clone());
    let workload_dialer = dialer.clone();

    // Experiment: one resource bound to the adapter's live policies.
    let target = StreamPolicyTarget::new();
    target
        .register(
            "web-front",
            dialer.upstream_policy(),
            dialer.downstream_policy(),
        )
        .unwrap();

    // Two-phase schedule: upstream latency at t=0, then a downstream
    // no-op republish at t=300ms so the workload straddles a live
    // transition on the shared epoch.
    let source = ScenarioScheduleV2 {
        version: SCHEDULE_SCHEMA_VERSION,
        seed: 2026,
        execution_key: 31,
        isolation: IsolationPolicyV2::Strict,
        cleanup: eggchaos_experiment::CleanupPolicyV2::Leave,
        phases: vec![
            SchedulePhaseV2 {
                name: Some("inject-latency".to_owned()),
                duration_ns: 300_000_000,
                actions: vec![eggchaos_experiment::ScenarioAction::SetPlan {
                    proxy: "web-front".to_owned(),
                    direction: Direction::Upstream,
                    faults: vec![latency("inject", 25)],
                }],
            },
            SchedulePhaseV2 {
                name: Some("hold".to_owned()),
                duration_ns: 0,
                actions: vec![eggchaos_experiment::ScenarioAction::SetPlan {
                    proxy: "web-front".to_owned(),
                    direction: Direction::Downstream,
                    faults: vec![],
                }],
            },
        ],
        repeat: None,
    };
    let prepared = PreparedExperiment::prepare(&source, target)
        .await
        .unwrap()
        .with_integration_id("m031-correlation")
        .unwrap();
    let fingerprint = prepared.fingerprint_hex();

    // Arm, then release schedule and workload from one epoch.
    let gate = EpochGate::new();
    let waiter = gate.waiter();
    let token = CancellationToken::new();
    let driver = tokio::spawn(async move {
        let mut events = Vec::new();
        let outcome = prepared.run(waiter, token, &mut events).await;
        (outcome, events)
    });
    let epoch = gate.start();
    assert_eq!(gate.epoch(), Some(epoch));

    // Caller workload on the same clock: two sequential keep-alive
    // requests through EggFetch over the impaired physical stream.
    let client = eggfetch_core::Client::builder()
        .dialer(workload_dialer)
        .build();
    let url = format!("http://127.0.0.1:{}/", service.port());
    let mut bodies = Vec::new();
    for _ in 0..2 {
        let mut response =
            tokio::time::timeout(Duration::from_secs(20), client.get(&url).unwrap().send())
                .await
                .expect("workload request stalled")
                .unwrap();
        let body = tokio::time::timeout(Duration::from_secs(20), response.bytes())
            .await
            .expect("workload body stalled")
            .unwrap();
        bodies.push(body.as_ref().to_vec());
    }
    drop(client);

    let (outcome, events) = tokio::time::timeout(Duration::from_secs(20), driver)
        .await
        .expect("experiment stalled")
        .unwrap();

    // Correlate every layer.
    assert_eq!(outcome.status, ScheduleRunStatus::Completed);
    assert_eq!(outcome.applied, 2);
    assert_eq!(outcome.evidence.schedule_fingerprint, fingerprint);
    assert_eq!(outcome.evidence.seed, 2026);
    assert_eq!(outcome.evidence.execution_key, 31);
    assert_eq!(outcome.evidence.integration_id.as_ref(), "m031-correlation");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].action, "set-plan");
    assert_eq!(events[0].upstream_generation, 2);

    // Transport: one physical dial (keep-alive reuse), one key, live
    // fault evidence matching the schedule publication.
    assert_eq!(route.dials.load(Ordering::SeqCst), 1);
    let records = observer.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].integration_id.as_ref(), "m031-correlation");
    let snapshot = records[0].evidence.snapshot();
    assert_eq!(snapshot.connection_key, records[0].connection_key);
    assert_eq!(snapshot.upstream.generation, 2);
    assert_eq!(snapshot.upstream.active_faults.len(), 1);
    assert_eq!(snapshot.upstream.active_faults[0].id, "inject");

    // Workload-observed outcome under impairment.
    assert_eq!(bodies, vec![b"ok".to_vec(), b"ok".to_vec()]);

    server.abort();
}
