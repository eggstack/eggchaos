//! Target conformance: `ControlState` adapter vs in-process stream target.
//!
//! The shared `eggchaos_experiment` driver executes both sides. These
//! tests prove the observable schedule semantics are identical for the
//! supported stream subset: same event/generation outcomes, same
//! strict/live behavior, same cleanup outcomes, and explicit typed
//! failure for datagram actions on the stream-only target.

use std::num::NonZeroU64;
use std::time::Duration;

use eggchaos_core::{
    Direction, FaultId, FaultKind, FaultPlan, FaultSpec, LatencyConfig, Probability,
};
use eggchaos_experiment::{
    EpochGate, PolicyTarget, PreparedExperiment, ScheduleEventResult, ScheduleRunStatus,
    StreamPolicyTarget, TargetError,
};
use tokio_util::sync::CancellationToken;

use crate::runtime::{ControlState, ProxySpec, RuntimeParams};
use crate::scenario::ScenarioAction;
use crate::scenario_v2::{
    CleanupPolicyV2, IsolationPolicyV2, ScenarioScheduleV2, SchedulePhaseV2,
    SCHEDULE_SCHEMA_VERSION,
};
use crate::ScheduleRunStatus as ServerRunStatus;

fn latency(id: &str) -> FaultSpec {
    FaultSpec {
        id: FaultId::new(id).unwrap(),
        probability: Probability::new(1.0).unwrap(),
        kind: FaultKind::Latency(LatencyConfig {
            delay: Duration::from_millis(5),
            jitter: Duration::ZERO,
            max_buffer_bytes: NonZeroU64::new(1024).unwrap(),
        }),
    }
}

fn set_plan(proxy: &str, direction: Direction, faults: Vec<FaultSpec>) -> ScenarioAction {
    ScenarioAction::SetPlan {
        proxy: proxy.to_owned(),
        direction,
        faults,
    }
}

fn schedule(
    isolation: IsolationPolicyV2,
    cleanup: CleanupPolicyV2,
    phases: Vec<(u64, Vec<ScenarioAction>)>,
) -> ScenarioScheduleV2 {
    ScenarioScheduleV2 {
        version: SCHEDULE_SCHEMA_VERSION,
        seed: 9,
        execution_key: 10,
        isolation,
        cleanup,
        phases: phases
            .into_iter()
            .map(|(duration_ns, actions)| SchedulePhaseV2 {
                name: None,
                duration_ns,
                actions,
            })
            .collect(),
        repeat: None,
    }
}

fn stream_schedule() -> ScenarioScheduleV2 {
    schedule(
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::RestoreInitial,
        vec![
            (
                0,
                vec![set_plan("api", Direction::Upstream, vec![latency("slow")])],
            ),
            (
                0,
                vec![set_plan(
                    "api",
                    Direction::Downstream,
                    vec![latency("down")],
                )],
            ),
            (
                0,
                vec![ScenarioAction::RemoveFault {
                    proxy: "api".to_owned(),
                    direction: Direction::Upstream,
                    id: "slow".to_owned(),
                }],
            ),
        ],
    )
}

fn test_control() -> ControlState {
    ControlState::with_params(RuntimeParams {
        seed: 7,
        term_grace: Duration::from_millis(100),
        ..RuntimeParams::default()
    })
}

async fn stream_proxy(control: &ControlState, name: &str) {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    control
        .create_proxy(ProxySpec::new(name, addr, addr))
        .await
        .unwrap();
}

async fn wait_schedule_v2(
    control: &ControlState,
    run_id: u64,
) -> crate::scenario_v2::ScenarioScheduleRunRecord {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let record = control.get_schedule_v2(run_id).await.unwrap();
            if matches!(
                record.status,
                ServerRunStatus::Completed | ServerRunStatus::Cancelled | ServerRunStatus::Failed
            ) {
                break record;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

fn stream_target() -> StreamPolicyTarget {
    let target = StreamPolicyTarget::new();
    target
        .register(
            "api",
            eggchaos_core::LivePolicy::new(FaultPlan::empty(), 100),
            eggchaos_core::LivePolicy::new(FaultPlan::empty(), 100),
        )
        .unwrap();
    target
}

/// Compare server run-record events with embedded collected events,
/// ignoring only the target-wide global generation (server global vs
/// in-process counter are different authorities by design).
fn assert_events_conform(server: &[ScheduleEventResult], embedded: &[ScheduleEventResult]) {
    assert_eq!(server.len(), embedded.len(), "event counts must match");
    for (left, right) in server.iter().zip(embedded.iter()) {
        assert_eq!(left.compiled_index, right.compiled_index);
        assert_eq!(left.scheduled_offset_ns, right.scheduled_offset_ns);
        assert_eq!(left.action, right.action);
        assert_eq!(left.resource, right.resource);
        assert_eq!(left.upstream_generation, right.upstream_generation);
        assert_eq!(left.downstream_generation, right.downstream_generation);
    }
}

#[tokio::test]
async fn control_and_stream_target_conform_on_stream_schedule() {
    let source = stream_schedule();

    // Server path: full ControlState run with run-record evidence.
    let control = test_control();
    stream_proxy(&control, "api").await;
    let record = control.start_schedule_v2(source.clone()).await.unwrap();
    let record = wait_schedule_v2(&control, record.run_id).await;
    assert_eq!(record.status, ServerRunStatus::Completed);
    assert_eq!(record.events.len(), 3);

    // Embedded path: same source through the in-process stream target.
    let target = stream_target();
    let prepared = PreparedExperiment::prepare(&source, target).await.unwrap();
    let gate = EpochGate::started();
    let mut events = Vec::new();
    let outcome = prepared
        .run_from_epoch(gate.epoch().unwrap(), CancellationToken::new(), &mut events)
        .await;
    assert_eq!(outcome.status, ScheduleRunStatus::Completed);

    assert_events_conform(&record.events, &events);
    // Per-event generation outcomes match exactly.
    assert_eq!(events[0].upstream_generation, 2);
    assert_eq!(events[1].downstream_generation, 2);
    assert_eq!(events[2].upstream_generation, 3);
    // Restore-initial cleanup matches on both sides.
    assert_eq!(record.cleanup.as_ref().unwrap(), &outcome.cleanup);
}

#[tokio::test]
async fn live_mode_conforms_with_prior_external_publish() {
    let source = schedule(
        IsolationPolicyV2::Live,
        CleanupPolicyV2::Leave,
        vec![(
            0,
            vec![set_plan("api", Direction::Upstream, vec![latency("slow")])],
        )],
    );

    let control = test_control();
    stream_proxy(&control, "api").await;
    control
        .publish_direction_expected("api", Direction::Upstream, FaultPlan::empty(), 7, 1)
        .await
        .unwrap();
    let record = control.start_schedule_v2(source.clone()).await.unwrap();
    let record = wait_schedule_v2(&control, record.run_id).await;
    assert_eq!(record.status, ServerRunStatus::Completed);

    let target = stream_target();
    let resource = eggchaos_experiment::ScheduleResource {
        proxy: "api".to_owned(),
        direction: Direction::Upstream,
        transport: eggchaos_experiment::ScheduleTransport::Stream,
    };
    target
        .publish(
            &resource,
            1,
            eggchaos_experiment::TargetPlan::Stream(FaultPlan::empty()),
            7,
        )
        .await
        .unwrap();
    let prepared = PreparedExperiment::prepare(&source, target).await.unwrap();
    let gate = EpochGate::started();
    let mut events = Vec::new();
    let outcome = prepared
        .run_from_epoch(gate.epoch().unwrap(), CancellationToken::new(), &mut events)
        .await;
    assert_eq!(outcome.status, ScheduleRunStatus::Completed);

    // Live mode based both runs on the externally published generation.
    assert_events_conform(&record.events, &events);
    assert_eq!(events[0].upstream_generation, 3);
}

#[tokio::test]
async fn stream_target_rejects_datagram_while_server_applies_it() {
    use eggchaos_core::{DatagramFaultKind, DatagramFaultSpec, DatagramPlan};
    let loss = DatagramFaultSpec {
        id: FaultId::new("loss").unwrap(),
        probability: Probability::new(1.0).unwrap(),
        kind: DatagramFaultKind::Loss,
    };
    let source = schedule(
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
        vec![(
            0,
            vec![ScenarioAction::SetDatagramPlan {
                proxy: "dgram".to_owned(),
                direction: Direction::Upstream,
                faults: vec![loss],
            }],
        )],
    );

    // Embedded stream-only target fails during preparation, publishing
    // nothing.
    let target = stream_target();
    let error = PreparedExperiment::prepare(&source, target)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        eggchaos_experiment::ExperimentError::Target(TargetError::UnsupportedCapability(_))
    ));

    // The server adapter applies the same schedule to a datagram proxy.
    let control = test_control();
    let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let listen: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let spec = crate::DatagramProxySpec::new(
        "dgram",
        listen,
        socket.local_addr().unwrap(),
        eggchaos_core::DatagramQueueLimits {
            max_queued_datagrams: NonZeroU64::new(8).unwrap(),
            max_queued_bytes: NonZeroU64::new(4096).unwrap(),
            max_datagram_bytes: NonZeroU64::new(2048).unwrap(),
        },
    )
    .unwrap();
    control.create_datagram_proxy(spec).await.unwrap();
    let record = control.start_schedule_v2(source).await.unwrap();
    let record = wait_schedule_v2(&control, record.run_id).await;
    assert_eq!(record.status, ServerRunStatus::Completed);
    assert_eq!(record.events[0].action, "set-datagram-plan");
    let _ = DatagramPlan::empty();
}
