//! Unified scenario run lifecycle registry.
//!
//! M050 consolidates the duplicated per-family run/record/token/admission
//! bookkeeping into a single internal authority while preserving the
//! Scenario V1 and Scenario V2 semantic authorities and the existing
//! per-family `MAX_SCENARIO_RUNS = 32` capacity.
//!
//! Public `ControlState` methods stay unchanged in name and signature;
//! each is now a thin wrapper over the registry. The single
//! `next_run_id` allocator and single `JoinSet` supervisor remain the
//! only run-ID and task authorities.

use std::collections::{BTreeMap, HashMap};

use tokio_util::sync::CancellationToken;

use crate::scenario::{ScenarioRunRecord, ScenarioRunStatus};
use crate::scenario_v2::{ScenarioScheduleRunRecord, ScheduleRunStatus};

/// Family tag identifying which scenario language a run record belongs
/// to. The registry keeps the V1 and V2 record shapes distinct because
/// their types are semantically different; the family tag exists only
/// for shared admission/pruning helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScenarioFamily {
    /// Scenario V1 event-driven run.
    V1,
    /// Scenario V2 schedule run.
    V2,
}

/// Per-family capacity for active + retained runs. The consolidated
/// registry preserves the pre-M050 effective capability: 32 V1 runs
/// AND 32 V2 runs concurrently. The two families share admission
/// logic but never collapse into a single global quota.
pub(crate) const SCENARIO_FAMILY_CAPACITY: usize = 32;

/// Internal registry owning run records and cancellation tokens for
/// both scenario families. The `next_run_id` allocator and the
/// scenario `JoinSet` supervisor stay where they are; this registry
/// owns the *bookkeeping* the previous per-family maps used to do.
pub(crate) struct ScenarioRegistry {
    v1_runs: BTreeMap<u64, ScenarioRunRecord>,
    v2_runs: BTreeMap<u64, ScenarioScheduleRunRecord>,
    v1_tokens: HashMap<u64, CancellationToken>,
    v2_tokens: HashMap<u64, CancellationToken>,
}

/// Outcome of an admission attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmitError {
    /// The target family already holds `SCENARIO_FAMILY_CAPACITY`
    /// active runs.
    CapacityExceeded,
}

impl ScenarioRegistry {
    pub(crate) fn new() -> Self {
        Self {
            v1_runs: BTreeMap::new(),
            v2_runs: BTreeMap::new(),
            v1_tokens: HashMap::new(),
            v2_tokens: HashMap::new(),
        }
    }

    /// Try to admit a new V1 run record. May prune the oldest finished
    /// V1 entry to make room when the family is at retention capacity.
    pub(crate) fn admit_v1(
        &mut self,
        run_id: u64,
        record: ScenarioRunRecord,
    ) -> Result<(), AdmitError> {
        let active = self.active_v1_count();
        let retained = self.v1_runs.len();
        if active >= SCENARIO_FAMILY_CAPACITY {
            return Err(AdmitError::CapacityExceeded);
        }
        if retained >= SCENARIO_FAMILY_CAPACITY {
            if let Some(oldest) = self.first_oldest_finished_v1() {
                self.remove_run(ScenarioFamily::V1, oldest);
            }
        }
        self.v1_runs.insert(run_id, record);
        Ok(())
    }

    /// Try to admit a new V2 run record. May prune the oldest finished
    /// V2 entry to make room when the family is at retention capacity.
    pub(crate) fn admit_v2(
        &mut self,
        run_id: u64,
        record: ScenarioScheduleRunRecord,
    ) -> Result<(), AdmitError> {
        let active = self.active_v2_count();
        let retained = self.v2_runs.len();
        if active >= SCENARIO_FAMILY_CAPACITY {
            return Err(AdmitError::CapacityExceeded);
        }
        if retained >= SCENARIO_FAMILY_CAPACITY {
            if let Some(oldest) = self.first_oldest_finished_v2() {
                self.remove_run(ScenarioFamily::V2, oldest);
            }
        }
        self.v2_runs.insert(run_id, record);
        Ok(())
    }

    /// Admit a V1 run and install its cancellation token atomically.
    /// A single `&mut self` call, so callers holding the registry lock
    /// cannot lose a cancel racing between separate admit/insert locks.
    pub(crate) fn admit_v1_with_token(
        &mut self,
        run_id: u64,
        record: ScenarioRunRecord,
        token: CancellationToken,
    ) -> Result<(), AdmitError> {
        self.admit_v1(run_id, record)?;
        self.insert_token_v1(run_id, token);
        Ok(())
    }

    /// Admit a V2 run and install its cancellation token atomically.
    pub(crate) fn admit_v2_with_token(
        &mut self,
        run_id: u64,
        record: ScenarioScheduleRunRecord,
        token: CancellationToken,
    ) -> Result<(), AdmitError> {
        self.admit_v2(run_id, record)?;
        self.insert_token_v2(run_id, token);
        Ok(())
    }

    fn first_oldest_finished_v1(&self) -> Option<u64> {
        self.v1_runs
            .iter()
            .find(|(_, record)| {
                !matches!(
                    record.status,
                    ScenarioRunStatus::Pending
                        | ScenarioRunStatus::Running
                        | ScenarioRunStatus::Cancelling
                )
            })
            .map(|(id, _)| *id)
    }

    fn first_oldest_finished_v2(&self) -> Option<u64> {
        self.v2_runs
            .iter()
            .find(|(_, record)| {
                !matches!(
                    record.status,
                    ScheduleRunStatus::Pending
                        | ScheduleRunStatus::Running
                        | ScheduleRunStatus::Cancelling
                )
            })
            .map(|(id, _)| *id)
    }

    fn remove_run(&mut self, family: ScenarioFamily, run_id: u64) {
        match family {
            ScenarioFamily::V1 => {
                self.v1_runs.remove(&run_id);
                self.v1_tokens.remove(&run_id);
            }
            ScenarioFamily::V2 => {
                self.v2_runs.remove(&run_id);
                self.v2_tokens.remove(&run_id);
            }
        }
    }

    /// Number of V1 runs whose status is `Pending`, `Running`, or
    /// `Cancelling`. Active-run count is what gates admission.
    pub(crate) fn active_v1_count(&self) -> usize {
        self.v1_runs
            .values()
            .filter(|record| {
                matches!(
                    record.status,
                    ScenarioRunStatus::Pending
                        | ScenarioRunStatus::Running
                        | ScenarioRunStatus::Cancelling
                )
            })
            .count()
    }

    /// Number of V2 runs whose status is `Pending`, `Running`, or
    /// `Cancelling`. Active-run count is what gates admission.
    pub(crate) fn active_v2_count(&self) -> usize {
        self.v2_runs
            .values()
            .filter(|record| {
                matches!(
                    record.status,
                    ScheduleRunStatus::Pending
                        | ScheduleRunStatus::Running
                        | ScheduleRunStatus::Cancelling
                )
            })
            .count()
    }

    /// Fetch a cloned V1 record.
    pub(crate) fn get_v1(&self, run_id: u64) -> Option<ScenarioRunRecord> {
        self.v1_runs.get(&run_id).cloned()
    }

    /// Fetch a cloned V2 record.
    pub(crate) fn get_v2(&self, run_id: u64) -> Option<ScenarioScheduleRunRecord> {
        self.v2_runs.get(&run_id).cloned()
    }

    /// Apply a typed mutation to a V1 record. Returns false when the run
    /// is unknown (pruned or never-existed) instead of a silent no-op.
    pub(crate) fn update_v1(
        &mut self,
        run_id: u64,
        update: impl FnOnce(&mut ScenarioRunRecord),
    ) -> bool {
        if let Some(record) = self.v1_runs.get_mut(&run_id) {
            update(record);
            true
        } else {
            false
        }
    }

    /// Apply a typed mutation to a V2 record. Returns false when the run
    /// is unknown (pruned or never-existed) instead of a silent no-op.
    pub(crate) fn update_v2(
        &mut self,
        run_id: u64,
        update: impl FnOnce(&mut ScenarioScheduleRunRecord),
    ) -> bool {
        if let Some(record) = self.v2_runs.get_mut(&run_id) {
            update(record);
            true
        } else {
            false
        }
    }

    /// Insert a V1 cancellation token.
    pub(crate) fn insert_token_v1(&mut self, run_id: u64, token: CancellationToken) {
        self.v1_tokens.insert(run_id, token);
    }

    /// Insert a V2 cancellation token.
    pub(crate) fn insert_token_v2(&mut self, run_id: u64, token: CancellationToken) {
        self.v2_tokens.insert(run_id, token);
    }

    /// Take (and remove) the V1 token, leaving no token behind.
    pub(crate) fn take_token_v1(&mut self, run_id: u64) -> Option<CancellationToken> {
        self.v1_tokens.remove(&run_id)
    }

    /// Take (and remove) the V2 token, leaving no token behind.
    pub(crate) fn take_token_v2(&mut self, run_id: u64) -> Option<CancellationToken> {
        self.v2_tokens.remove(&run_id)
    }

    /// Remove the V1 token without returning it.
    pub(crate) fn drop_token_v1(&mut self, run_id: u64) {
        self.v1_tokens.remove(&run_id);
    }

    /// Remove the V2 token without returning it.
    pub(crate) fn drop_token_v2(&mut self, run_id: u64) {
        self.v2_tokens.remove(&run_id);
    }

    /// Cancel and clear every active token for both families. Shutdown
    /// cascades to all scenario runs through these tokens. Records stay
    /// in the registry so post-shutdown observers still see the final
    /// status (the existing driver updates status before exiting).
    pub(crate) fn cancel_all(&mut self) {
        for token in self.v1_tokens.values() {
            token.cancel();
        }
        for token in self.v2_tokens.values() {
            token.cancel();
        }
        self.v1_tokens.clear();
        self.v2_tokens.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{ScenarioRunRecord, ScenarioRunStatus};
    use crate::scenario_v2::ScenarioScheduleRunRecord;
    use eggchaos_experiment::{CleanupPolicyV2, IsolationPolicyV2, ScheduleRunStatus};

    fn v1_record(id: u64, status: ScenarioRunStatus) -> ScenarioRunRecord {
        ScenarioRunRecord {
            run_id: id,
            seed: 0,
            status,
            applied: 0,
            failure: None,
            trail: Vec::new(),
        }
    }

    fn v2_record(id: u64, status: ScheduleRunStatus) -> ScenarioScheduleRunRecord {
        ScenarioScheduleRunRecord {
            run_id: id,
            seed: 0,
            execution_key: 0,
            schedule_fingerprint: [0u8; 32],
            compiler_semantics_version: 1,
            isolation: IsolationPolicyV2::Strict,
            cleanup_policy: CleanupPolicyV2::RestoreInitial,
            status,
            applied: 0,
            failure: None,
            events: Vec::new(),
            cleanup: None,
        }
    }

    #[test]
    fn admit_v1_then_active_count_grows() {
        let mut reg = ScenarioRegistry::new();
        assert!(reg
            .admit_v1(1, v1_record(1, ScenarioRunStatus::Pending))
            .is_ok());
        assert_eq!(reg.active_v1_count(), 1);
    }

    #[test]
    fn active_capacity_is_per_family() {
        let mut reg = ScenarioRegistry::new();
        // Fill V1 to capacity.
        for id in 1..=SCENARIO_FAMILY_CAPACITY as u64 {
            assert!(
                reg.admit_v1(id, v1_record(id, ScenarioRunStatus::Pending))
                    .is_ok(),
                "id {id}"
            );
        }
        assert_eq!(
            reg.admit_v1(33, v1_record(33, ScenarioRunStatus::Pending))
                .unwrap_err(),
            AdmitError::CapacityExceeded
        );
        // V2 still admits freely.
        assert!(reg
            .admit_v2(1, v2_record(1, ScheduleRunStatus::Pending))
            .is_ok());
        // And V1 can admit if a slot opens up.
        reg.update_v1(1, |r| r.status = ScenarioRunStatus::Completed);
        assert_eq!(reg.active_v1_count(), SCENARIO_FAMILY_CAPACITY - 1);
        assert!(reg
            .admit_v1(34, v1_record(34, ScenarioRunStatus::Pending))
            .is_ok());
    }

    #[test]
    fn mixed_family_full_capacity_is_supported() {
        let mut reg = ScenarioRegistry::new();
        for id in 1..=SCENARIO_FAMILY_CAPACITY as u64 {
            reg.admit_v1(id, v1_record(id, ScenarioRunStatus::Pending))
                .unwrap();
            reg.admit_v2(id, v2_record(id, ScheduleRunStatus::Pending))
                .unwrap();
        }
        assert_eq!(reg.active_v1_count(), SCENARIO_FAMILY_CAPACITY);
        assert_eq!(reg.active_v2_count(), SCENARIO_FAMILY_CAPACITY);
        assert!(reg
            .admit_v1(99, v1_record(99, ScenarioRunStatus::Pending))
            .is_err());
        assert!(reg
            .admit_v2(99, v2_record(99, ScheduleRunStatus::Pending))
            .is_err());
    }

    #[test]
    fn retention_pruning_only_drops_finished_runs_in_the_relevant_family() {
        let mut reg = ScenarioRegistry::new();
        for id in 1..=SCENARIO_FAMILY_CAPACITY as u64 {
            reg.admit_v1(id, v1_record(id, ScenarioRunStatus::Pending))
                .unwrap();
            reg.admit_v2(id, v2_record(id, ScheduleRunStatus::Pending))
                .unwrap();
        }
        // Mark one V1 entry as finished and try to admit a fresh V1.
        reg.update_v1(1, |r| r.status = ScenarioRunStatus::Completed);
        assert!(reg
            .admit_v1(100, v1_record(100, ScenarioRunStatus::Pending))
            .is_ok());
        assert!(reg.get_v1(1).is_none());
        // V2 records are untouched: every original V2 entry is still present.
        for id in 1..=SCENARIO_FAMILY_CAPACITY as u64 {
            assert!(reg.get_v2(id).is_some());
        }
    }

    #[test]
    fn take_token_removes_and_returns() {
        let mut reg = ScenarioRegistry::new();
        reg.admit_v1(7, v1_record(7, ScenarioRunStatus::Pending))
            .unwrap();
        let token = CancellationToken::new();
        reg.insert_token_v1(7, token.clone());
        assert!(reg.take_token_v1(7).is_some());
        assert!(reg.take_token_v1(7).is_none());
    }

    #[test]
    fn cancel_all_signals_both_families() {
        let mut reg = ScenarioRegistry::new();
        reg.admit_v1(1, v1_record(1, ScenarioRunStatus::Running))
            .unwrap();
        reg.admit_v2(1, v2_record(1, ScheduleRunStatus::Running))
            .unwrap();
        let t1 = CancellationToken::new();
        let t2 = CancellationToken::new();
        reg.insert_token_v1(1, t1.clone());
        reg.insert_token_v2(1, t2.clone());
        reg.cancel_all();
        assert!(t1.is_cancelled());
        assert!(t2.is_cancelled());
        assert!(reg.take_token_v1(1).is_none());
        assert!(reg.take_token_v2(1).is_none());
    }

    #[test]
    fn unknown_run_lookups_return_none_and_no_op_updates() {
        let mut reg = ScenarioRegistry::new();
        assert!(reg.get_v1(99).is_none());
        assert!(reg.get_v2(99).is_none());
        // Update on missing ID is a silent no-op, mirroring the
        // pre-M050 `update_scenario_run` / `update_schedule_v2_run` contract.
        reg.update_v1(99, |r| r.applied += 1);
        reg.update_v2(99, |r| r.applied += 1);
    }
}
