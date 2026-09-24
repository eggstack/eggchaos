//! Wire DTOs for ScenarioScheduleV2 and TOML/JSON parsing.
//!
//! Scenario v2 is a separate, additive wire family from Scenario v1 —
//! v1 wire format remains unchanged, v2 carries the same scenario
//! actions under a richer source/compiler envelope. This module owns:
//!
//! - the explicit `ScenarioScheduleV2` wire DTOs (version-aware JSON),
//! - a TOML authoring representation (human duration strings parsed at
//!   the TOML boundary into integer nanoseconds),
//! - the conversion from wire DTOs to the internal model (which is the
//!   same model the M027 runtime consumes through `ControlState`).
//!
//! The CLI sends the parsed semantic document to the server-side
//! validation/compile authority; it does not own a second expansion or
//! fingerprint implementation.

use serde::{Deserialize, Serialize};

use crate::scenario::ScenarioAction;
use crate::scenario_v2::{
    CleanupPolicyV2, IsolationPolicyV2, ScenarioScheduleV2, SchedulePhaseV2, ScheduleRepeatV2,
    SCHEDULE_SCHEMA_VERSION,
};

/// Wire DTO mirroring the internal model. JSON forms of v2 use this;
/// TOML authoring forms follow the same shape modulo duration strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioScheduleV2Dto {
    pub version: u32,
    pub seed: u64,
    pub execution_key: u64,
    #[serde(default)]
    pub isolation: IsolationPolicyV2,
    #[serde(default)]
    pub cleanup: CleanupPolicyV2,
    #[serde(default)]
    pub phases: Vec<ScenarioPhaseV2Dto>,
    #[serde(default)]
    pub repeat: Option<ScenarioRepeatV2Dto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioPhaseV2Dto {
    #[serde(default)]
    pub name: Option<String>,
    pub duration_ns: u64,
    pub actions: Vec<ScenarioActionDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioRepeatV2Dto {
    pub count: u32,
    pub phases: Vec<ScenarioPhaseV2Dto>,
}

/// Wire-only mirror of [`ScenarioAction`] using the same kebab-case
/// tags as `ScenarioActionV1`. Conversions land on the internal enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ScenarioActionDto {
    #[serde(rename = "set-plan")]
    SetPlan {
        proxy: String,
        direction: eggchaos_core::Direction,
        faults: Vec<crate::ScenarioFaultV1>,
    },
    #[serde(rename = "remove-fault")]
    RemoveFault {
        proxy: String,
        direction: eggchaos_core::Direction,
        id: String,
    },
    #[serde(rename = "set-datagram-plan")]
    SetDatagramPlan {
        proxy: String,
        direction: eggchaos_core::Direction,
        faults: Vec<crate::DatagramFaultSpecV1>,
    },
    #[serde(rename = "remove-datagram-fault")]
    RemoveDatagramFault {
        proxy: String,
        direction: eggchaos_core::Direction,
        id: String,
    },
}

impl ScenarioActionDto {
    /// Convert this wire DTO to the internal [`ScenarioAction`].
    pub fn into_internal(self) -> Result<ScenarioAction, String> {
        match self {
            Self::SetPlan {
                proxy,
                direction,
                faults,
            } => {
                let mut converted = Vec::with_capacity(faults.len());
                for fault in faults {
                    converted.push(eggchaos_core::FaultSpec {
                        id: eggchaos_core::FaultId::new(fault.id)
                            .map_err(|error| error.to_string())?,
                        probability: eggchaos_core::Probability::new(fault.probability)
                            .map_err(|error| error.to_string())?,
                        kind: fault.kind.into_runtime()?,
                    });
                }
                eggchaos_core::FaultPlan::new(converted.clone())
                    .map_err(|error| error.to_string())?;
                Ok(ScenarioAction::SetPlan {
                    proxy,
                    direction,
                    faults: converted,
                })
            }
            Self::RemoveFault {
                proxy,
                direction,
                id,
            } => {
                eggchaos_core::FaultId::new(id.clone()).map_err(|error| error.to_string())?;
                Ok(ScenarioAction::RemoveFault {
                    proxy,
                    direction,
                    id,
                })
            }
            Self::SetDatagramPlan {
                proxy,
                direction,
                faults,
            } => {
                let converted = faults
                    .into_iter()
                    .map(crate::DatagramFaultSpecV1::into_runtime)
                    .collect::<Result<Vec<_>, _>>()?;
                eggchaos_core::DatagramPlan::new(converted.clone())
                    .map_err(|error| error.to_string())?;
                Ok(ScenarioAction::SetDatagramPlan {
                    proxy,
                    direction,
                    faults: converted,
                })
            }
            Self::RemoveDatagramFault {
                proxy,
                direction,
                id,
            } => {
                eggchaos_core::FaultId::new(id.clone()).map_err(|error| error.to_string())?;
                Ok(ScenarioAction::RemoveDatagramFault {
                    proxy,
                    direction,
                    id,
                })
            }
        }
    }
}

impl From<ScenarioScheduleV2> for ScenarioScheduleV2Dto {
    fn from(source: ScenarioScheduleV2) -> Self {
        let phases = source
            .phases
            .into_iter()
            .map(ScenarioPhaseV2Dto::from)
            .collect();
        let repeat = source.repeat.map(ScenarioRepeatV2Dto::from);
        Self {
            version: source.version,
            seed: source.seed,
            execution_key: source.execution_key,
            isolation: source.isolation,
            cleanup: source.cleanup,
            phases,
            repeat,
        }
    }
}

impl From<SchedulePhaseV2> for ScenarioPhaseV2Dto {
    fn from(phase: SchedulePhaseV2) -> Self {
        let actions = phase
            .actions
            .into_iter()
            .map(ScenarioActionDto::from)
            .collect();
        Self {
            name: phase.name,
            duration_ns: phase.duration_ns,
            actions,
        }
    }
}

impl From<ScheduleRepeatV2> for ScenarioRepeatV2Dto {
    fn from(repeat: ScheduleRepeatV2) -> Self {
        Self {
            count: repeat.count,
            phases: repeat
                .phases
                .into_iter()
                .map(ScenarioPhaseV2Dto::from)
                .collect(),
        }
    }
}

impl From<crate::scenario::ScenarioAction> for ScenarioActionDto {
    fn from(action: crate::scenario::ScenarioAction) -> Self {
        match action {
            crate::scenario::ScenarioAction::SetPlan {
                proxy,
                direction,
                faults,
            } => ScenarioActionDto::SetPlan {
                proxy,
                direction,
                faults: faults
                    .into_iter()
                    .map(|fault| crate::ScenarioFaultV1 {
                        id: fault.id.to_string(),
                        probability: fault.probability.get(),
                        kind: crate::FaultKindV1::from_runtime(fault.kind),
                    })
                    .collect(),
            },
            crate::scenario::ScenarioAction::RemoveFault {
                proxy,
                direction,
                id,
            } => ScenarioActionDto::RemoveFault {
                proxy,
                direction,
                id,
            },
            crate::scenario::ScenarioAction::SetDatagramPlan {
                proxy,
                direction,
                faults,
            } => ScenarioActionDto::SetDatagramPlan {
                proxy,
                direction,
                faults: faults
                    .into_iter()
                    .map(crate::DatagramFaultSpecV1::from)
                    .collect(),
            },
            crate::scenario::ScenarioAction::RemoveDatagramFault {
                proxy,
                direction,
                id,
            } => ScenarioActionDto::RemoveDatagramFault {
                proxy,
                direction,
                id,
            },
        }
    }
}

impl ScenarioScheduleV2Dto {
    /// Parse a v2 schedule from its JSON wire form and convert to the
    /// internal `ScenarioScheduleV2`. Any structural/identity error is
    /// surfaced as a bounded `String` so it cannot leak source bytes
    /// beyond the validator message.
    pub fn from_json_str(input: &str) -> Result<ScenarioScheduleV2, String> {
        let dto: Self = serde_json::from_str(input).map_err(|error| error.to_string())?;
        dto.into_internal()
    }

    /// Convert the wire DTO to the internal source model.
    pub fn into_internal(self) -> Result<ScenarioScheduleV2, String> {
        if self.version != SCHEDULE_SCHEMA_VERSION {
            return Err(format!("unsupported schedule version: {}", self.version));
        }
        let phases = self
            .phases
            .into_iter()
            .map(ScenarioPhaseV2Dto::into_internal)
            .collect::<Result<Vec<_>, _>>()?;
        let repeat = self
            .repeat
            .map(ScenarioRepeatV2Dto::into_internal)
            .transpose()?;
        Ok(ScenarioScheduleV2 {
            version: self.version,
            seed: self.seed,
            execution_key: self.execution_key,
            isolation: self.isolation,
            cleanup: self.cleanup,
            phases,
            repeat,
        })
    }
}

impl ScenarioPhaseV2Dto {
    fn into_internal(self) -> Result<SchedulePhaseV2, String> {
        let actions = self
            .actions
            .into_iter()
            .map(ScenarioActionDto::into_internal)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SchedulePhaseV2 {
            name: self.name,
            duration_ns: self.duration_ns,
            actions,
        })
    }
}

impl ScenarioRepeatV2Dto {
    fn into_internal(self) -> Result<ScheduleRepeatV2, String> {
        let phases = self
            .phases
            .into_iter()
            .map(ScenarioPhaseV2Dto::into_internal)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ScheduleRepeatV2 {
            count: self.count,
            phases,
        })
    }
}

/// TOML authoring DTO. Durations are accepted as integer-nanosecond
/// values to keep the boundary deterministic; a future human-duration
/// extension is a separate plan that must round-trip to this integer
/// form. Keeping the boundary deterministic avoids silent
/// rounding/overflow at the parser edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioScheduleV2Toml {
    pub version: u32,
    pub seed: u64,
    pub execution_key: u64,
    #[serde(default)]
    pub isolation: IsolationPolicyV2,
    #[serde(default)]
    pub cleanup: CleanupPolicyV2,
    #[serde(default)]
    pub phases: Vec<ScenarioPhaseV2Toml>,
    #[serde(default)]
    pub repeat: Option<ScenarioRepeatV2Toml>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioPhaseV2Toml {
    #[serde(default)]
    pub name: Option<String>,
    pub duration_ns: u64,
    pub actions: Vec<ScenarioActionDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioRepeatV2Toml {
    pub count: u32,
    pub phases: Vec<ScenarioPhaseV2Toml>,
}

impl ScenarioScheduleV2Toml {
    /// Parse a v2 schedule from TOML and convert to the internal
    /// source model.
    pub fn from_toml_str(input: &str) -> Result<ScenarioScheduleV2, String> {
        let dto: Self = toml::from_str(input).map_err(|error| error.to_string())?;
        dto.into_internal()
    }

    /// Convert the TOML DTO to the internal source model.
    pub fn into_internal(self) -> Result<ScenarioScheduleV2, String> {
        if self.version != SCHEDULE_SCHEMA_VERSION {
            return Err(format!("unsupported schedule version: {}", self.version));
        }
        let phases = self
            .phases
            .into_iter()
            .map(|phase| {
                let actions = phase
                    .actions
                    .into_iter()
                    .map(ScenarioActionDto::into_internal)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok::<_, String>(SchedulePhaseV2 {
                    name: phase.name,
                    duration_ns: phase.duration_ns,
                    actions,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let repeat = match self.repeat {
            Some(repeat) => {
                let phases = repeat
                    .phases
                    .into_iter()
                    .map(|phase| {
                        let actions = phase
                            .actions
                            .into_iter()
                            .map(ScenarioActionDto::into_internal)
                            .collect::<Result<Vec<_>, _>>()?;
                        Ok::<_, String>(SchedulePhaseV2 {
                            name: phase.name,
                            duration_ns: phase.duration_ns,
                            actions,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Some(ScheduleRepeatV2 {
                    count: repeat.count,
                    phases,
                })
            }
            None => None,
        };
        Ok(ScenarioScheduleV2 {
            version: self.version,
            seed: self.seed,
            execution_key: self.execution_key,
            isolation: self.isolation,
            cleanup: self.cleanup,
            phases,
            repeat,
        })
    }
}

/// Convenience: compact canonical JSON for a v2 source after parsing
/// round-trips through the explicit wire DTO (avoiding the internal
/// enum's missing serde tag).
pub fn to_canonical_json(source: &ScenarioScheduleV2) -> Result<String, String> {
    serde_json::to_string(&ScenarioScheduleV2Dto::from(source.clone()))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dto_parses_minimal_json_and_round_trips_to_compiler() {
        let json = r#"{
            "version": 2,
            "seed": 7,
            "execution_key": 11,
            "isolation": "strict",
            "cleanup": "restore-initial",
            "phases": [
                {
                    "name": "warmup",
                    "duration_ns": 1000000000,
                    "actions": [
                        {
                            "type": "set-plan",
                            "proxy": "cache",
                            "direction": "downstream",
                            "faults": [
                                {
                                    "id": "delay",
                                    "probability": 1.0,
                                    "kind": {
                                        "type": "latency",
                                        "delay_ns": 5000000,
                                        "jitter_ns": 0,
                                        "max_buffer_bytes": 1024
                                    }
                                }
                            ]
                        }
                    ]
                }
            ]
        }"#;
        let source = ScenarioScheduleV2Dto::from_json_str(json).expect("parse");
        let compiled = crate::scenario_v2::compile_schedule(&source).expect("compile");
        assert_eq!(compiled.events.len(), 1);
    }

    #[test]
    fn toml_parses_to_the_same_internal_source_as_json() {
        let toml_text = r#"
            version = 2
            seed = 7
            execution_key = 11
            isolation = "strict"
            cleanup = "restore-initial"

            [[phases]]
            name = "warmup"
            duration_ns = 1000000000

            [[phases.actions]]
            type = "set-plan"
            proxy = "cache"
            direction = "downstream"

            [[phases.actions.faults]]
            id = "delay"
            probability = 1.0

            [phases.actions.faults.kind]
            type = "latency"
            delay_ns = 5000000
            jitter_ns = 0
            max_buffer_bytes = 1024
        "#;
        let toml_source = ScenarioScheduleV2Toml::from_toml_str(toml_text).expect("toml parse");
        let toml_compiled =
            crate::scenario_v2::compile_schedule(&toml_source).expect("toml compile");
        let from_json = r#"{
            "version": 2, "seed": 7, "execution_key": 11,
            "isolation": "strict", "cleanup": "restore-initial",
            "phases": [{"name":"warmup","duration_ns":1000000000,
                "actions":[{"type":"set-plan","proxy":"cache","direction":"downstream",
                    "faults":[{"id":"delay","probability":1.0,
                    "kind":{"type":"latency","delay_ns":5000000,"jitter_ns":0,"max_buffer_bytes":1024}}]}]}]}"#;
        let json_source = ScenarioScheduleV2Dto::from_json_str(from_json).expect("json parse");
        let json_compiled =
            crate::scenario_v2::compile_schedule(&json_source).expect("json compile");
        assert_eq!(json_compiled, toml_compiled);
        assert_eq!(
            crate::scenario_v2::compiled_fingerprint(&json_compiled),
            crate::scenario_v2::compiled_fingerprint(&toml_compiled),
            "JSON and TOML must produce the same fingerprint"
        );
    }

    #[test]
    fn dto_rejects_unknown_fields() {
        let json = r#"{
            "version": 2, "seed": 1, "execution_key": 1,
            "phases": [], "phantom_field": true
        }"#;
        let err = ScenarioScheduleV2Dto::from_json_str(json).expect_err("unknown");
        assert!(!err.is_empty());
    }

    #[test]
    fn dto_rejects_unsupported_version() {
        let json = r#"{"version":1,"seed":1,"execution_key":1,"phases":[]}"#;
        let err = ScenarioScheduleV2Dto::from_json_str(json).expect_err("version");
        assert!(err.contains("unsupported schedule version"));
    }

    #[test]
    fn dto_rejects_invalid_fault_identity() {
        let json = r#"{
            "version": 2, "seed": 1, "execution_key": 1,
            "phases": [{"duration_ns": 0, "actions": [
                {"type": "remove-fault", "proxy": "p", "direction": "upstream", "id": ""}
            ]}]
        }"#;
        let err = ScenarioScheduleV2Dto::from_json_str(json).expect_err("invalid id");
        assert!(!err.is_empty());
    }
}
