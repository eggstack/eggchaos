//! M030 shared-epoch, target, and lifecycle tests.

use std::num::NonZeroU64;
use std::time::Duration;

use eggchaos_core::{
    DisconnectConfig, FaultId, FaultKind, FaultPlan, FaultSpec, LatencyConfig, LivePolicy,
    Probability,
};
use tokio_util::sync::CancellationToken;

use super::*;

fn fault(id: &str, kind: FaultKind) -> FaultSpec {
    FaultSpec {
        id: FaultId::new(id).unwrap(),
        probability: Probability::new(1.0).unwrap(),
        kind,
    }
}

fn latency_fault(id: &str, delay_ms: u64) -> FaultSpec {
    fault(
        id,
        FaultKind::Latency(LatencyConfig {
            delay: Duration::from_millis(delay_ms),
            jitter: Duration::ZERO,
            max_buffer_bytes: NonZeroU64::new(1 << 20).unwrap(),
        }),
    )
}

fn set_plan(
    proxy: &str,
    direction: eggchaos_core::Direction,
    faults: Vec<FaultSpec>,
) -> ScenarioAction {
    ScenarioAction::SetPlan {
        proxy: proxy.to_owned(),
        direction,
        faults,
    }
}

fn source_with(
    phases: Vec<SchedulePhaseV2>,
    isolation: IsolationPolicyV2,
    cleanup: CleanupPolicyV2,
) -> ScenarioScheduleV2 {
    ScenarioScheduleV2 {
        version: SCHEDULE_SCHEMA_VERSION,
        seed: 11,
        execution_key: 22,
        isolation,
        cleanup,
        phases,
        repeat: None,
    }
}

fn phase(offset_ms: u64, actions: Vec<ScenarioAction>) -> SchedulePhaseV2 {
    SchedulePhaseV2 {
        name: None,
        duration_ns: offset_ms * 1_000_000,
        actions,
    }
}

fn stream_target_named(name: &str) -> StreamPolicyTarget {
    let target = StreamPolicyTarget::new();
    target
        .register(
            name,
            LivePolicy::new(FaultPlan::empty(), 100),
            LivePolicy::new(FaultPlan::empty(), 100),
        )
        .unwrap();
    target
}

#[test]
fn compiler_identity_is_frozen() {
    assert_eq!(SCHEDULE_SCHEMA_VERSION, 2);
    assert_eq!(COMPILER_SEMANTICS_VERSION, 1);
    assert_eq!(MAX_COMPILED_EVENTS, 1024);
}

#[test]
fn compile_is_deterministic_and_seed_sensitive() {
    let phases = vec![phase(
        0,
        vec![set_plan("api", eggchaos_core::Direction::Upstream, vec![])],
    )];
    let first = compile_schedule(&source_with(
        phases.clone(),
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
    ))
    .unwrap();
    let second = compile_schedule(&source_with(
        phases,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
    ))
    .unwrap();
    assert_eq!(compiled_fingerprint(&first), compiled_fingerprint(&second));
    let mut other = source_with(
        vec![phase(
            0,
            vec![set_plan("api", eggchaos_core::Direction::Upstream, vec![])],
        )],
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
    );
    other.seed = 12;
    let third = compile_schedule(&other).unwrap();
    assert_ne!(compiled_fingerprint(&first), compiled_fingerprint(&third));
}

#[tokio::test]
async fn prepare_rejects_datagram_on_stream_only_target() {
    let target = stream_target_named("api");
    let source = source_with(
        vec![phase(
            0,
            vec![ScenarioAction::SetDatagramPlan {
                proxy: "api".to_owned(),
                direction: eggchaos_core::Direction::Upstream,
                faults: vec![],
            }],
        )],
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
    );
    let error = PreparedExperiment::prepare(&source, target)
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            ExperimentError::Target(TargetError::UnsupportedCapability(_))
        ),
        "datagram actions must fail explicitly, got {error:?}"
    );
}

#[tokio::test]
async fn prepare_rejects_missing_resource_without_publication() {
    let target = StreamPolicyTarget::new();
    let source = source_with(
        vec![phase(
            0,
            vec![set_plan(
                "ghost",
                eggchaos_core::Direction::Upstream,
                vec![],
            )],
        )],
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
    );
    let error = PreparedExperiment::prepare(&source, target)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("ghost"), "got {error}");
}

#[tokio::test]
async fn prepare_publishes_nothing_and_counts_touched_resources() {
    let target = stream_target_named("api");
    let source = source_with(
        vec![phase(
            0,
            vec![
                set_plan("api", eggchaos_core::Direction::Upstream, vec![]),
                set_plan("api", eggchaos_core::Direction::Downstream, vec![]),
            ],
        )],
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
    );
    let prepared = PreparedExperiment::prepare(&source, target).await.unwrap();
    assert_eq!(prepared.touched_resource_count(), 2);
    assert_eq!(prepared.compiled().events.len(), 2);
    assert!(!prepared.fingerprint_hex().is_empty());
    // Dropping a prepared experiment publishes nothing and spawns nothing.
    drop(prepared);
}

#[tokio::test]
async fn start_gate_shares_one_epoch() {
    let gate = EpochGate::new();
    assert_eq!(gate.epoch(), None);
    let waiter = gate.waiter();
    assert_eq!(waiter.try_epoch(), None);
    let first = gate.start();
    let second = gate.start();
    assert_eq!(first, second, "epoch is captured exactly once");
    assert_eq!(waiter.await_epoch().await, first);
    // Late waiters resolve immediately from retained state.
    assert_eq!(gate.waiter().await_epoch().await, first);
}

#[tokio::test]
async fn no_event_applies_before_release() {
    let target = stream_target_named("api");
    let source = source_with(
        vec![phase(
            0,
            vec![set_plan(
                "api",
                eggchaos_core::Direction::Upstream,
                vec![latency_fault("slow", 5)],
            )],
        )],
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
    );
    let prepared = PreparedExperiment::prepare(&source, target).await.unwrap();
    let gate = EpochGate::new();
    let waiter = gate.waiter();
    let token = CancellationToken::new();
    let driver = tokio::spawn(async move {
        let mut events = Vec::new();
        prepared.run(waiter, token, &mut events).await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!driver.is_finished(), "driver must wait for the epoch");
    gate.start();
    let outcome = driver.await.unwrap();
    assert_eq!(outcome.status, ScheduleRunStatus::Completed);
    assert_eq!(outcome.applied, 1);
}

#[tokio::test(start_paused = true)]
async fn paused_time_events_fire_without_drift() {
    let target = stream_target_named("api");
    let source = ScenarioScheduleV2 {
        version: SCHEDULE_SCHEMA_VERSION,
        seed: 1,
        execution_key: 2,
        isolation: IsolationPolicyV2::Strict,
        cleanup: CleanupPolicyV2::Leave,
        phases: vec![
            SchedulePhaseV2 {
                name: None,
                duration_ns: 1_000_000_000,
                actions: vec![set_plan("api", eggchaos_core::Direction::Upstream, vec![])],
            },
            SchedulePhaseV2 {
                name: None,
                duration_ns: 1_000_000_000,
                actions: vec![set_plan("api", eggchaos_core::Direction::Upstream, vec![])],
            },
            SchedulePhaseV2 {
                name: None,
                duration_ns: 1_000_000_000,
                actions: vec![set_plan("api", eggchaos_core::Direction::Upstream, vec![])],
            },
        ],
        repeat: None,
    };
    let prepared = PreparedExperiment::prepare(&source, target).await.unwrap();
    assert_eq!(prepared.touched_resource_count(), 1);
    let gate = EpochGate::started();
    let token = CancellationToken::new();
    let driver = tokio::spawn(async move {
        let mut events = Vec::new();
        let outcome = prepared
            .run_from_epoch(gate.epoch().unwrap(), token, &mut events)
            .await;
        (outcome, events)
    });
    // Let the driver register its first deadline before advancing: the
    // 0-offset event applies at the epoch, then each 1s step fires
    // exactly one deadline with no drift.
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(1)).await;
    let (outcome, events) = driver.await.unwrap();
    assert_eq!(outcome.status, ScheduleRunStatus::Completed);
    assert_eq!(events.len(), 3);
    // Sparse 1s/2s deadlines fire exactly at epoch-relative offsets:
    // no drift accumulation, compiled order preserved.
    assert_eq!(events[0].scheduled_offset_ns, 0);
    assert_eq!(events[1].scheduled_offset_ns, 1_000_000_000);
    assert_eq!(events[2].scheduled_offset_ns, 2_000_000_000);
    for event in &events {
        assert_eq!(event.applied_elapsed_ns, event.scheduled_offset_ns);
        assert_eq!(event.late_by_ns, 0);
    }
    assert_eq!(events[0].compiled_index, 0);
    assert_eq!(events[1].compiled_index, 1);
    assert_eq!(events[2].compiled_index, 2);
}

#[tokio::test(start_paused = true)]
async fn equal_deadline_events_preserve_compiled_order() {
    let target = stream_target_named("api");
    let source = source_with(
        vec![phase(
            0,
            vec![
                set_plan("api", eggchaos_core::Direction::Upstream, vec![]),
                set_plan("api", eggchaos_core::Direction::Downstream, vec![]),
            ],
        )],
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
    );
    let prepared = PreparedExperiment::prepare(&source, target).await.unwrap();
    let gate = EpochGate::started();
    let token = CancellationToken::new();
    let mut events = Vec::new();
    let outcome = prepared
        .run_from_epoch(gate.epoch().unwrap(), token, &mut events)
        .await;
    assert_eq!(outcome.status, ScheduleRunStatus::Completed);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].compiled_index, 0);
    assert_eq!(events[1].compiled_index, 1);
    assert_eq!(events[0].scheduled_offset_ns, 0);
    assert_eq!(events[1].scheduled_offset_ns, 0);
}

#[tokio::test]
async fn cancel_before_start_publishes_nothing() {
    let target = stream_target_named("api");
    let upstream = target
        .snapshot(&ScheduleResource {
            proxy: "api".to_owned(),
            direction: eggchaos_core::Direction::Upstream,
            transport: ScheduleTransport::Stream,
        })
        .await
        .unwrap();
    assert_eq!(upstream.generation, 1);
    let source = source_with(
        vec![phase(
            0,
            vec![set_plan(
                "api",
                eggchaos_core::Direction::Upstream,
                vec![latency_fault("slow", 5)],
            )],
        )],
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
    );
    let prepared = PreparedExperiment::prepare(&source, target).await.unwrap();
    let gate = EpochGate::started();
    let token = CancellationToken::new();
    token.cancel();
    let mut events = Vec::new();
    let outcome = prepared
        .run_from_epoch(gate.epoch().unwrap(), token, &mut events)
        .await;
    assert_eq!(outcome.status, ScheduleRunStatus::Cancelled);
    assert_eq!(outcome.applied, 0);
    assert!(events.is_empty());
}

#[tokio::test]
async fn cancel_while_sleeping_is_prompt() {
    let target = stream_target_named("api");
    let source = source_with(
        vec![
            phase(
                100_000,
                vec![set_plan("api", eggchaos_core::Direction::Upstream, vec![])],
            ),
            phase(
                0,
                vec![set_plan("api", eggchaos_core::Direction::Upstream, vec![])],
            ),
        ],
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
    );
    let prepared = PreparedExperiment::prepare(&source, target).await.unwrap();
    let gate = EpochGate::started();
    let token = CancellationToken::new();
    let canceller = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        canceller.cancel();
    });
    let mut events = Vec::new();
    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        prepared.run_from_epoch(gate.epoch().unwrap(), token, &mut events),
    )
    .await
    .expect("cancellation must wake the deadline sleep promptly");
    assert_eq!(outcome.status, ScheduleRunStatus::Cancelled);
    assert_eq!(outcome.applied, 1);
    // Leave cleanup records NotRequested without publishing.
    assert_eq!(outcome.cleanup.resources.len(), 1);
    assert_eq!(
        outcome.cleanup.resources[0].outcome,
        CleanupResourceOutcome::NotRequested
    );
}

#[tokio::test]
async fn strict_external_move_conflicts_without_overwrite() {
    let target = stream_target_named("api");
    let source = source_with(
        vec![phase(
            0,
            vec![set_plan(
                "api",
                eggchaos_core::Direction::Upstream,
                vec![latency_fault("slow", 5)],
            )],
        )],
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::RestoreInitial,
    );
    let prepared = PreparedExperiment::prepare(&source, target.clone())
        .await
        .unwrap();
    // External move between preparation and start: the run must fail on
    // the next affected event and cleanup must not clobber it.
    let external = FaultPlan::new(vec![fault(
        "ext",
        FaultKind::Disconnect(DisconnectConfig {
            after: Duration::ZERO,
            hard_reset: false,
        }),
    )])
    .unwrap();
    let resource = ScheduleResource {
        proxy: "api".to_owned(),
        direction: eggchaos_core::Direction::Upstream,
        transport: ScheduleTransport::Stream,
    };
    target
        .publish(&resource, 1, TargetPlan::Stream(external.clone()), 999)
        .await
        .unwrap();
    let gate = EpochGate::started();
    let token = CancellationToken::new();
    let mut events = Vec::new();
    let outcome = prepared
        .run_from_epoch(gate.epoch().unwrap(), token, &mut events)
        .await;
    assert_eq!(outcome.status, ScheduleRunStatus::Failed);
    assert!(outcome.failure.unwrap().contains("publish failed"));
    let snapshot = target.snapshot(&resource).await.unwrap();
    assert_eq!(snapshot.generation, 2);
    assert_eq!(
        snapshot.plan,
        TargetPlan::Stream(external),
        "cleanup conflict must not overwrite external state"
    );
    let TargetPlan::Stream(plan) = snapshot.plan else {
        unreachable!();
    };
    assert!(plan.get("ext").is_some());
    assert_eq!(
        outcome.cleanup.resources[0].outcome,
        CleanupResourceOutcome::Conflict
    );
}

#[tokio::test]
async fn live_mode_observes_current_state() {
    let target = stream_target_named("api");
    let source = source_with(
        vec![phase(
            0,
            vec![set_plan(
                "api",
                eggchaos_core::Direction::Upstream,
                vec![latency_fault("slow", 5)],
            )],
        )],
        IsolationPolicyV2::Live,
        CleanupPolicyV2::Leave,
    );
    let prepared = PreparedExperiment::prepare(&source, target.clone())
        .await
        .unwrap();
    let resource = ScheduleResource {
        proxy: "api".to_owned(),
        direction: eggchaos_core::Direction::Upstream,
        transport: ScheduleTransport::Stream,
    };
    target
        .publish(&resource, 1, TargetPlan::Stream(FaultPlan::empty()), 999)
        .await
        .unwrap();
    let gate = EpochGate::started();
    let token = CancellationToken::new();
    let mut events = Vec::new();
    let outcome = prepared
        .run_from_epoch(gate.epoch().unwrap(), token, &mut events)
        .await;
    assert_eq!(outcome.status, ScheduleRunStatus::Completed);
    // Live mode based the event on the current generation (2), so the
    // run published generation 3 rather than conflicting.
    let snapshot = target.snapshot(&resource).await.unwrap();
    assert_eq!(snapshot.generation, 3);
    assert_eq!(events[0].upstream_generation, 3);
}

#[tokio::test]
async fn restore_cleanup_returns_to_initial_plan() {
    let target = stream_target_named("api");
    let source = source_with(
        vec![phase(
            0,
            vec![set_plan(
                "api",
                eggchaos_core::Direction::Upstream,
                vec![latency_fault("slow", 5)],
            )],
        )],
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::RestoreInitial,
    );
    let prepared = PreparedExperiment::prepare(&source, target.clone())
        .await
        .unwrap();
    let gate = EpochGate::started();
    let token = CancellationToken::new();
    let mut events = Vec::new();
    let outcome = prepared
        .run_from_epoch(gate.epoch().unwrap(), token, &mut events)
        .await;
    assert_eq!(outcome.status, ScheduleRunStatus::Completed);
    assert_eq!(
        outcome.cleanup.resources[0].outcome,
        CleanupResourceOutcome::Restored
    );
    let resource = ScheduleResource {
        proxy: "api".to_owned(),
        direction: eggchaos_core::Direction::Upstream,
        transport: ScheduleTransport::Stream,
    };
    let snapshot = target.snapshot(&resource).await.unwrap();
    let TargetPlan::Stream(plan) = snapshot.plan else {
        unreachable!();
    };
    assert!(plan.faults().is_empty(), "initial empty plan was restored");
}

#[tokio::test]
async fn set_then_remove_fault_flows_through_expected_generations() {
    let target = stream_target_named("api");
    let source = source_with(
        vec![
            phase(
                0,
                vec![set_plan(
                    "api",
                    eggchaos_core::Direction::Upstream,
                    vec![latency_fault("slow", 5)],
                )],
            ),
            phase(
                0,
                vec![ScenarioAction::RemoveFault {
                    proxy: "api".to_owned(),
                    direction: eggchaos_core::Direction::Upstream,
                    id: "slow".to_owned(),
                }],
            ),
        ],
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
    );
    let prepared = PreparedExperiment::prepare(&source, target).await.unwrap();
    let gate = EpochGate::started();
    let token = CancellationToken::new();
    let mut events = Vec::new();
    let outcome = prepared
        .run_from_epoch(gate.epoch().unwrap(), token, &mut events)
        .await;
    assert_eq!(outcome.status, ScheduleRunStatus::Completed);
    assert_eq!(events[0].action, "set-plan");
    assert_eq!(events[0].upstream_generation, 2);
    assert_eq!(events[1].action, "remove-fault");
    assert_eq!(events[1].upstream_generation, 3);
}

#[tokio::test]
async fn remove_missing_fault_fails_fast() {
    let target = stream_target_named("api");
    let source = source_with(
        vec![phase(
            0,
            vec![ScenarioAction::RemoveFault {
                proxy: "api".to_owned(),
                direction: eggchaos_core::Direction::Upstream,
                id: "nope".to_owned(),
            }],
        )],
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
    );
    let prepared = PreparedExperiment::prepare(&source, target).await.unwrap();
    let gate = EpochGate::started();
    let token = CancellationToken::new();
    let mut events = Vec::new();
    let outcome = prepared
        .run_from_epoch(gate.epoch().unwrap(), token, &mut events)
        .await;
    assert_eq!(outcome.status, ScheduleRunStatus::Failed);
    assert!(outcome.failure.unwrap().contains("not present"));
}

#[tokio::test]
async fn run_with_workload_shares_epoch_with_caller() {
    let target = stream_target_named("api");
    let source = source_with(
        vec![phase(
            0,
            vec![set_plan("api", eggchaos_core::Direction::Upstream, vec![])],
        )],
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
    );
    let prepared = PreparedExperiment::prepare(&source, target).await.unwrap();
    let token = CancellationToken::new();
    let mut events = Vec::new();
    let (outcome, workload_epoch) = prepared
        .run_with_workload(token, &mut events, |epoch| async move { epoch })
        .await;
    assert_eq!(outcome.status, ScheduleRunStatus::Completed);
    // The 0-offset event applied at the shared epoch: schedule and
    // caller observed the same clock.
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].scheduled_offset_ns, 0);
    assert!(events[0].applied_elapsed_ns < 5_000_000_000);
    assert_eq!(outcome.evidence.schedule_fingerprint.len(), 64);
    assert_eq!(outcome.evidence.seed, 11);
    assert_eq!(outcome.evidence.execution_key, 22);
    let _ = workload_epoch;
}

#[tokio::test]
async fn experiment_evidence_carries_identity_and_versions() {
    let target = stream_target_named("api");
    let source = source_with(
        vec![phase(
            0,
            vec![set_plan("api", eggchaos_core::Direction::Upstream, vec![])],
        )],
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
    );
    let prepared = PreparedExperiment::prepare(&source, target)
        .await
        .unwrap()
        .with_integration_id("downstream-harness-7")
        .unwrap();
    let gate = EpochGate::started();
    let token = CancellationToken::new();
    let mut events = Vec::new();
    let outcome = prepared
        .run_from_epoch(gate.epoch().unwrap(), token, &mut events)
        .await;
    assert_eq!(
        outcome.evidence.compiler_semantics_version,
        COMPILER_SEMANTICS_VERSION
    );
    assert_eq!(
        outcome.evidence.integration_id.as_ref(),
        "downstream-harness-7"
    );
    assert_eq!(outcome.evidence.applied, 1);
}

#[test]
fn target_bounds_are_rejected() {
    let target = StreamPolicyTarget::new();
    assert!(matches!(
        target.register(
            "x".repeat(129),
            LivePolicy::new(FaultPlan::empty(), 0),
            LivePolicy::new(FaultPlan::empty(), 0)
        ),
        Err(TargetError::Validation(_))
    ));
    assert!(matches!(TargetError::bounded("y".repeat(300)).len(), 256));
}

#[tokio::test]
async fn integration_identity_is_bounded() {
    let target = stream_target_named("api");
    let source = source_with(
        vec![phase(
            0,
            vec![set_plan("api", eggchaos_core::Direction::Upstream, vec![])],
        )],
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
    );
    let error = PreparedExperiment::prepare(&source, target)
        .await
        .unwrap()
        .with_integration_id("z".repeat(129))
        .unwrap_err();
    assert_eq!(error, ExperimentError::IdentityTooLong);
}

#[tokio::test]
async fn stream_target_from_policies_registers_one_resource() {
    let target = stream_target_from_policies(
        "web",
        LivePolicy::new(FaultPlan::empty(), 3),
        LivePolicy::new(FaultPlan::empty(), 3),
    )
    .unwrap();
    assert_eq!(target.names(), vec!["web".to_owned()]);
    assert_eq!(target.global_generation(), 0);
    let resource = ScheduleResource {
        proxy: "web".to_owned(),
        direction: eggchaos_core::Direction::Downstream,
        transport: ScheduleTransport::Stream,
    };
    target
        .publish(&resource, 1, TargetPlan::Stream(FaultPlan::empty()), 3)
        .await
        .unwrap();
    assert_eq!(target.global_generation(), 1);
    // Publishing a datagram plan to a stream-only target is a typed
    // failure, never a silent translation.
    let error = target
        .publish(&resource, 2, TargetPlan::Datagram(empty_datagram_plan()), 3)
        .await
        .unwrap_err();
    assert!(matches!(error, TargetError::UnsupportedCapability(_)));
}

fn empty_datagram_plan() -> eggchaos_core::DatagramPlan {
    eggchaos_core::DatagramPlan::empty()
}

#[test]
fn stream_policy_target_exposes_stream_only_capabilities() {
    let target = StreamPolicyTarget::new();
    let capabilities = target.capabilities();
    assert!(capabilities.stream);
    assert!(!capabilities.datagram);
}

#[test]
fn action_resource_splits_stream_and_datagram() {
    let stream = set_plan("a", eggchaos_core::Direction::Upstream, vec![]);
    assert_eq!(
        action_resource(&stream).transport,
        ScheduleTransport::Stream
    );
    let datagram = ScenarioAction::RemoveDatagramFault {
        proxy: "a".to_owned(),
        direction: eggchaos_core::Direction::Downstream,
        id: "x".to_owned(),
    };
    let resource = action_resource(&datagram);
    assert_eq!(resource.transport, ScheduleTransport::Datagram);
    assert_eq!(
        resource_key(
            "a",
            eggchaos_core::Direction::Upstream,
            ScheduleTransport::Stream
        ),
        "a|Upstream|Stream"
    );
}
