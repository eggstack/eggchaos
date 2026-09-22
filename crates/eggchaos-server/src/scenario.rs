use std::time::Duration;

use eggchaos_core::{Direction, FaultId, FaultPlan, FaultSpec};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::{ControlState, EggchaosError};

/// Deterministic bounded scenario document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    /// Scenario schema version.
    pub version: u32,
    /// Explicit run seed recorded in evidence.
    pub seed: u64,
    /// Relative control events.
    pub events: Vec<ScenarioEvent>,
}

/// One monotonic-time scenario event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioEvent {
    /// Milliseconds from scenario start.
    pub at_ms: u64,
    /// Control action.
    pub action: ScenarioAction,
}

/// Supported scenario v1 actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScenarioAction {
    /// Replace one directional plan as a barrier generation.
    SetPlan {
        proxy: String,
        direction: Direction,
        faults: Vec<FaultSpec>,
    },
    /// Remove one fault from a directional plan.
    RemoveFault {
        proxy: String,
        direction: Direction,
        id: String,
    },
}

/// Bounded execution report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioReport {
    /// Scenario seed.
    pub seed: u64,
    /// Number of applied events.
    pub applied: usize,
    /// Final native generation.
    pub generation: u64,
}

/// Apply a scenario through the same control authority as HTTP mutations.
pub async fn apply_scenario(
    state: ControlState,
    scenario: Scenario,
) -> Result<ScenarioReport, EggchaosError> {
    if scenario.version != 1 {
        return Err(EggchaosError::InvalidProxy(
            "unsupported scenario version".into(),
        ));
    }
    if scenario.events.len() > 1024 {
        return Err(EggchaosError::InvalidProxy(
            "too many scenario events".into(),
        ));
    }
    let mut previous = 0;
    let mut applied = 0;
    for event in scenario.events {
        if event.at_ms < previous {
            return Err(EggchaosError::InvalidProxy(
                "scenario events must be ordered".into(),
            ));
        }
        sleep(Duration::from_millis(event.at_ms - previous)).await;
        previous = event.at_ms;
        let action = event.action;
        let proxy_name = match &action {
            ScenarioAction::SetPlan { proxy, .. } | ScenarioAction::RemoveFault { proxy, .. } => {
                proxy.clone()
            }
        };
        let proxy = state
            .get(&proxy_name)
            .await
            .ok_or_else(|| EggchaosError::InvalidProxy("scenario proxy not found".into()))?;
        let (mut upstream, mut downstream) = (
            proxy.upstream_faults.clone(),
            proxy.downstream_faults.clone(),
        );
        match action {
            ScenarioAction::SetPlan {
                direction: Direction::Upstream,
                faults,
                ..
            } => upstream = FaultPlan::new(faults)?,
            ScenarioAction::SetPlan {
                direction: Direction::Downstream,
                faults,
                ..
            } => downstream = FaultPlan::new(faults)?,
            ScenarioAction::RemoveFault {
                direction: Direction::Upstream,
                ref id,
                ..
            } => upstream = upstream.without_fault(id),
            ScenarioAction::RemoveFault {
                direction: Direction::Downstream,
                ref id,
                ..
            } => downstream = downstream.without_fault(id),
        }
        state
            .publish_plans(&proxy_name, upstream, downstream)
            .await
            .map_err(|error| EggchaosError::InvalidProxy(error.to_string()))?;
        applied += 1;
    }
    Ok(ScenarioReport {
        seed: scenario.seed,
        applied,
        generation: state.generation(),
    })
}

/// Keep the ID type in the scenario API documentation without duplicating a
/// second parser for fault IDs.
#[allow(dead_code)]
fn _fault_id(value: String) -> Result<FaultId, eggchaos_core::ValidationError> {
    FaultId::new(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProxySpec;

    #[tokio::test]
    async fn ordered_zero_time_scenario_publishes_one_generation() {
        let proxy = ProxySpec::new(
            "p",
            "127.0.0.1:0".parse().unwrap(),
            "127.0.0.1:1".parse().unwrap(),
        );
        let state = ControlState::new([proxy]);
        let report = apply_scenario(
            state.clone(),
            Scenario {
                version: 1,
                seed: 7,
                events: vec![ScenarioEvent {
                    at_ms: 0,
                    action: ScenarioAction::SetPlan {
                        proxy: "p".into(),
                        direction: Direction::Upstream,
                        faults: Vec::new(),
                    },
                }],
            },
        )
        .await
        .unwrap();
        assert_eq!(report.applied, 1);
        assert_eq!(state.generation(), 2);
    }
}
