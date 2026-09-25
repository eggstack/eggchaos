//! Consumer-neutral policy publication target.
//!
//! The Scenario V2 driver publishes through this narrow contract instead
//! of a concrete state store. The standalone server adapts its
//! `ControlState`; embedded harnesses adapt in-process `LivePolicy`
//! pairs (see [`crate::StreamPolicyTarget`]). Both share one driver
//! with identical strict/live and cleanup semantics.
//!
//! The contract exposes no payload bytes and no consumer product model.
//! Unsupported transport families fail explicitly; they are never
//! silently skipped or translated into another transport.

use eggchaos_core::{DatagramPlan, Direction, FaultPlan};

use super::run::{ScheduleResource, ScheduleTransport};

/// Maximum byte length of an experiment resource name or target error
/// detail string.
pub const MAX_TARGET_LABEL_BYTES: usize = 256;

/// Maximum byte length of a stream-target resource name.
pub const MAX_RESOURCE_NAME_BYTES: usize = 128;

/// Which transport families a target implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetCapabilities {
    /// Named directional stream (TCP) policy resources.
    pub stream: bool,
    /// Named directional datagram (UDP) policy resources.
    pub datagram: bool,
}

/// A directional plan snapshot for one resource.
#[derive(Debug, Clone, PartialEq)]
pub enum TargetPlan {
    /// Stream fault plan.
    Stream(FaultPlan),
    /// Datagram fault plan.
    Datagram(DatagramPlan),
}

/// Snapshot of one resource: current plan plus its generation.
#[derive(Debug, Clone)]
pub struct TargetSnapshot {
    /// Current policy generation.
    pub generation: u64,
    /// Current directional plan.
    pub plan: TargetPlan,
}

/// Successful expected-generation publication receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishReceipt {
    /// New policy generation after publication.
    pub generation: u64,
}

/// Bounded, stable target failure categories.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TargetError {
    /// Named resource does not exist on this target.
    #[error("resource not found: {0}")]
    MissingResource(String),
    /// Target does not implement the requested transport family.
    #[error("unsupported capability: {0}")]
    UnsupportedCapability(String),
    /// Base generation moved before publication; nothing published.
    #[error("generation conflict: expected {expected}, found {found}")]
    GenerationConflict {
        /// Generation the publisher based its plan on.
        expected: u64,
        /// Generation actually current when publishing.
        found: u64,
    },
    /// The replacement plan is invalid; nothing published.
    #[error("invalid plan: {0}")]
    Validation(String),
    /// Target-internal failure; nothing published.
    #[error("target error: {0}")]
    Internal(String),
}

impl TargetError {
    /// Build a bounded detail string, truncated on a character boundary.
    pub fn bounded(detail: impl Into<String>) -> String {
        let detail = detail.into();
        if detail.len() <= MAX_TARGET_LABEL_BYTES {
            return detail;
        }
        let mut end = MAX_TARGET_LABEL_BYTES;
        while !detail.is_char_boundary(end) {
            end -= 1;
        }
        detail[..end].to_owned()
    }

    /// Missing-resource error with bounded detail.
    pub fn missing(detail: impl Into<String>) -> Self {
        Self::MissingResource(Self::bounded(detail))
    }

    /// Unsupported-capability error with bounded detail.
    pub fn unsupported(detail: impl Into<String>) -> Self {
        Self::UnsupportedCapability(Self::bounded(detail))
    }

    /// Validation error with bounded detail.
    pub fn invalid(detail: impl Into<String>) -> Self {
        Self::Validation(Self::bounded(detail))
    }

    /// Internal error with bounded detail.
    pub fn internal(detail: impl Into<String>) -> Self {
        Self::Internal(Self::bounded(detail))
    }
}

/// Narrow publication contract the Scenario V2 driver requires.
///
/// Implementations must not hold locks across schedule sleeps; the
/// driver awaits deadlines between calls, so each method should lock
/// only for its own snapshot or publication step.
pub trait PolicyTarget: Send + Sync + 'static {
    /// Which transport families this target implements.
    fn capabilities(&self) -> TargetCapabilities;

    /// Snapshot one resource's current plan and generation.
    fn snapshot(
        &self,
        resource: &ScheduleResource,
    ) -> impl std::future::Future<Output = Result<TargetSnapshot, TargetError>> + Send;

    /// Current generation of one resource.
    fn current_generation(
        &self,
        resource: &ScheduleResource,
    ) -> impl std::future::Future<Output = Result<u64, TargetError>> + Send;

    /// Publish a replacement plan only when the current generation
    /// still equals `expected`. Returns the new generation.
    fn publish(
        &self,
        resource: &ScheduleResource,
        expected: u64,
        plan: TargetPlan,
        seed_namespace: u64,
    ) -> impl std::future::Future<Output = Result<PublishReceipt, TargetError>> + Send;

    /// Target-wide configuration generation for run evidence.
    fn global_generation(&self) -> u64;
}

/// Build the schedule resource identity for an action target.
pub fn resource_key(proxy: &str, direction: Direction, transport: ScheduleTransport) -> String {
    format!("{proxy}|{direction:?}|{transport:?}")
}

/// Resolve the [`ScheduleResource`] for one scenario action.
pub fn action_resource(action: &crate::ScenarioAction) -> ScheduleResource {
    match action {
        crate::ScenarioAction::SetPlan {
            proxy, direction, ..
        }
        | crate::ScenarioAction::RemoveFault {
            proxy, direction, ..
        } => ScheduleResource {
            proxy: proxy.clone(),
            direction: *direction,
            transport: ScheduleTransport::Stream,
        },
        crate::ScenarioAction::SetDatagramPlan {
            proxy, direction, ..
        }
        | crate::ScenarioAction::RemoveDatagramFault {
            proxy, direction, ..
        } => ScheduleResource {
            proxy: proxy.clone(),
            direction: *direction,
            transport: ScheduleTransport::Datagram,
        },
    }
}
