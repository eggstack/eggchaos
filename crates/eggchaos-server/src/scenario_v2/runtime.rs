//! Owner of the v2 schedule run lifecycle.
//!
//! This module drives one `CompiledScenarioV2` event tape through the
//! existing `ControlState` publication paths. The driver is pure
//! control-plane code: it never touches the stream/datagram engines
//! directly, never holds a global lock for the run, and never opens
//! sockets. It anchors absolute deadlines to one captured monotonic
//! `tokio::time::Instant` and tracks per-resource logical generation
//! ownership so cleanup is CAS-safe and non-clobbering.

use std::collections::BTreeMap;
use std::time::Duration;

use eggchaos_core::{derive_schedule_policy_seed, DatagramPlan, Direction, FaultPlan};
use tokio::time::Instant;

use crate::scenario::ScenarioAction;
use crate::ControlState;

use super::compiler::{CompiledEventV2, CompiledScenarioV2};
use super::fingerprint::compiled_fingerprint;
use super::run::{
    CleanupOutcome, CleanupResourceOutcome, CleanupResourceRecord, ScheduleEventResult,
    ScheduleResource, ScheduleRunStatus, ScheduleTransport,
};
use super::source::IsolationPolicyV2;

fn resource_key(proxy: &str, direction: Direction, transport: ScheduleTransport) -> String {
    format!("{proxy}|{:?}|{transport:?}", direction)
}

#[derive(Debug, Clone)]
struct OwnedDirectionState {
    last_owned_generation: u64,
    initial_plan: InitialPlan,
}

#[derive(Debug, Clone)]
enum InitialPlan {
    Stream(FaultPlan),
    Datagram(DatagramPlan),
}

struct CleanupTarget {
    resource: ScheduleResource,
    state: OwnedDirectionState,
}

/// Drive one owned v2 schedule run to completion, cancellation, or
/// fail-fast failure. Per-event evidence accumulates in the run
/// record through `append_schedule_v2_event`; the driver keeps no
/// local event buffer so the record is the single source of truth.
pub(crate) async fn drive_schedule_v2_run(
    state: ControlState,
    run_id: u64,
    compiled: CompiledScenarioV2,
    token: tokio_util::sync::CancellationToken,
) {
    state
        .update_schedule_v2_run(run_id, |record| {
            record.status = ScheduleRunStatus::Running;
        })
        .await;

    let schedule_fingerprint = compiled_fingerprint(&compiled);
    let epoch = Instant::now();
    let mut owned: BTreeMap<String, CleanupTarget> = BTreeMap::new();
    let mut did_discover = false;
    let mut failure: Option<String> = None;
    let mut cancelled = false;

    for event in compiled.events.iter() {
        if !did_discover {
            match snapshot_initial_state(&state, &compiled).await {
                Ok(initial) => {
                    owned = initial;
                    did_discover = true;
                }
                Err(reason) => {
                    failure = Some(reason);
                    break;
                }
            }
        }
        if wait_for_deadline(&token, epoch, event.offset_ns)
            .await
            .is_err()
        {
            cancelled = true;
            break;
        }
        if let Err(reason) = apply_event(
            &state,
            run_id,
            &compiled,
            event,
            &mut owned,
            epoch,
            schedule_fingerprint,
        )
        .await
        {
            failure = Some(reason);
            break;
        }
    }

    let terminal = if cancelled {
        ScheduleRunStatus::Cancelled
    } else if failure.is_some() {
        ScheduleRunStatus::Failed
    } else {
        ScheduleRunStatus::Completed
    };
    state
        .update_schedule_v2_run(run_id, |record| {
            record.status = terminal;
            if record.status == ScheduleRunStatus::Failed {
                record.failure.clone_from(&failure);
            }
        })
        .await;

    let cleanup_outcome = run_cleanup(&state, &compiled, owned).await;
    state
        .update_schedule_v2_run(run_id, |record| {
            record.cleanup = Some(cleanup_outcome);
        })
        .await;
    state.remove_schedule_v2_token(run_id).await;
}

/// Wait until `epoch + offset_ns` or cancellation wins.
///
/// Already-due deadlines (`now >= deadline`) skip the sleep so drift
/// never accumulates; this is the documented ADR 004 contract.
///
/// The epoch/deadline sum uses `checked_add`: a `u64` offset near the
/// representation limit cannot overflow the monotonic clock into a
/// panic. An unrepresentable deadline waits (pending) until
/// cancellation, which is the only prompt exit from such a wait.
async fn wait_for_deadline(
    token: &tokio_util::sync::CancellationToken,
    epoch: Instant,
    offset_ns: u64,
) -> Result<(), ()> {
    let Some(deadline) = epoch.checked_add(Duration::from_nanos(offset_ns)) else {
        tokio::select! {
            biased;
            () = token.cancelled() => return Err(()),
            () = std::future::pending::<()>() => return Ok(()),
        }
    };
    if Instant::now() >= deadline {
        return Ok(());
    }
    tokio::select! {
        biased;
        () = token.cancelled() => Err(()),
        () = tokio::time::sleep_until(deadline) => Ok(()),
    }
}

#[allow(clippy::too_many_arguments)]
async fn apply_event(
    state: &ControlState,
    run_id: u64,
    compiled: &CompiledScenarioV2,
    event: &CompiledEventV2,
    owned: &mut BTreeMap<String, CleanupTarget>,
    epoch: Instant,
    schedule_fingerprint: [u8; 32],
) -> Result<(), String> {
    let applied_elapsed_ns = Instant::now().duration_since(epoch).as_nanos() as u64;
    let late_by_ns = applied_elapsed_ns.saturating_sub(event.offset_ns);
    let namespace = derive_schedule_policy_seed(
        compiled.seed,
        compiled.execution_key,
        schedule_fingerprint,
        u64::from(event.compiled_index),
    );
    let strict = matches!(compiled.isolation, IsolationPolicyV2::Strict);
    match &event.action {
        ScenarioAction::SetPlan {
            proxy,
            direction,
            faults,
        } => {
            let plan = FaultPlan::new(faults.clone())
                .map_err(|error| format!("set-plan invalid: {error}"))?;
            let key = resource_key(proxy, *direction, ScheduleTransport::Stream);
            let expected = expected_generation(state, proxy, *direction, strict, &key, owned)
                .await
                .ok_or_else(|| format!("stream proxy {proxy} not found"))?;
            let (global, new_gen) = state
                .publish_direction_expected(proxy, *direction, plan, namespace, expected)
                .await
                .map_err(|error| format!("set-plan publish failed: {error}"))?;
            update_owned_stream(owned, &key, proxy, *direction, new_gen);
            let (upstream, downstream) = directional_gens(state, proxy, *direction, new_gen).await;
            push_event(
                state,
                run_id,
                event,
                applied_elapsed_ns,
                late_by_ns,
                ScheduleResource {
                    proxy: proxy.clone(),
                    direction: *direction,
                    transport: ScheduleTransport::Stream,
                },
                upstream,
                downstream,
                global,
                "set-plan",
            )
            .await;
            Ok(())
        }
        ScenarioAction::RemoveFault {
            proxy,
            direction,
            id,
        } => {
            let Some((base_upstream, base_downstream)) = state.snapshot_policies(proxy).await
            else {
                return Err(format!("stream proxy {proxy} not found"));
            };
            let present = match direction {
                Direction::Upstream => base_upstream.plan.get(id).is_some(),
                Direction::Downstream => base_downstream.plan.get(id).is_some(),
            };
            if !present {
                return Err(format!("fault {id} not present on {proxy}"));
            }
            let plan = match direction {
                Direction::Upstream => (*base_upstream.plan).clone().without_fault(id),
                Direction::Downstream => (*base_downstream.plan).clone().without_fault(id),
            };
            let key = resource_key(proxy, *direction, ScheduleTransport::Stream);
            let expected = expected_generation(state, proxy, *direction, strict, &key, owned)
                .await
                .ok_or_else(|| format!("stream proxy {proxy} not found"))?;
            let (global, new_gen) = state
                .publish_direction_expected(proxy, *direction, plan, namespace, expected)
                .await
                .map_err(|error| format!("remove-fault publish failed: {error}"))?;
            update_owned_stream(owned, &key, proxy, *direction, new_gen);
            let (upstream, downstream) = directional_gens(state, proxy, *direction, new_gen).await;
            push_event(
                state,
                run_id,
                event,
                applied_elapsed_ns,
                late_by_ns,
                ScheduleResource {
                    proxy: proxy.clone(),
                    direction: *direction,
                    transport: ScheduleTransport::Stream,
                },
                upstream,
                downstream,
                global,
                "remove-fault",
            )
            .await;
            Ok(())
        }
        ScenarioAction::SetDatagramPlan {
            proxy,
            direction,
            faults,
        } => {
            let plan = DatagramPlan::new(faults.clone())
                .map_err(|error| format!("set-datagram-plan invalid: {error}"))?;
            let key = resource_key(proxy, *direction, ScheduleTransport::Datagram);
            let expected =
                expected_datagram_generation(state, proxy, *direction, strict, &key, owned)
                    .await
                    .ok_or_else(|| format!("datagram proxy {proxy} not found"))?;
            let new_gen = state
                .publish_datagram_plan(proxy, *direction, plan, namespace, Some(expected))
                .await
                .map_err(|error| format!("set-datagram-plan publish failed: {error}"))?;
            update_owned_datagram(owned, &key, proxy, *direction, new_gen);
            let (upstream, downstream) = match direction {
                Direction::Upstream => (new_gen, 0),
                Direction::Downstream => (0, new_gen),
            };
            push_event(
                state,
                run_id,
                event,
                applied_elapsed_ns,
                late_by_ns,
                ScheduleResource {
                    proxy: proxy.clone(),
                    direction: *direction,
                    transport: ScheduleTransport::Datagram,
                },
                upstream,
                downstream,
                state.generation(),
                "set-datagram-plan",
            )
            .await;
            Ok(())
        }
        ScenarioAction::RemoveDatagramFault {
            proxy,
            direction,
            id,
        } => {
            let current = state
                .get_datagram_plan(proxy, *direction)
                .await
                .map_err(|error| format!("datagram proxy {proxy} not found: {error}"))?;
            let mut faults = current.0.faults().to_vec();
            if !faults.iter().any(|fault| fault.id.as_str() == id) {
                return Err(format!("datagram fault {id} not present on {proxy}"));
            }
            faults.retain(|fault| fault.id.as_str() != id);
            let plan = DatagramPlan::new(faults)
                .map_err(|error| format!("datagram plan invalid: {error}"))?;
            let key = resource_key(proxy, *direction, ScheduleTransport::Datagram);
            let expected =
                expected_datagram_generation(state, proxy, *direction, strict, &key, owned)
                    .await
                    .ok_or_else(|| format!("datagram proxy {proxy} not found"))?;
            let new_gen = state
                .publish_datagram_plan(proxy, *direction, plan, namespace, Some(expected))
                .await
                .map_err(|error| format!("remove-datagram-fault publish failed: {error}"))?;
            update_owned_datagram(owned, &key, proxy, *direction, new_gen);
            push_event(
                state,
                run_id,
                event,
                applied_elapsed_ns,
                late_by_ns,
                ScheduleResource {
                    proxy: proxy.clone(),
                    direction: *direction,
                    transport: ScheduleTransport::Datagram,
                },
                0,
                0,
                state.generation(),
                "remove-datagram-fault",
            )
            .await;
            Ok(())
        }
    }
}

/// Resolve the expected generation for a stream publication.
///
/// Strict mode uses the last generation owned by this run (fail-fast
/// on external moves); live mode re-reads the live generation at
/// fire time so a completed manual update may become the base, while
/// a concurrent move during publication still conflicts.
async fn expected_generation(
    state: &ControlState,
    proxy: &str,
    direction: Direction,
    strict: bool,
    key: &str,
    owned: &BTreeMap<String, CleanupTarget>,
) -> Option<u64> {
    if strict {
        if let Some(target) = owned.get(key) {
            return Some(target.state.last_owned_generation);
        }
    }
    let (g_up, g_down) = state.current_generations(proxy).await?;
    Some(match direction {
        Direction::Upstream => g_up,
        Direction::Downstream => g_down,
    })
}

/// Datagram twin of [`expected_generation`].
async fn expected_datagram_generation(
    state: &ControlState,
    proxy: &str,
    direction: Direction,
    strict: bool,
    key: &str,
    owned: &BTreeMap<String, CleanupTarget>,
) -> Option<u64> {
    if strict {
        if let Some(target) = owned.get(key) {
            return Some(target.state.last_owned_generation);
        }
    }
    let (g_up, g_down) = state.datagram_current_generations(proxy).await?;
    Some(match direction {
        Direction::Upstream => g_up,
        Direction::Downstream => g_down,
    })
}

/// Pair the published generation with the untouched direction's
/// latest generation for run evidence.
async fn directional_gens(
    state: &ControlState,
    proxy: &str,
    direction: Direction,
    published: u64,
) -> (u64, u64) {
    let other = state
        .current_generations(proxy)
        .await
        .map(|(g_up, g_down)| match direction {
            Direction::Upstream => g_down,
            Direction::Downstream => g_up,
        })
        .unwrap_or(0);
    match direction {
        Direction::Upstream => (published, other),
        Direction::Downstream => (other, published),
    }
}

#[allow(clippy::too_many_arguments)]
async fn push_event(
    state: &ControlState,
    run_id: u64,
    event: &CompiledEventV2,
    applied_elapsed_ns: u64,
    late_by_ns: u64,
    resource: ScheduleResource,
    upstream_generation: u64,
    downstream_generation: u64,
    global_generation: u64,
    action: &'static str,
) {
    state
        .append_schedule_v2_event(
            run_id,
            ScheduleEventResult {
                compiled_index: event.compiled_index,
                phase: event.phase,
                scheduled_offset_ns: event.offset_ns,
                applied_elapsed_ns,
                late_by_ns,
                action: action.to_owned(),
                resource,
                upstream_generation,
                downstream_generation,
                global_generation,
            },
        )
        .await;
}

fn update_owned_stream(
    owned: &mut BTreeMap<String, CleanupTarget>,
    key: &str,
    proxy: &str,
    direction: Direction,
    new_gen: u64,
) {
    let entry = owned
        .entry(key.to_owned())
        .or_insert_with(|| CleanupTarget {
            resource: ScheduleResource {
                proxy: proxy.to_owned(),
                direction,
                transport: ScheduleTransport::Stream,
            },
            state: OwnedDirectionState {
                last_owned_generation: new_gen,
                initial_plan: InitialPlan::Stream(FaultPlan::empty()),
            },
        });
    entry.state.last_owned_generation = new_gen;
}

fn update_owned_datagram(
    owned: &mut BTreeMap<String, CleanupTarget>,
    key: &str,
    proxy: &str,
    direction: Direction,
    new_gen: u64,
) {
    let entry = owned
        .entry(key.to_owned())
        .or_insert_with(|| CleanupTarget {
            resource: ScheduleResource {
                proxy: proxy.to_owned(),
                direction,
                transport: ScheduleTransport::Datagram,
            },
            state: OwnedDirectionState {
                last_owned_generation: new_gen,
                initial_plan: InitialPlan::Datagram(DatagramPlan::empty()),
            },
        });
    entry.state.last_owned_generation = new_gen;
}

/// Snapshot the initial state for every resource the schedule
/// touches. Runs once before the first event so later cleanup knows
/// the pre-run baseline. A missing proxy fails the run before any
/// event publishes.
async fn snapshot_initial_state(
    state: &ControlState,
    compiled: &CompiledScenarioV2,
) -> Result<BTreeMap<String, CleanupTarget>, String> {
    let mut owned: BTreeMap<String, CleanupTarget> = BTreeMap::new();
    for event in &compiled.events {
        let (proxy, direction, transport) = match &event.action {
            ScenarioAction::SetPlan {
                proxy, direction, ..
            }
            | ScenarioAction::RemoveFault {
                proxy, direction, ..
            } => (proxy.clone(), *direction, ScheduleTransport::Stream),
            ScenarioAction::SetDatagramPlan {
                proxy, direction, ..
            }
            | ScenarioAction::RemoveDatagramFault {
                proxy, direction, ..
            } => (proxy.clone(), *direction, ScheduleTransport::Datagram),
        };
        let key = resource_key(&proxy, direction, transport);
        if owned.contains_key(&key) {
            continue;
        }
        match transport {
            ScheduleTransport::Stream => {
                let Some((upstream_pub, downstream_pub)) = state.snapshot_policies(&proxy).await
                else {
                    return Err(format!("stream proxy {proxy} not found"));
                };
                let (initial_gen, initial_plan) = match direction {
                    Direction::Upstream => (upstream_pub.generation, (*upstream_pub.plan).clone()),
                    Direction::Downstream => {
                        (downstream_pub.generation, (*downstream_pub.plan).clone())
                    }
                };
                owned.insert(
                    key,
                    CleanupTarget {
                        resource: ScheduleResource {
                            proxy,
                            direction,
                            transport,
                        },
                        state: OwnedDirectionState {
                            last_owned_generation: initial_gen,
                            initial_plan: InitialPlan::Stream(initial_plan),
                        },
                    },
                );
            }
            ScheduleTransport::Datagram => {
                let current = state
                    .get_datagram_plan(&proxy, direction)
                    .await
                    .map_err(|_| format!("datagram proxy {proxy} not found"))?;
                owned.insert(
                    key,
                    CleanupTarget {
                        resource: ScheduleResource {
                            proxy,
                            direction,
                            transport,
                        },
                        state: OwnedDirectionState {
                            last_owned_generation: current.1,
                            initial_plan: InitialPlan::Datagram(current.0),
                        },
                    },
                );
            }
        }
    }
    Ok(owned)
}

/// Run cleanup once the terminal outcome is known: `restore-initial`
/// republishes each touched resource's initial plan only while the
/// run still owns its generation; `leave` records `NotRequested`
/// without publishing.
async fn run_cleanup(
    state: &ControlState,
    compiled: &CompiledScenarioV2,
    owned: BTreeMap<String, CleanupTarget>,
) -> CleanupOutcome {
    let mut resources = Vec::with_capacity(owned.len());
    for (_, target) in owned {
        let outcome = match compiled.cleanup {
            super::source::CleanupPolicyV2::Leave => CleanupResourceOutcome::NotRequested,
            super::source::CleanupPolicyV2::RestoreInitial => restore_target(state, &target).await,
        };
        resources.push(CleanupResourceRecord {
            resource: target.resource,
            outcome,
        });
    }
    CleanupOutcome {
        policy: compiled.cleanup,
        resources,
    }
}

/// Attempt a CAS-safe restore of one touched resource. Returns
/// `Conflict` (never overwrites) when the generation moved
/// externally, `Missing` when the proxy is gone.
async fn restore_target(state: &ControlState, target: &CleanupTarget) -> CleanupResourceOutcome {
    let resource = &target.resource;
    let owned_gen = target.state.last_owned_generation;
    match (&target.state.initial_plan, resource.transport) {
        (InitialPlan::Stream(plan), ScheduleTransport::Stream) => {
            let Some((cur_up, cur_down)) = state.snapshot_policies(&resource.proxy).await else {
                return CleanupResourceOutcome::Missing;
            };
            let current_gen = match resource.direction {
                Direction::Upstream => cur_up.generation,
                Direction::Downstream => cur_down.generation,
            };
            if current_gen != owned_gen {
                return CleanupResourceOutcome::Conflict;
            }
            match state
                .publish_direction_expected(
                    &resource.proxy,
                    resource.direction,
                    plan.clone(),
                    0,
                    current_gen,
                )
                .await
            {
                Ok(_) => CleanupResourceOutcome::Restored,
                Err(_) => CleanupResourceOutcome::Conflict,
            }
        }
        (InitialPlan::Datagram(plan), ScheduleTransport::Datagram) => {
            let Ok((_plan, current_gen, _ns)) = state
                .get_datagram_plan(&resource.proxy, resource.direction)
                .await
            else {
                return CleanupResourceOutcome::Missing;
            };
            if current_gen != owned_gen {
                return CleanupResourceOutcome::Conflict;
            }
            match state
                .publish_datagram_plan(
                    &resource.proxy,
                    resource.direction,
                    plan.clone(),
                    0,
                    Some(current_gen),
                )
                .await
            {
                Ok(_) => CleanupResourceOutcome::Restored,
                Err(_) => CleanupResourceOutcome::Conflict,
            }
        }
        _ => CleanupResourceOutcome::Conflict,
    }
}
