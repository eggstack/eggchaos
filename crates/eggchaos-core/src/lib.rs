//! Protocol-neutral deterministic byte-stream fault injection.
#![deny(unsafe_code)]

mod engine;
mod plan;
mod policy;
mod rng;
mod stream;

pub use engine::{
    DirectionEngine, EngineEvidence, TerminationHandle, TerminationInfo, TerminationRequest,
};
pub use plan::{
    BandwidthConfig, BlackholeConfig, DisconnectConfig, FaultId, FaultKind, FaultPlan, FaultSpec,
    LatencyConfig, LimitDataConfig, Probability, RngVersion, SliceConfig, SlowCloseConfig,
    ValidationError, FAULT_TYPE_NAMES,
};
pub use policy::{LivePolicy, PolicyConflict, PublishError, PublishedPolicy};
pub use rng::{derive_policy_seed, derive_seed, DeterministicRng, RngEvidence};
pub use stream::{
    ActiveFault, BidirectionalChaosStream, ChaosStream, DirectionSummary, EngineError,
    StreamEvidence, MAX_EVIDENCE_FAULTS,
};

/// The direction in which application bytes travel through a proxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Client to target.
    Upstream,
    /// Target to client.
    Downstream,
}

impl Direction {
    /// Return a stable wire/config spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Upstream => "upstream",
            Self::Downstream => "downstream",
        }
    }
}

/// Transport capabilities that an embedding runtime may expose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StreamCapabilities {
    /// Whether graceful shutdown is available.
    pub graceful_shutdown: bool,
    /// Whether an independent half-close is available.
    pub half_close: bool,
    /// Whether a true hard reset is available.
    pub hard_reset: bool,
}
