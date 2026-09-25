//! Shared Scenario V2 schedule driver over [`crate::PolicyTarget`].
//!
//! This driver is the single schedule-execution authority used by both
//! the standalone server (through its `ControlState` adapter) and
//! embedded harnesses (through [`crate::StreamPolicyTarget`] or other
//! consumer-neutral targets). Strict/live ownership, absolute
//! `epoch + offset` deadlines, generation-guarded cleanup, and
//! fail-fast cancellation behave identically on every target.
//!
//! The driver never touches sockets, wall clocks, or payload bytes. It
//! holds no target lock across schedule sleeps: each target call locks
//! only for its own snapshot or publication step.

use std::collections::BTreeMap;
use std::time::Duration;

use eggchaos_core::{derive_schedule_policy_seed, DatagramPlan, Direction, FaultPlan};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::compiler::{CompiledEventV2, CompiledScenarioV2};
use crate::run::{
    CleanupOutcome, CleanupResourceOutcome, CleanupResourceRecord, ScheduleEventResult,
    ScheduleResource, ScheduleRunStatus, ScheduleTransport,
};
use crate::source::{CleanupPolicyV2, IsolationPolicyV2};
use crate::target::{
    action_resource, resource_key, PolicyTarget, PublishReceipt, TargetError, TargetPlan,
};
use crate::ScenarioAction;

/// Async receiver for per-event evidence.
///
/// The driver calls `record` once per successfully applied event, in
/// compiled order, so partial progress stays visible on failure or
/// cancellation. The server appends to its run record; embedded
/// harnesses collect into a caller-owned vector.
pub trait EventSink: Send + 'static {
    /// Record one applied event.
    fn record(
        &mut self,
        event: ScheduleEventResult,
    ) -> impl std::future::Future<Output = ()> + Send;
}

/// Trivial sink collecting events into a caller-owned vector.
impl EventSink for Vec<ScheduleEventResult> {
    async fn record(&mut self, event: ScheduleEventResult) {
        self.push(event);
    }
}

/// Per-resource ownership tracked across one run.
#[derive(Debug, Clone)]
pub struct OwnedResource {
    /// Touched resource identity.
    pub resource: ScheduleResource,
    /// Last generation published by this run (or snapshotted initially).
    pub last_owned_generation: u64,
    /// Pre-run plan used by `restore-initial` cleanup.
    pub initial_plan: TargetPlan,
}

/// Terminal outcome of one driven schedule.
#[derive(Debug, Clone)]
pub struct DriveOutcome {
    /// Terminal run status (`Completed`, `Failed`, or `Cancelled`).
    pub status: ScheduleRunStatus,
    /// Failure detail for `Failed` runs.
    pub failure: Option<String>,
    /// Post-run cleanup outcome.
    pub cleanup: CleanupOutcome,
    /// Number of events successfully applied.
    pub applied: usize,
}

/// Snapshot the initial state for every resource the schedule touches.
///
/// Runs during preparation, before the epoch is captured and before any
/// event publishes. A missing resource or an unsupported transport
/// fails preparation with no side effects.
pub async fn prepare_initial<T: PolicyTarget>(
    target: &T,
    compiled: &CompiledScenarioV2,
) -> Result<BTreeMap<String, OwnedResource>, TargetError> {
    let capabilities = target.capabilities();
    let mut owned: BTreeMap<String, OwnedResource> = BTreeMap::new();
    for event in &compiled.events {
        let resource = action_resource(&event.action);
        let key = resource_key(&resource.proxy, resource.direction, resource.transport);
        if owned.contains_key(&key) {
            continue;
        }
        match resource.transport {
            ScheduleTransport::Stream if !capabilities.stream => {
                return Err(TargetError::unsupported(format!(
                    "stream resource '{}' requires stream support",
                    TargetError::bounded(resource.proxy.clone()),
                )));
            }
            ScheduleTransport::Datagram if !capabilities.datagram => {
                return Err(TargetError::unsupported(format!(
                    "datagram resource '{}' requires datagram support",
                    TargetError::bounded(resource.proxy.clone()),
                )));
            }
            _ => {}
        }
        let snapshot = target
            .snapshot(&resource)
            .await
            .map_err(|error| match error {
                TargetError::MissingResource(_) => match resource.transport {
                    ScheduleTransport::Stream => {
                        TargetError::missing(format!("stream proxy {} not found", resource.proxy))
                    }
                    ScheduleTransport::Datagram => {
                        TargetError::missing(format!("datagram proxy {} not found", resource.proxy))
                    }
                },
                other => other,
            })?;
        owned.insert(
            key,
            OwnedResource {
                resource,
                last_owned_generation: snapshot.generation,
                initial_plan: snapshot.plan,
            },
        );
    }
    Ok(owned)
}

/// Drive one compiled schedule to completion, cancellation, or
/// fail-fast failure.
///
/// `epoch` is the shared monotonic experiment epoch; every deadline is
/// `epoch + compiled_offset` so event-application latency never
/// accumulates into later deadlines. Already-due deadlines skip the
/// sleep. Cancellation wakes promptly from sleeps and runs the
/// selected cleanup.
#[allow(clippy::too_many_arguments)]
pub async fn drive<T: PolicyTarget, S: EventSink>(
    target: &T,
    compiled: &CompiledScenarioV2,
    fingerprint: [u8; 32],
    mut owned: BTreeMap<String, OwnedResource>,
    epoch: Instant,
    token: &CancellationToken,
    sink: &mut S,
) -> DriveOutcome {
    let mut failure: Option<String> = None;
    let mut cancelled = false;
    let mut applied = 0usize;

    for event in compiled.events.iter() {
        // Check cancellation before the deadline wait: an already-due
        // (e.g. 0-offset) event must not publish when cancellation won
        // before the run started. The wait below still wakes promptly
        // for cancellation that arrives mid-sleep.
        if token.is_cancelled() {
            cancelled = true;
            break;
        }
        if wait_for_deadline(token, epoch, event.offset_ns)
            .await
            .is_err()
        {
            cancelled = true;
            break;
        }
        match apply_event(
            target,
            compiled,
            event,
            &mut owned,
            epoch,
            fingerprint,
            sink,
        )
        .await
        {
            Ok(()) => applied += 1,
            Err(reason) => {
                failure = Some(reason);
                break;
            }
        }
    }

    let status = if cancelled {
        ScheduleRunStatus::Cancelled
    } else if failure.is_some() {
        ScheduleRunStatus::Failed
    } else {
        ScheduleRunStatus::Completed
    };
    let cleanup = run_cleanup(target, compiled, owned).await;
    DriveOutcome {
        status,
        failure,
        cleanup,
        applied,
    }
}

/// Wait until `epoch + offset_ns` or cancellation wins.
///
/// Already-due deadlines (`now >= deadline`) skip the sleep so drift
/// never accumulates. The epoch/deadline sum uses `checked_add`: a
/// `u64` offset near the representation limit cannot overflow the
/// monotonic clock into a panic. An unrepresentable deadline waits
/// (pending) until cancellation, which is the only prompt exit from
/// such a wait.
async fn wait_for_deadline(
    token: &CancellationToken,
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
async fn apply_event<T: PolicyTarget, S: EventSink>(
    target: &T,
    compiled: &CompiledScenarioV2,
    event: &CompiledEventV2,
    owned: &mut BTreeMap<String, OwnedResource>,
    epoch: Instant,
    schedule_fingerprint: [u8; 32],
    sink: &mut S,
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
            let resource = ScheduleResource {
                proxy: proxy.clone(),
                direction: *direction,
                transport: ScheduleTransport::Stream,
            };
            let key = resource_key(proxy, *direction, ScheduleTransport::Stream);
            let expected = expected_generation(target, &resource, strict, &key, owned)
                .await
                .ok_or_else(|| format!("stream proxy {proxy} not found"))?;
            let receipt = target
                .publish(&resource, expected, TargetPlan::Stream(plan), namespace)
                .await
                .map_err(|error| format!("set-plan publish failed: {error}"))?;
            update_owned(owned, &key, resource.clone(), receipt);
            let (upstream, downstream) = directional_gens(target, &resource, receipt).await;
            sink.record(ScheduleEventResult {
                compiled_index: event.compiled_index,
                phase: event.phase,
                scheduled_offset_ns: event.offset_ns,
                applied_elapsed_ns,
                late_by_ns,
                action: "set-plan".to_owned(),
                resource,
                upstream_generation: upstream,
                downstream_generation: downstream,
                global_generation: target.global_generation(),
            })
            .await;
            Ok(())
        }
        ScenarioAction::RemoveFault {
            proxy,
            direction,
            id,
        } => {
            let resource = ScheduleResource {
                proxy: proxy.clone(),
                direction: *direction,
                transport: ScheduleTransport::Stream,
            };
            let snapshot = target
                .snapshot(&resource)
                .await
                .map_err(|_| format!("stream proxy {proxy} not found"))?;
            let TargetPlan::Stream(base) = snapshot.plan else {
                return Err(format!("stream proxy {proxy} not found"));
            };
            if base.get(id).is_none() {
                return Err(format!("fault {id} not present on {proxy}"));
            }
            let plan = base.without_fault(id);
            let key = resource_key(proxy, *direction, ScheduleTransport::Stream);
            let expected = expected_generation(target, &resource, strict, &key, owned)
                .await
                .ok_or_else(|| format!("stream proxy {proxy} not found"))?;
            let receipt = target
                .publish(&resource, expected, TargetPlan::Stream(plan), namespace)
                .await
                .map_err(|error| format!("remove-fault publish failed: {error}"))?;
            update_owned(owned, &key, resource.clone(), receipt);
            let (upstream, downstream) = directional_gens(target, &resource, receipt).await;
            sink.record(ScheduleEventResult {
                compiled_index: event.compiled_index,
                phase: event.phase,
                scheduled_offset_ns: event.offset_ns,
                applied_elapsed_ns,
                late_by_ns,
                action: "remove-fault".to_owned(),
                resource,
                upstream_generation: upstream,
                downstream_generation: downstream,
                global_generation: target.global_generation(),
            })
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
            let resource = ScheduleResource {
                proxy: proxy.clone(),
                direction: *direction,
                transport: ScheduleTransport::Datagram,
            };
            let key = resource_key(proxy, *direction, ScheduleTransport::Datagram);
            let expected = expected_generation(target, &resource, strict, &key, owned)
                .await
                .ok_or_else(|| format!("datagram proxy {proxy} not found"))?;
            let receipt = target
                .publish(&resource, expected, TargetPlan::Datagram(plan), namespace)
                .await
                .map_err(|error| format!("set-datagram-plan publish failed: {error}"))?;
            update_owned(owned, &key, resource.clone(), receipt);
            let (upstream, downstream) = match direction {
                Direction::Upstream => (receipt.generation, 0),
                Direction::Downstream => (0, receipt.generation),
            };
            sink.record(ScheduleEventResult {
                compiled_index: event.compiled_index,
                phase: event.phase,
                scheduled_offset_ns: event.offset_ns,
                applied_elapsed_ns,
                late_by_ns,
                action: "set-datagram-plan".to_owned(),
                resource,
                upstream_generation: upstream,
                downstream_generation: downstream,
                global_generation: target.global_generation(),
            })
            .await;
            Ok(())
        }
        ScenarioAction::RemoveDatagramFault {
            proxy,
            direction,
            id,
        } => {
            let resource = ScheduleResource {
                proxy: proxy.clone(),
                direction: *direction,
                transport: ScheduleTransport::Datagram,
            };
            let snapshot = target
                .snapshot(&resource)
                .await
                .map_err(|_| format!("datagram proxy {proxy} not found"))?;
            let TargetPlan::Datagram(current) = snapshot.plan else {
                return Err(format!("datagram proxy {proxy} not found"));
            };
            let mut faults = current.faults().to_vec();
            if !faults.iter().any(|fault| fault.id.as_str() == id) {
                return Err(format!("datagram fault {id} not present on {proxy}"));
            }
            faults.retain(|fault| fault.id.as_str() != id);
            let plan = DatagramPlan::new(faults)
                .map_err(|error| format!("datagram plan invalid: {error}"))?;
            let key = resource_key(proxy, *direction, ScheduleTransport::Datagram);
            let expected = expected_generation(target, &resource, strict, &key, owned)
                .await
                .ok_or_else(|| format!("datagram proxy {proxy} not found"))?;
            let receipt = target
                .publish(&resource, expected, TargetPlan::Datagram(plan), namespace)
                .await
                .map_err(|error| format!("remove-datagram-fault publish failed: {error}"))?;
            update_owned(owned, &key, resource.clone(), receipt);
            let _ = receipt;
            sink.record(ScheduleEventResult {
                compiled_index: event.compiled_index,
                phase: event.phase,
                scheduled_offset_ns: event.offset_ns,
                applied_elapsed_ns,
                late_by_ns,
                action: "remove-datagram-fault".to_owned(),
                resource,
                upstream_generation: 0,
                downstream_generation: 0,
                global_generation: target.global_generation(),
            })
            .await;
            Ok(())
        }
    }
}

/// Resolve the expected generation for a publication.
///
/// Strict mode uses the last generation owned by this run (fail-fast
/// on external moves); live mode re-reads the live generation at
/// fire time so a completed manual update may become the base, while
/// a concurrent move during publication still conflicts.
async fn expected_generation<T: PolicyTarget>(
    target: &T,
    resource: &ScheduleResource,
    strict: bool,
    key: &str,
    owned: &BTreeMap<String, OwnedResource>,
) -> Option<u64> {
    if strict {
        if let Some(entry) = owned.get(key) {
            return Some(entry.last_owned_generation);
        }
    }
    target.current_generation(resource).await.ok()
}

/// Pair the published generation with the untouched direction's
/// latest generation for run evidence.
async fn directional_gens<T: PolicyTarget>(
    target: &T,
    resource: &ScheduleResource,
    receipt: PublishReceipt,
) -> (u64, u64) {
    let sibling = ScheduleResource {
        proxy: resource.proxy.clone(),
        direction: match resource.direction {
            Direction::Upstream => Direction::Downstream,
            Direction::Downstream => Direction::Upstream,
        },
        transport: resource.transport,
    };
    let other = target.current_generation(&sibling).await.unwrap_or(0);
    match resource.direction {
        Direction::Upstream => (receipt.generation, other),
        Direction::Downstream => (other, receipt.generation),
    }
}

fn update_owned(
    owned: &mut BTreeMap<String, OwnedResource>,
    key: &str,
    resource: ScheduleResource,
    receipt: PublishReceipt,
) {
    if let Some(entry) = owned.get_mut(key) {
        entry.last_owned_generation = receipt.generation;
    } else {
        // Defensive: preparation snapshots every touched resource, so
        // this path only triggers if the target set changed mid-run.
        let initial_plan = match resource.transport {
            ScheduleTransport::Stream => TargetPlan::Stream(FaultPlan::empty()),
            ScheduleTransport::Datagram => TargetPlan::Datagram(DatagramPlan::empty()),
        };
        let _ = owned.insert(
            key.to_owned(),
            OwnedResource {
                resource,
                last_owned_generation: receipt.generation,
                initial_plan,
            },
        );
    }
}

/// Run cleanup once the terminal outcome is known: `restore-initial`
/// republishes each touched resource's initial plan only while the
/// run still owns its generation; `leave` records `NotRequested`
/// without publishing.
async fn run_cleanup<T: PolicyTarget>(
    target: &T,
    compiled: &CompiledScenarioV2,
    owned: BTreeMap<String, OwnedResource>,
) -> CleanupOutcome {
    let mut resources = Vec::with_capacity(owned.len());
    for (_, entry) in owned {
        let outcome = match compiled.cleanup {
            CleanupPolicyV2::Leave => CleanupResourceOutcome::NotRequested,
            CleanupPolicyV2::RestoreInitial => restore_target(target, &entry).await,
        };
        resources.push(CleanupResourceRecord {
            resource: entry.resource,
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
/// externally, `Missing` when the resource is gone.
async fn restore_target<T: PolicyTarget>(
    target: &T,
    entry: &OwnedResource,
) -> CleanupResourceOutcome {
    let owned_gen = entry.last_owned_generation;
    let current = match target.current_generation(&entry.resource).await {
        Ok(generation) => generation,
        Err(_) => return CleanupResourceOutcome::Missing,
    };
    if current != owned_gen {
        return CleanupResourceOutcome::Conflict;
    }
    match target
        .publish(&entry.resource, current, entry.initial_plan.clone(), 0)
        .await
    {
        Ok(_) => CleanupResourceOutcome::Restored,
        Err(_) => CleanupResourceOutcome::Conflict,
    }
}
