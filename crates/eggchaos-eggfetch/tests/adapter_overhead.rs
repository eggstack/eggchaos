//! M031 adapter/evidence overhead measurements.
//!
//! Compares bare-dialer behavior against the M029 adapter on stable
//! local harnesses: empty-plan throughput over in-memory duplex
//! streams, wrap latency with the evidence observer disabled vs
//! enabled, and compile/prepare/start overhead for small and
//! ceiling-size Scenario V2 schedules.
//!
//! These are regression measurements, not frozen budgets: generous
//! functional bounds catch pathological regressions without baking
//! machine-specific numbers into the gate. Measured values are
//! recorded in the M031 closure evidence.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eggchaos_core::{Direction, FaultPlan, LivePolicy};
use eggchaos_eggfetch::{ChaosDialer, RecordingObserver};
use eggchaos_experiment::{
    CleanupPolicyV2, EpochGate, IsolationPolicyV2, PreparedExperiment, ScenarioScheduleV2,
    SchedulePhaseV2, StreamPolicyTarget, SCHEDULE_SCHEMA_VERSION,
};
use eggfetch_core::{DialFuture, DialStream, DialTarget, Dialer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Default)]
struct DuplexDialer;

impl Dialer for DuplexDialer {
    fn dial(&self, _target: DialTarget) -> DialFuture<'_> {
        Box::pin(async move {
            let (client, server) = tokio::io::duplex(256 * 1024);
            tokio::spawn(async move {
                let (mut read, mut write) = tokio::io::split(server);
                let _ = tokio::io::copy(&mut read, &mut write).await;
            });
            Ok(Box::new(client) as DialStream)
        })
    }
}

const PAYLOAD_BYTES: usize = 8 * 1024 * 1024;

async fn push_bytes(stream: DialStream, bytes: usize) -> Duration {
    // Write and read concurrently: a bounded duplex buffer deadlocks a
    // strict write-then-read sequence once the echo backlog fills.
    let (mut read, mut write) = tokio::io::split(stream);
    let chunk = vec![0x11u8; 64 * 1024];
    let start = Instant::now();
    let writer = tokio::spawn(async move {
        let mut remaining = bytes;
        while remaining > 0 {
            let take = remaining.min(chunk.len());
            write.write_all(&chunk[..take]).await.unwrap();
            remaining -= take;
        }
        write.flush().await.unwrap();
        write.shutdown().await.unwrap();
    });
    let mut drain = vec![0u8; 64 * 1024];
    let mut received = 0;
    while received < bytes {
        let n = read.read(&mut drain).await.unwrap();
        assert!(n > 0, "echo closed early");
        received += n;
    }
    writer.await.unwrap();
    start.elapsed()
}

#[tokio::test]
async fn empty_plan_adapter_throughput_stays_close_to_bare() {
    let target = DialTarget::new("duplex", 1);
    // Bare duplex echo baseline.
    let bare = DuplexDialer;
    let bare_stream = bare.dial(target.clone()).await.unwrap();
    let bare_elapsed = push_bytes(bare_stream, PAYLOAD_BYTES).await;

    // M029 adapter, empty plan, observer disabled.
    let plain = ChaosDialer::wrap(DuplexDialer, 1, "perf");
    let plain_stream = plain.dial(target.clone()).await.unwrap();
    let plain_elapsed = push_bytes(plain_stream, PAYLOAD_BYTES).await;

    // M029 adapter, empty plan, bounded observer enabled.
    let observer = Arc::new(RecordingObserver::new(8));
    let observed = ChaosDialer::wrap(DuplexDialer, 1, "perf").with_observer(observer);
    let observed_stream = observed.dial(target).await.unwrap();
    let observed_elapsed = push_bytes(observed_stream, PAYLOAD_BYTES).await;

    let bare_secs = bare_elapsed.as_secs_f64().max(1e-9);
    let plain_ratio = bare_secs / plain_elapsed.as_secs_f64().max(1e-9);
    let observed_ratio = bare_secs / observed_elapsed.as_secs_f64().max(1e-9);
    eprintln!(
        "overhead: bare={bare_elapsed:?} plain={plain_elapsed:?} observed={observed_elapsed:?}"
    );
    eprintln!("overhead: plain_ratio={plain_ratio:.3} observed_ratio={observed_ratio:.3}");
    // Generous functional bounds: empty-plan data movement must not
    // collapse against the same-topology baseline.
    assert!(
        plain_ratio > 0.25,
        "empty-plan adapter throughput collapsed"
    );
    assert!(
        observed_ratio > 0.25,
        "observer-enabled throughput collapsed"
    );
}

#[tokio::test]
async fn wrap_latency_with_observer_stays_bounded() {
    let target = DialTarget::new("duplex", 1);
    let plain = ChaosDialer::wrap(DuplexDialer, 1, "perf");
    let start = Instant::now();
    for _ in 0..200 {
        let _stream = plain.dial(target.clone()).await.unwrap();
    }
    let plain_elapsed = start.elapsed();

    let observer = Arc::new(RecordingObserver::new(256));
    let observed = ChaosDialer::wrap(DuplexDialer, 1, "perf").with_observer(observer);
    let start = Instant::now();
    for _ in 0..200 {
        let _stream = observed.dial(target.clone()).await.unwrap();
    }
    let observed_elapsed = start.elapsed();

    eprintln!("overhead: wrap_plain={plain_elapsed:?} wrap_observed={observed_elapsed:?}");
    assert!(
        observed_elapsed < plain_elapsed.mul_f64(4.0) + Duration::from_millis(500),
        "evidence observer dominates dial latency"
    );
}

fn schedule_with_events(count: usize) -> ScenarioScheduleV2 {
    let actions = (0..count)
        .map(|_| eggchaos_experiment::ScenarioAction::SetPlan {
            proxy: "api".to_owned(),
            direction: Direction::Upstream,
            faults: vec![],
        })
        .collect();
    ScenarioScheduleV2 {
        version: SCHEDULE_SCHEMA_VERSION,
        seed: 1,
        execution_key: 2,
        isolation: IsolationPolicyV2::Strict,
        cleanup: CleanupPolicyV2::Leave,
        phases: vec![SchedulePhaseV2 {
            name: None,
            duration_ns: 0,
            actions,
        }],
        repeat: None,
    }
}

#[tokio::test]
async fn schedule_compile_prepare_start_overhead_is_bounded() {
    let target = StreamPolicyTarget::new();
    target
        .register(
            "api",
            LivePolicy::new(FaultPlan::empty(), 1),
            LivePolicy::new(FaultPlan::empty(), 1),
        )
        .unwrap();

    // Small schedule.
    let start = Instant::now();
    let prepared = PreparedExperiment::prepare(&schedule_with_events(3), target.clone())
        .await
        .unwrap();
    let prepare_small = start.elapsed();
    let gate = EpochGate::started();
    let mut events = Vec::new();
    let outcome = prepared
        .run_from_epoch(gate.epoch().unwrap(), CancellationToken::new(), &mut events)
        .await;
    assert_eq!(events.len(), 3);
    assert_eq!(outcome.applied, 3);
    eprintln!("overhead: prepare_small={prepare_small:?}");

    // Ceiling-size schedule: 1024 events in 16 phases of 64 actions.
    let mut phases = Vec::new();
    for _ in 0..16 {
        phases.push(SchedulePhaseV2 {
            name: None,
            duration_ns: 0,
            actions: (0..64)
                .map(|_| eggchaos_experiment::ScenarioAction::SetPlan {
                    proxy: "api".to_owned(),
                    direction: Direction::Upstream,
                    faults: vec![],
                })
                .collect(),
        });
    }
    let big = ScenarioScheduleV2 {
        version: SCHEDULE_SCHEMA_VERSION,
        seed: 1,
        execution_key: 2,
        isolation: IsolationPolicyV2::Strict,
        cleanup: CleanupPolicyV2::Leave,
        phases,
        repeat: None,
    };
    let start = Instant::now();
    let prepared = PreparedExperiment::prepare(&big, target).await.unwrap();
    let prepare_big = start.elapsed();
    assert_eq!(prepared.touched_resource_count(), 1);
    let gate = EpochGate::started();
    let mut events = Vec::new();
    let outcome = prepared
        .run_from_epoch(gate.epoch().unwrap(), CancellationToken::new(), &mut events)
        .await;
    assert_eq!(outcome.applied, 1024);
    eprintln!("overhead: prepare_big_1024={prepare_big:?}");
    assert!(
        prepare_small < Duration::from_secs(5),
        "small prepare too slow"
    );
    assert!(
        prepare_big < Duration::from_secs(10),
        "ceiling prepare too slow"
    );
    let _ = outcome;
}
