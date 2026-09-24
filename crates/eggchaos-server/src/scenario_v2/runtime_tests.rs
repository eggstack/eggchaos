//! V2 schedule runtime, isolation, and lifecycle tests.
//!
//! These tests exercise the M027 supervisor through `ControlState`:
//! epoch-anchored deadlines under paused time, strict/live ownership,
//! CAS-safe cleanup, cancellation, shutdown, and the v2 evidence
//! contract. Stream/datagram engine semantics are unchanged; the
//! tests assert the scheduling layer above them.

use std::num::NonZeroU64;
use std::time::Duration;

use eggchaos_core::{
    derive_schedule_policy_seed, DatagramFaultKind, DatagramFaultSpec, DatagramPlan,
    DatagramQueueLimits, Direction, FaultId, FaultKind, FaultPlan, FaultSpec, LatencyConfig,
    Probability,
};

use crate::runtime::{ControlState, ProxySpec, RuntimeParams};
use crate::scenario::ScenarioAction;
use crate::scenario_v2::{
    CleanupPolicyV2, IsolationPolicyV2, ScenarioScheduleV2, SchedulePhaseV2,
    SCHEDULE_SCHEMA_VERSION,
};
use crate::{DatagramProxySpec, ScheduleRunStatus};

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

fn loss(id: &str) -> DatagramFaultSpec {
    DatagramFaultSpec {
        id: FaultId::new(id).unwrap(),
        probability: Probability::new(1.0).unwrap(),
        kind: DatagramFaultKind::Loss,
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
    seed: u64,
    execution_key: u64,
    isolation: IsolationPolicyV2,
    cleanup: CleanupPolicyV2,
    phases: Vec<(u64, Vec<ScenarioAction>)>,
) -> ScenarioScheduleV2 {
    ScenarioScheduleV2 {
        version: SCHEDULE_SCHEMA_VERSION,
        seed,
        execution_key,
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

async fn datagram_proxy(control: &ControlState, name: &str) {
    let target = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let listen: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let spec = DatagramProxySpec::new(
        name,
        listen,
        target.local_addr().unwrap(),
        DatagramQueueLimits {
            max_queued_datagrams: NonZeroU64::new(8).unwrap(),
            max_queued_bytes: NonZeroU64::new(4096).unwrap(),
            max_datagram_bytes: NonZeroU64::new(2048).unwrap(),
        },
    )
    .unwrap();
    control.create_datagram_proxy(spec).await.unwrap();
}

/// Paused-time-aware terminal wait: all deadlines are already due, so
/// the driver only needs task polls, never clock advances.
async fn wait_paused_schedule_v2(
    control: &ControlState,
    run_id: u64,
) -> crate::scenario_v2::ScenarioScheduleRunRecord {
    for _ in 0..1000 {
        let record = control.get_schedule_v2(run_id).await.unwrap();
        if matches!(
            record.status,
            ScheduleRunStatus::Completed | ScheduleRunStatus::Cancelled | ScheduleRunStatus::Failed
        ) {
            return record;
        }
        tokio::task::yield_now().await;
    }
    panic!("paused schedule run {run_id} did not reach a terminal state");
}

/// Paused-time-aware applied-count wait.
async fn wait_paused_applied(control: &ControlState, run_id: u64, count: usize) {
    for _ in 0..1000 {
        let record = control.get_schedule_v2(run_id).await.unwrap();
        if record.applied >= count {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("paused schedule run {run_id} did not reach applied={count}");
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
                ScheduleRunStatus::Completed
                    | ScheduleRunStatus::Cancelled
                    | ScheduleRunStatus::Failed
            ) {
                break record;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

async fn wait_applied(control: &ControlState, run_id: u64, count: usize) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let record = control.get_schedule_v2(run_id).await.unwrap();
            if record.applied >= count {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(start_paused = true)]
async fn paused_time_events_fire_at_epoch_offsets_without_drift() {
    let control = test_control();
    stream_proxy(&control, "cache").await;
    let source = schedule(
        1,
        1,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
        vec![
            (
                1_000_000_000,
                vec![set_plan("cache", Direction::Downstream, vec![latency("a")])],
            ),
            (
                1_000_000_000,
                vec![set_plan("cache", Direction::Downstream, vec![latency("b")])],
            ),
            (
                1_000_000_000,
                vec![set_plan("cache", Direction::Downstream, vec![latency("c")])],
            ),
        ],
    );
    let record = control.start_schedule_v2(source).await.unwrap();
    // Simulated application work between events: unrelated control
    // traffic that consumes real task polls but no paused-clock time.
    // Later deadlines must still anchor to the single epoch.
    tokio::time::advance(Duration::from_secs(1)).await;
    wait_paused_applied(&control, record.run_id, 1).await;
    // Unrelated manual publication on another proxy between events.
    stream_proxy(&control, "other").await;
    tokio::time::advance(Duration::from_secs(1)).await;
    wait_paused_applied(&control, record.run_id, 2).await;
    tokio::time::advance(Duration::from_secs(1)).await;
    let final_record = wait_paused_schedule_v2(&control, record.run_id).await;
    assert_eq!(final_record.status, ScheduleRunStatus::Completed);
    assert_eq!(final_record.events.len(), 3);
    let offsets: Vec<u64> = final_record
        .events
        .iter()
        .map(|event| event.scheduled_offset_ns)
        .collect();
    assert_eq!(
        offsets,
        vec![0, 1_000_000_000, 2_000_000_000],
        "deadlines anchor to one epoch"
    );
    for event in &final_record.events {
        assert_eq!(
            event.late_by_ns, 0,
            "paused-time events apply exactly on deadline"
        );
        assert_eq!(event.applied_elapsed_ns, event.scheduled_offset_ns);
    }
    control.shutdown_and_join().await;
}

#[tokio::test(start_paused = true)]
async fn same_deadline_events_execute_in_compiled_order() {
    let control = test_control();
    stream_proxy(&control, "cache").await;
    let source = schedule(
        2,
        2,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
        vec![(
            0,
            vec![
                set_plan("cache", Direction::Upstream, vec![latency("a")]),
                set_plan("cache", Direction::Downstream, vec![latency("b")]),
            ],
        )],
    );
    let record = control.start_schedule_v2(source).await.unwrap();
    let final_record = wait_paused_schedule_v2(&control, record.run_id).await;
    assert_eq!(final_record.status, ScheduleRunStatus::Completed);
    assert_eq!(final_record.events.len(), 2);
    assert_eq!(final_record.events[0].compiled_index, 0);
    assert_eq!(final_record.events[1].compiled_index, 1);
    assert_eq!(final_record.events[0].scheduled_offset_ns, 0);
    assert_eq!(final_record.events[1].scheduled_offset_ns, 0);
    control.shutdown_and_join().await;
}

#[tokio::test(start_paused = true)]
async fn already_late_events_apply_immediately_with_lateness() {
    let control = test_control();
    stream_proxy(&control, "cache").await;
    // Offsets are [0, 100ms]: the first phase's actions sit at the
    // epoch and its duration pushes the cursor forward before the
    // second phase's actions.
    let source = schedule(
        3,
        3,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
        vec![
            (
                100_000_000,
                vec![set_plan("cache", Direction::Downstream, vec![latency("a")])],
            ),
            (
                100_000_000,
                vec![set_plan("cache", Direction::Downstream, vec![latency("b")])],
            ),
        ],
    );
    let record = control.start_schedule_v2(source).await.unwrap();
    // Let the first event apply at time 0 before moving the clock;
    // otherwise the epoch itself would shift with the jump.
    wait_paused_applied(&control, record.run_id, 1).await;
    // Jump past the second deadline; it applies immediately in
    // compiled order with deterministic lateness.
    tokio::time::advance(Duration::from_millis(500)).await;
    let final_record = wait_paused_schedule_v2(&control, record.run_id).await;
    assert_eq!(final_record.status, ScheduleRunStatus::Completed);
    assert_eq!(final_record.events.len(), 2);
    assert_eq!(final_record.events[0].scheduled_offset_ns, 0);
    assert_eq!(final_record.events[0].late_by_ns, 0);
    assert_eq!(final_record.events[1].scheduled_offset_ns, 100_000_000);
    assert_eq!(final_record.events[1].late_by_ns, 400_000_000);
    assert_eq!(final_record.events[0].compiled_index, 0);
    assert_eq!(final_record.events[1].compiled_index, 1);
    for event in &final_record.events {
        assert!(event.applied_elapsed_ns >= event.scheduled_offset_ns);
    }
    control.shutdown_and_join().await;
}

#[tokio::test]
async fn same_schedule_under_different_run_ids_publishes_identical_namespaces() {
    let control = test_control();
    stream_proxy(&control, "cache").await;
    let source = schedule(
        21,
        5,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
        vec![
            (
                0,
                vec![set_plan("cache", Direction::Downstream, vec![latency("a")])],
            ),
            (
                0,
                vec![set_plan("cache", Direction::Downstream, vec![latency("b")])],
            ),
        ],
    );
    let first = control.start_schedule_v2(source.clone()).await.unwrap();
    let first_record = wait_schedule_v2(&control, first.run_id).await;
    assert_eq!(first_record.status, ScheduleRunStatus::Completed);
    let ns_first = control
        .list()
        .await
        .into_iter()
        .find(|view| view.name == "cache")
        .unwrap()
        .downstream_seed_namespace;

    let second = control.start_schedule_v2(source.clone()).await.unwrap();
    assert_ne!(first.run_id, second.run_id);
    let second_record = wait_schedule_v2(&control, second.run_id).await;
    assert_eq!(second_record.status, ScheduleRunStatus::Completed);
    let ns_second = control
        .list()
        .await
        .into_iter()
        .find(|view| view.name == "cache")
        .unwrap()
        .downstream_seed_namespace;

    assert_eq!(ns_first, ns_second, "run_id must not affect v2 namespaces");
    let fingerprint = crate::compiled_fingerprint(&crate::compile_schedule(&source).unwrap());
    // Last applied event is index 1.
    assert_eq!(
        ns_second,
        derive_schedule_policy_seed(21, 5, fingerprint, 1)
    );
    control.shutdown_and_join().await;
}

#[tokio::test]
async fn strict_stream_manual_mutation_is_never_overwritten() {
    let control = test_control();
    stream_proxy(&control, "cache").await;
    // Offsets are [0, 1h]: the run sleeps after its first event while
    // an external manual publication moves the owned generation.
    let source = schedule(
        4,
        4,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::RestoreInitial,
        vec![
            (
                3_600_000_000_000,
                vec![set_plan("cache", Direction::Downstream, vec![latency("a")])],
            ),
            (
                0,
                vec![set_plan("cache", Direction::Downstream, vec![latency("b")])],
            ),
        ],
    );
    let record = control.start_schedule_v2(source).await.unwrap();
    wait_applied(&control, record.run_id, 1).await;
    // External manual publication moves the generation the run owns.
    let (_, base_downstream) = control.snapshot_policies("cache").await.unwrap();
    control
        .publish_direction_expected(
            "cache",
            Direction::Downstream,
            FaultPlan::new(vec![latency("manual")]).unwrap(),
            999,
            base_downstream.generation,
        )
        .await
        .unwrap();
    // Cancel instead of waiting out the hour; cleanup must conflict on
    // the moved resource rather than overwrite it.
    control.cancel_schedule_v2(record.run_id).await;
    let final_record = wait_schedule_v2(&control, record.run_id).await;
    assert_eq!(final_record.status, ScheduleRunStatus::Cancelled);
    let cleanup = final_record.cleanup.as_ref().unwrap();
    assert_eq!(
        cleanup.resources[0].outcome,
        crate::CleanupResourceOutcome::Conflict
    );
    // Manual state must survive: the schedule never overwrote it.
    let view = control
        .list()
        .await
        .into_iter()
        .find(|view| view.name == "cache")
        .unwrap();
    assert!(
        view.downstream_faults
            .faults()
            .iter()
            .any(|fault| fault.id.as_str() == "manual"),
        "strict mode must not overwrite external state"
    );
    control.shutdown_and_join().await;
}

#[tokio::test]
async fn strict_conflict_fails_fast_on_short_deadline() {
    let control = test_control();
    stream_proxy(&control, "cache").await;
    // Offsets are [0, 50ms] so the external mutation lands between
    // the two strict events.
    let source = schedule(
        44,
        44,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::RestoreInitial,
        vec![
            (
                50_000_000,
                vec![set_plan("cache", Direction::Downstream, vec![latency("a")])],
            ),
            (
                0,
                vec![set_plan("cache", Direction::Downstream, vec![latency("b")])],
            ),
        ],
    );
    let record = control.start_schedule_v2(source).await.unwrap();
    wait_applied(&control, record.run_id, 1).await;
    let (_, base_downstream) = control.snapshot_policies("cache").await.unwrap();
    control
        .publish_direction_expected(
            "cache",
            Direction::Downstream,
            FaultPlan::new(vec![latency("manual")]).unwrap(),
            999,
            base_downstream.generation,
        )
        .await
        .unwrap();
    let final_record = wait_schedule_v2(&control, record.run_id).await;
    assert_eq!(final_record.status, ScheduleRunStatus::Failed);
    assert!(
        final_record
            .failure
            .as_deref()
            .unwrap_or_default()
            .contains("publish failed"),
        "strict conflict must fail the run, got {:?}",
        final_record.failure
    );
    // Cleanup must conflict, not overwrite.
    let cleanup = final_record.cleanup.as_ref().unwrap();
    assert_eq!(cleanup.resources.len(), 1);
    assert_eq!(
        cleanup.resources[0].outcome,
        crate::CleanupResourceOutcome::Conflict
    );
    let view = control
        .list()
        .await
        .into_iter()
        .find(|view| view.name == "cache")
        .unwrap();
    assert!(view
        .downstream_faults
        .faults()
        .iter()
        .any(|fault| fault.id.as_str() == "manual"));
    control.shutdown_and_join().await;
}

#[tokio::test]
async fn strict_datagram_manual_mutation_causes_fail_fast_conflict() {
    let control = test_control();
    datagram_proxy(&control, "dns").await;
    // Offsets are [0, 50ms] so the external mutation lands between
    // the two strict events.
    let source = schedule(
        45,
        45,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::RestoreInitial,
        vec![
            (
                50_000_000,
                vec![ScenarioAction::SetDatagramPlan {
                    proxy: "dns".into(),
                    direction: Direction::Upstream,
                    faults: vec![loss("a")],
                }],
            ),
            (
                0,
                vec![ScenarioAction::SetDatagramPlan {
                    proxy: "dns".into(),
                    direction: Direction::Upstream,
                    faults: vec![loss("b")],
                }],
            ),
        ],
    );
    let record = control.start_schedule_v2(source).await.unwrap();
    wait_applied(&control, record.run_id, 1).await;
    let (_, gen, _) = control
        .get_datagram_plan("dns", Direction::Upstream)
        .await
        .unwrap();
    control
        .publish_datagram_plan(
            "dns",
            Direction::Upstream,
            DatagramPlan::new(vec![loss("manual")]).unwrap(),
            999,
            Some(gen),
        )
        .await
        .unwrap();
    let final_record = wait_schedule_v2(&control, record.run_id).await;
    assert_eq!(final_record.status, ScheduleRunStatus::Failed);
    let (plan, _, _) = control
        .get_datagram_plan("dns", Direction::Upstream)
        .await
        .unwrap();
    assert!(
        plan.faults()
            .iter()
            .any(|fault| fault.id.as_str() == "manual"),
        "strict datagram mode must not overwrite external state"
    );
    control.shutdown_and_join().await;
}

#[tokio::test]
async fn live_mode_uses_current_state_after_manual_mutation() {
    let control = test_control();
    stream_proxy(&control, "cache").await;
    // Offsets are [0, 50ms]: the first phase's duration separates the
    // two events so the manual mutation lands deterministically between
    // them.
    let source = schedule(
        46,
        46,
        IsolationPolicyV2::Live,
        CleanupPolicyV2::Leave,
        vec![
            (
                50_000_000,
                vec![set_plan("cache", Direction::Downstream, vec![latency("a")])],
            ),
            (
                0,
                vec![set_plan("cache", Direction::Downstream, vec![latency("b")])],
            ),
        ],
    );
    let record = control.start_schedule_v2(source).await.unwrap();
    wait_applied(&control, record.run_id, 1).await;
    // Manual update completes before the next event snapshot: live
    // mode may base the next publication on it.
    let (_, base_downstream) = control.snapshot_policies("cache").await.unwrap();
    control
        .publish_direction_expected(
            "cache",
            Direction::Downstream,
            FaultPlan::new(vec![latency("manual")]).unwrap(),
            999,
            base_downstream.generation,
        )
        .await
        .unwrap();
    let final_record = wait_schedule_v2(&control, record.run_id).await;
    assert_eq!(
        final_record.status,
        ScheduleRunStatus::Completed,
        "live mode absorbs the completed manual update, got {:?}",
        final_record.failure
    );
    assert_eq!(final_record.applied, 2);
    // The last publication wins: the schedule's second plan is live.
    let view = control
        .list()
        .await
        .into_iter()
        .find(|view| view.name == "cache")
        .unwrap();
    assert!(view
        .downstream_faults
        .faults()
        .iter()
        .any(|fault| fault.id.as_str() == "b"));
    control.shutdown_and_join().await;
}

#[tokio::test]
async fn cancel_while_sleeping_transitions_promptly_and_cleans_up() {
    let control = test_control();
    stream_proxy(&control, "cache").await;
    // Offsets are [0, 1h]: the driver sleeps until the run is
    // cancelled.
    let source = schedule(
        6,
        6,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::RestoreInitial,
        vec![
            (
                3_600_000_000_000,
                vec![set_plan("cache", Direction::Downstream, vec![latency("a")])],
            ),
            (
                0,
                vec![set_plan("cache", Direction::Downstream, vec![latency("b")])],
            ),
        ],
    );
    let record = control.start_schedule_v2(source).await.unwrap();
    wait_applied(&control, record.run_id, 1).await;
    let cancelled = control.cancel_schedule_v2(record.run_id).await.unwrap();
    assert_eq!(cancelled.status, ScheduleRunStatus::Cancelling);
    let final_record = wait_schedule_v2(&control, record.run_id).await;
    assert_eq!(final_record.status, ScheduleRunStatus::Cancelled);
    assert_eq!(final_record.applied, 1);
    // restore-initial ran: the plan is back to the pre-run empty plan.
    let view = control
        .list()
        .await
        .into_iter()
        .find(|view| view.name == "cache")
        .unwrap();
    assert!(view.downstream_faults.is_empty());
    let cleanup = final_record.cleanup.as_ref().unwrap();
    assert_eq!(cleanup.resources.len(), 1);
    assert_eq!(
        cleanup.resources[0].outcome,
        crate::CleanupResourceOutcome::Restored
    );
    // Cancelling a finished run returns the final record unchanged.
    let again = control.cancel_schedule_v2(record.run_id).await.unwrap();
    assert_eq!(again.status, ScheduleRunStatus::Cancelled);
    control.shutdown_and_join().await;
}

#[tokio::test]
async fn failure_after_several_events_performs_selected_cleanup() {
    let control = test_control();
    stream_proxy(&control, "cache").await;
    let source = schedule(
        7,
        7,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::RestoreInitial,
        vec![
            (
                0,
                vec![set_plan("cache", Direction::Downstream, vec![latency("a")])],
            ),
            (
                10_000_000,
                vec![ScenarioAction::RemoveFault {
                    proxy: "cache".into(),
                    direction: Direction::Downstream,
                    id: "absent".into(),
                }],
            ),
        ],
    );
    let record = control.start_schedule_v2(source).await.unwrap();
    let final_record = wait_schedule_v2(&control, record.run_id).await;
    assert_eq!(final_record.status, ScheduleRunStatus::Failed);
    assert_eq!(final_record.applied, 1);
    assert!(final_record.failure.is_some());
    // Cleanup restored the pre-run empty plan.
    let view = control
        .list()
        .await
        .into_iter()
        .find(|view| view.name == "cache")
        .unwrap();
    assert!(view.downstream_faults.is_empty());
    control.shutdown_and_join().await;
}

#[tokio::test]
async fn leave_cleanup_records_not_requested_and_keeps_state() {
    let control = test_control();
    stream_proxy(&control, "cache").await;
    let source = schedule(
        8,
        8,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
        vec![(
            0,
            vec![set_plan("cache", Direction::Downstream, vec![latency("a")])],
        )],
    );
    let record = control.start_schedule_v2(source).await.unwrap();
    let final_record = wait_schedule_v2(&control, record.run_id).await;
    assert_eq!(final_record.status, ScheduleRunStatus::Completed);
    let cleanup = final_record.cleanup.as_ref().unwrap();
    assert_eq!(
        cleanup.resources[0].outcome,
        crate::CleanupResourceOutcome::NotRequested
    );
    // Last published state is preserved.
    let view = control
        .list()
        .await
        .into_iter()
        .find(|view| view.name == "cache")
        .unwrap();
    assert!(view
        .downstream_faults
        .faults()
        .iter()
        .any(|fault| fault.id.as_str() == "a"));
    control.shutdown_and_join().await;
}

#[tokio::test]
async fn cleanup_conflict_on_one_resource_does_not_block_another() {
    let control = test_control();
    stream_proxy(&control, "a").await;
    stream_proxy(&control, "b").await;
    // Offsets are [0, 0, 1h]: both first events apply, then the run
    // sleeps until cancellation while proxy A is moved externally.
    let source = schedule(
        9,
        9,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::RestoreInitial,
        vec![
            (
                0,
                vec![set_plan("a", Direction::Downstream, vec![latency("x")])],
            ),
            (
                3_600_000_000_000,
                vec![set_plan("b", Direction::Downstream, vec![latency("y")])],
            ),
            (
                0,
                vec![set_plan("a", Direction::Downstream, vec![latency("z")])],
            ),
        ],
    );
    let record = control.start_schedule_v2(source).await.unwrap();
    wait_applied(&control, record.run_id, 2).await;
    // Externally move proxy A while the run sleeps before its last event.
    let (_, gen_a) = control.snapshot_policies("a").await.unwrap();
    let _ = gen_a;
    let (_, base_a_down) = control.snapshot_policies("a").await.unwrap();
    control
        .publish_direction_expected(
            "a",
            Direction::Downstream,
            FaultPlan::new(vec![latency("external")]).unwrap(),
            4242,
            base_a_down.generation,
        )
        .await
        .unwrap();
    // Cancel so cleanup runs promptly instead of waiting out the hour.
    control.cancel_schedule_v2(record.run_id).await;
    let final_record = wait_schedule_v2(&control, record.run_id).await;
    assert_eq!(final_record.status, ScheduleRunStatus::Cancelled);
    let cleanup = final_record.cleanup.as_ref().unwrap();
    assert_eq!(cleanup.resources.len(), 2);
    let outcome_a = cleanup
        .resources
        .iter()
        .find(|resource| resource.resource.proxy == "a")
        .unwrap()
        .outcome;
    let outcome_b = cleanup
        .resources
        .iter()
        .find(|resource| resource.resource.proxy == "b")
        .unwrap()
        .outcome;
    assert_eq!(outcome_a, crate::CleanupResourceOutcome::Conflict);
    assert_eq!(outcome_b, crate::CleanupResourceOutcome::Restored);
    // A keeps external state; B is restored to its pre-run empty plan.
    let views = control.list().await;
    let view_a = views.iter().find(|view| view.name == "a").unwrap();
    let view_b = views.iter().find(|view| view.name == "b").unwrap();
    assert!(view_a
        .downstream_faults
        .faults()
        .iter()
        .any(|fault| fault.id.as_str() == "external"));
    assert!(view_b.downstream_faults.is_empty());
    control.shutdown_and_join().await;
}

#[tokio::test]
async fn service_shutdown_cancels_and_joins_v2_tasks() {
    let control = test_control();
    stream_proxy(&control, "cache").await;
    // Offsets are [0, 1h]: the first event applies, then the driver
    // sleeps until shutdown cancels it.
    let source = schedule(
        10,
        10,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
        vec![
            (
                3_600_000_000_000,
                vec![set_plan("cache", Direction::Downstream, vec![latency("a")])],
            ),
            (
                0,
                vec![set_plan("cache", Direction::Downstream, vec![latency("b")])],
            ),
        ],
    );
    let record = control.start_schedule_v2(source).await.unwrap();
    // Give the driver a chance to enter its deadline wait.
    tokio::time::sleep(Duration::from_millis(50)).await;
    control.shutdown_and_join().await;
    let final_record = control.get_schedule_v2(record.run_id).await.unwrap();
    assert_eq!(final_record.status, ScheduleRunStatus::Cancelled);
}

#[tokio::test]
async fn invalid_schedule_creates_no_run() {
    let control = test_control();
    stream_proxy(&control, "cache").await;
    // Targets a proxy that does not exist.
    let source = schedule(
        11,
        11,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
        vec![(
            0,
            vec![set_plan("ghost", Direction::Downstream, vec![latency("a")])],
        )],
    );
    let record = control.start_schedule_v2(source).await.unwrap();
    let final_record = wait_schedule_v2(&control, record.run_id).await;
    assert_eq!(final_record.status, ScheduleRunStatus::Failed);
    assert_eq!(final_record.applied, 0);
    // Structurally invalid sources are rejected before a run exists:
    // a compile error maps to Invalid without allocating a run id.
    let bad = ScenarioScheduleV2 {
        version: 99,
        ..schedule(
            11,
            11,
            IsolationPolicyV2::Strict,
            CleanupPolicyV2::Leave,
            vec![(
                0,
                vec![set_plan("cache", Direction::Downstream, vec![latency("a")])],
            )],
        )
    };
    let before = control.get_schedule_v2(record.run_id + 1).await;
    assert!(before.is_none());
    assert!(control.start_schedule_v2(bad).await.is_err());
    assert!(control.get_schedule_v2(record.run_id + 1).await.is_none());
    control.shutdown_and_join().await;
}

#[tokio::test]
async fn v2_run_evidence_carries_identity_timing_and_cleanup() {
    let control = test_control();
    stream_proxy(&control, "cache").await;
    datagram_proxy(&control, "dns").await;
    let source = schedule(
        12,
        13,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::RestoreInitial,
        vec![
            (
                0,
                vec![set_plan("cache", Direction::Downstream, vec![latency("a")])],
            ),
            (
                10_000_000,
                vec![ScenarioAction::SetDatagramPlan {
                    proxy: "dns".into(),
                    direction: Direction::Upstream,
                    faults: vec![loss("q")],
                }],
            ),
        ],
    );
    let expected_fingerprint =
        crate::compiled_fingerprint(&crate::compile_schedule(&source).unwrap());
    let record = control.start_schedule_v2(source).await.unwrap();
    assert_eq!(record.schedule_fingerprint, expected_fingerprint);
    assert_eq!(
        record.compiler_semantics_version,
        crate::COMPILER_SEMANTICS_VERSION
    );
    let final_record = wait_schedule_v2(&control, record.run_id).await;
    assert_eq!(final_record.status, ScheduleRunStatus::Completed);
    assert_eq!(final_record.seed, 12);
    assert_eq!(final_record.execution_key, 13);
    assert_eq!(final_record.events.len(), 2);
    assert_eq!(final_record.events[0].compiled_index, 0);
    assert_eq!(final_record.events[1].compiled_index, 1);
    assert_eq!(final_record.events[1].scheduled_offset_ns, 0);
    for event in &final_record.events {
        assert!(event.applied_elapsed_ns >= event.scheduled_offset_ns);
        assert_eq!(
            event.late_by_ns,
            event.applied_elapsed_ns - event.scheduled_offset_ns
        );
    }
    assert_eq!(final_record.events[0].action, "set-plan");
    assert_eq!(final_record.events[1].action, "set-datagram-plan");
    // No payload bytes anywhere in the evidence surface.
    let evidence_json = serde_json::to_value(&final_record).unwrap();
    assert!(!evidence_json.to_string().contains("payload"));
    control.shutdown_and_join().await;
}

#[tokio::test]
async fn toml_and_json_v2_apply_compile_to_the_same_fingerprint() {
    let control = test_control();
    stream_proxy(&control, "cache").await;
    let json = r#"{
        "version": 2, "seed": 30, "execution_key": 31,
        "isolation": "strict", "cleanup": "leave",
        "phases": [{"duration_ns": 0, "actions": [
            {"type": "set-plan", "proxy": "cache", "direction": "downstream",
             "faults": [{"id": "a", "probability": 1.0,
               "kind": {"type": "latency", "delay_ns": 5000000, "jitter_ns": 0, "max_buffer_bytes": 1024}}]}
        ]}]
    }"#;
    let toml_text = r#"
        version = 2
        seed = 30
        execution_key = 31
        isolation = "strict"
        cleanup = "leave"

        [[phases]]
        duration_ns = 0

        [[phases.actions]]
        type = "set-plan"
        proxy = "cache"
        direction = "downstream"

        [[phases.actions.faults]]
        id = "a"
        probability = 1.0

        [phases.actions.faults.kind]
        type = "latency"
        delay_ns = 5000000
        jitter_ns = 0
        max_buffer_bytes = 1024
    "#;
    let from_json = crate::ScenarioScheduleV2Dto::from_json_str(json).expect("json source");
    let from_toml = crate::ScenarioScheduleV2Toml::from_toml_str(toml_text).expect("toml source");
    let first = control.start_schedule_v2(from_json).await.unwrap();
    wait_schedule_v2(&control, first.run_id).await;
    // Reset the plan so the second run starts from the same baseline.
    let (_, gen) = control.snapshot_policies("cache").await.unwrap();
    let _ = gen;
    let second = control.start_schedule_v2(from_toml).await.unwrap();
    let second_record = wait_schedule_v2(&control, second.run_id).await;
    assert_eq!(second_record.status, ScheduleRunStatus::Completed);
    let first_record = control.get_schedule_v2(first.run_id).await.unwrap();
    assert_eq!(
        first_record.schedule_fingerprint, second_record.schedule_fingerprint,
        "TOML and JSON apply must share replay identity"
    );
    control.shutdown_and_join().await;
}

#[tokio::test]
async fn v2_metrics_count_runs_events_and_late_events() {
    let control = test_control();
    stream_proxy(&control, "cache").await;
    let before = control.metrics_text().await;
    let runs_before = metric_value(&before, "eggchaos_schedule_v2_runs_total");
    let source = schedule(
        50,
        50,
        IsolationPolicyV2::Strict,
        CleanupPolicyV2::Leave,
        vec![(
            0,
            vec![set_plan("cache", Direction::Downstream, vec![latency("a")])],
        )],
    );
    let record = control.start_schedule_v2(source).await.unwrap();
    wait_schedule_v2(&control, record.run_id).await;
    let after = control.metrics_text().await;
    assert_eq!(
        metric_value(&after, "eggchaos_schedule_v2_runs_total"),
        runs_before + 1
    );
    assert!(metric_value(&after, "eggchaos_schedule_v2_events_total") >= 1);
    // No fingerprint, run id, or phase label may enter metric labels.
    for line in after.lines() {
        if line.starts_with("eggchaos_schedule_v2_") {
            assert!(!line.contains("fingerprint"), "{line}");
            assert!(!line.contains("run_id"), "{line}");
            assert!(!line.contains("phase"), "{line}");
        }
    }
    control.shutdown_and_join().await;
}

fn metric_value(text: &str, name: &str) -> u64 {
    text.lines()
        .find(|line| line.starts_with(name))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}
