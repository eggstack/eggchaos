use std::{fmt, num::NonZeroU64, time::Duration};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The deterministic RNG contract used by eggchaos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RngVersion {
    /// SplitMix64 with the eggchaos v1 domain-separation encoding.
    #[default]
    V1,
}

/// A validated opaque fault identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FaultId(String);

impl FaultId {
    /// Construct an identifier, rejecting empty or oversized values.
    pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
        let value = value.into();
        if value.is_empty() || value.len() > 128 {
            return Err(ValidationError::InvalidFaultId);
        }
        Ok(Self(value))
    }

    /// Borrow the identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FaultId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A probability in the closed interval [0, 1].
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Probability(f64);

impl Probability {
    /// Construct a probability after checking finiteness and bounds.
    pub fn new(value: f64) -> Result<Self, ValidationError> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(ValidationError::ProbabilityOutOfRange)
        }
    }
    /// Return the floating representation.
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl Default for Probability {
    fn default() -> Self {
        Self(1.0)
    }
}

/// Latency configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencyConfig {
    /// Base delay.
    pub delay: Duration,
    /// Symmetric jitter, clipped at zero total delay.
    pub jitter: Duration,
    /// Maximum bytes buffered by this stage.
    pub max_buffer_bytes: NonZeroU64,
}

/// Bandwidth configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BandwidthConfig {
    /// Sustained rate in bytes per second.
    pub bytes_per_second: NonZeroU64,
    /// Maximum initial/refill burst in bytes.
    pub burst_bytes: NonZeroU64,
}

/// Blackhole/timeout configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlackholeConfig {
    /// Optional deadline after which the stream requests graceful termination.
    pub close_after: Option<Duration>,
}

/// Forward at most this many bytes per connection/direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitDataConfig {
    /// Maximum forwarded bytes.
    pub bytes: NonZeroU64,
}

/// Delay only shutdown, not ordinary writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlowCloseConfig {
    /// Shutdown delay.
    pub delay: Duration,
}

/// Split writes into bounded logical slices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceConfig {
    /// Average slice size.
    pub average_size: NonZeroU64,
    /// Symmetric size variation; the lower bound remains at least one.
    pub variation: u64,
    /// Optional delay between slices.
    pub delay: Duration,
}

/// Request a graceful or hard termination after the current contract boundary.
///
/// M009 corrective note: `after` was added pre-1.0 because the previous
/// type could not express Toxiproxy's delayed `reset_peer` behavior.
/// `after == ZERO` means terminate at the first defined contract boundary
/// (the first write/flush poll after the fault becomes active). A positive
/// value defers termination until that monotonic deadline has passed while
/// preserving bytes accepted before the deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisconnectConfig {
    /// Monotonic delay before the termination request becomes due.
    #[serde(default)]
    pub after: Duration,
    /// Prefer hard reset if the embedding transport can apply it.
    pub hard_reset: bool,
}

/// A typed native fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FaultKind {
    /// Delay accepted segments.
    Latency(LatencyConfig),
    /// Rate-limit accepted segments.
    Bandwidth(BandwidthConfig),
    /// Intentionally discard accepted bytes.
    Blackhole(BlackholeConfig),
    /// Stop forwarding after a byte boundary.
    LimitData(LimitDataConfig),
    /// Delay shutdown.
    SlowClose(SlowCloseConfig),
    /// Split writes into slices.
    Slice(SliceConfig),
    /// Request connection termination.
    Disconnect(DisconnectConfig),
}

/// An ordered fault with a stable identity and connection activation probability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaultSpec {
    /// Stable identity.
    pub id: FaultId,
    /// Connection activation probability.
    pub probability: Probability,
    /// Fault behavior.
    pub kind: FaultKind,
}

/// An ordered validated fault plan.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FaultPlan {
    faults: Vec<FaultSpec>,
}

impl FaultPlan {
    /// Construct and validate an ordered plan.
    pub fn new(faults: Vec<FaultSpec>) -> Result<Self, ValidationError> {
        let mut ids = std::collections::HashSet::new();
        for fault in &faults {
            if !ids.insert(fault.id.clone()) {
                return Err(ValidationError::DuplicateFaultId(fault.id.to_string()));
            }
            fault.validate()?;
        }
        Ok(Self { faults })
    }
    /// Construct an empty plan.
    pub fn empty() -> Self {
        Self::default()
    }
    /// Borrow faults in their execution order.
    pub fn faults(&self) -> &[FaultSpec] {
        &self.faults
    }
    /// Whether the plan has no stages.
    pub fn is_empty(&self) -> bool {
        self.faults.is_empty()
    }
    /// Find a fault by identity.
    pub fn get(&self, id: &str) -> Option<&FaultSpec> {
        self.faults.iter().find(|f| f.id.as_str() == id)
    }
    /// Return a plan with one fault appended.
    pub fn with_fault(mut self, fault: FaultSpec) -> Result<Self, ValidationError> {
        if self.get(fault.id.as_str()).is_some() {
            return Err(ValidationError::DuplicateFaultId(fault.id.to_string()));
        }
        fault.validate()?;
        self.faults.push(fault);
        Ok(self)
    }
    /// Remove one fault while preserving the remaining order.
    pub fn without_fault(mut self, id: &str) -> Self {
        self.faults.retain(|fault| fault.id.as_str() != id);
        self
    }
    /// Replace one fault in place, preserving its order.
    pub fn replace_fault(&self, fault: FaultSpec) -> Result<Self, ValidationError> {
        fault.validate()?;
        let mut next = self.clone();
        if let Some(existing) = next
            .faults
            .iter_mut()
            .find(|existing| existing.id == fault.id)
        {
            *existing = fault;
            Ok(next)
        } else {
            next.with_fault(fault)
        }
    }
    /// Validate all stages.
    pub fn validate(&self) -> Result<(), ValidationError> {
        for fault in &self.faults {
            fault.validate()?;
        }
        Ok(())
    }
}

impl FaultSpec {
    /// Validate the fault's internal invariants.
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self.kind {
            FaultKind::Latency(c) if c.max_buffer_bytes.get() == 0 => {
                Err(ValidationError::ZeroCapacity)
            }
            FaultKind::Slice(c) if c.variation >= c.average_size.get() => {
                Err(ValidationError::InvalidSlice)
            }
            FaultKind::Bandwidth(c)
                if c.bytes_per_second.get() == 0 || c.burst_bytes.get() == 0 =>
            {
                Err(ValidationError::InvalidRate)
            }
            FaultKind::LimitData(c) if c.bytes.get() == 0 => Err(ValidationError::ZeroLimit),
            _ => Ok(()),
        }
    }
}

/// Validation failures for native plans.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// Probability was NaN, infinite, or outside [0, 1].
    #[error("probability must be finite and between 0 and 1")]
    ProbabilityOutOfRange,
    /// Fault identifier is empty or too long.
    #[error("fault id must be 1..=128 bytes")]
    InvalidFaultId,
    /// A fault ID appears twice.
    #[error("duplicate fault id: {0}")]
    DuplicateFaultId(String),
    /// A required buffer is zero.
    #[error("fault buffer capacity must be non-zero")]
    ZeroCapacity,
    /// Slice variation cannot consume its entire average size.
    #[error("slice variation must be smaller than average size")]
    InvalidSlice,
    /// Rate configuration is invalid.
    #[error("bandwidth rate and burst must be non-zero")]
    InvalidRate,
    /// Limit must be non-zero.
    #[error("limit_data bytes must be non-zero")]
    ZeroLimit,
}
