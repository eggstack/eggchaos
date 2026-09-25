//! Scenario V2 wire DTO compatibility surface.
//!
//! The DTO authority lives in `eggchaos-protocol`; this module only
//! re-exports it so existing `eggchaos_server::*` imports keep compiling.

pub use eggchaos_protocol::{
    phase_identity_string, to_canonical_json, ScenarioActionDto, ScenarioPhaseV2Dto,
    ScenarioRepeatV2Dto, ScenarioScheduleV2Dto, ScenarioScheduleV2Toml, ScheduleCleanupResourceV2,
    ScheduleCleanupV2, ScheduleCompileV2, ScheduleCompiledEventV2, ScheduleRunEventV2,
    ScheduleRunV2, ScheduleValidateV2,
};
