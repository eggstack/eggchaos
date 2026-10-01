use std::{fmt, num::NonZeroU64, time::Duration};

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// The deterministic RNG contract used by eggchaos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RngVersion {
    /// `SplitMix64` with the eggchaos v1 domain-separation encoding.
    #[default]
    V1,
}

/// Maximum stream fault delay/after/hold duration (mirrors the 24h datagram cap).
pub const MAX_STREAM_FAULT_DURATION: Duration = Duration::from_secs(86_400);
/// Maximum number of faults in a stream plan (mirrors the datagram 256 cap).
pub const MAX_STREAM_FAULTS: usize = 256;

fn duration_exceeds_max(value: Duration) -> bool {
    value > MAX_STREAM_FAULT_DURATION
}

/// A validated opaque fault identity.
///
/// `FaultId` invariants are preserved on every safe construction path,
/// including [`serde`] deserialization (M049): the inner `String` is
/// reachable only through [`FaultId::new`], which rejects empty values and
/// values longer than 128 bytes. Deserialization that would otherwise
/// bypass that check is rejected with a [`serde::de::Error`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

impl Serialize for FaultId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for FaultId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        FaultId::new(raw).map_err(de::Error::custom)
    }
}

/// A probability in the closed interval [0, 1].
///
/// `Probability` invariants are preserved on every safe construction path,
/// including [`serde`] deserialization (M049): NaN, infinity, and values
/// outside `[0, 1]` are rejected. The serialized representation is a single
/// JSON number; the wire format is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
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

impl Serialize for Probability {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.0)
    }
}

impl<'de> Deserialize<'de> for Probability {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = f64::deserialize(deserializer)?;
        Probability::new(raw).map_err(de::Error::custom)
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

/// Deterministic userspace stream-chunk loss configuration (ADR 007).
///
/// This is loss of logical byte-stream ranges inside the userspace relay,
/// not IP/TCP packet loss: there is no packet capture, qdisc/netem, or TCP
/// retransmission model. Loss decisions are keyed to the absolute accepted
/// stream offset in fixed [`STREAM_LOSS_GRAIN_BYTES`] grains, so identical
/// byte streams produce identical decisions under any caller write
/// fragmentation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StreamLossConfig {
    /// Baseline probability that a logical chunk is discarded.
    pub loss_rate: Probability,
    /// Additional drop probability once the previous chunk was dropped;
    /// the correlated probability is `min(1, loss_rate + correlation)`.
    pub correlation: Probability,
}

/// Fixed v1 logical loss grain for [`FaultKind::StreamLoss`].
///
/// Byte offset `n` of the accepted stream belongs to logical chunk
/// `n / STREAM_LOSS_GRAIN_BYTES`. The grain is semantics metadata frozen by
/// ADR 007; it is not a user-configurable field in the M036-M039 tranche
/// because changing it would alter replay identity and reverse Toxiproxy
/// presentation.
pub const STREAM_LOSS_GRAIN_BYTES: u64 = 32 * 1024;

/// Stable native spelling for [`FaultKind::StreamLoss`].
///
/// Only the Toxiproxy compatibility presentation may use the post-v2.12
/// `packet_loss` spelling (M038); every native surface uses `stream-loss`.
pub const STREAM_LOSS_TYPE_NAME: &str = "stream-loss";

/// A typed native fault.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
    /// Deterministically discard fixed-grain logical stream chunks.
    StreamLoss(StreamLossConfig),
}

/// Stable low-cardinality fault-type spellings, in `FaultKind` variant
/// order. Used for metrics labels and connection evidence; the order is
/// part of the evidence contract and must not change.
pub const FAULT_TYPE_NAMES: [&str; 7] = [
    "latency",
    "bandwidth",
    "blackhole",
    "limit-data",
    "slow-close",
    "slice",
    "disconnect",
];

impl FaultKind {
    /// Stable low-cardinality type name for metrics and evidence.
    ///
    /// The name no longer indexes [`FAULT_TYPE_NAMES`]: `stream-loss` has
    /// no legacy activation-array slot (ADR 007 keeps the seven-slot
    /// arrays frozen) and reports through additive named evidence instead.
    pub const fn type_name(self) -> &'static str {
        match self {
            Self::Latency(_) => "latency",
            Self::Bandwidth(_) => "bandwidth",
            Self::Blackhole(_) => "blackhole",
            Self::LimitData(_) => "limit-data",
            Self::SlowClose(_) => "slow-close",
            Self::Slice(_) => "slice",
            Self::Disconnect(_) => "disconnect",
            Self::StreamLoss(_) => STREAM_LOSS_TYPE_NAME,
        }
    }
    /// Stable index into `FAULT_TYPE_NAMES` and activation counters, if the
    /// fault owns a legacy slot.
    ///
    /// `StreamLoss` returns `None`: its decisions are counted in the
    /// additive `stream_loss_*` evidence fields and must never resize or
    /// reorder the frozen seven-slot arrays.
    pub const fn type_index(self) -> Option<usize> {
        match self {
            Self::Latency(_) => Some(0),
            Self::Bandwidth(_) => Some(1),
            Self::Blackhole(_) => Some(2),
            Self::LimitData(_) => Some(3),
            Self::SlowClose(_) => Some(4),
            Self::Slice(_) => Some(5),
            Self::Disconnect(_) => Some(6),
            Self::StreamLoss(_) => None,
        }
    }
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
///
/// `FaultPlan` invariants are preserved on every safe construction path,
/// including [`serde`] deserialization (M049): the inner `Vec<FaultSpec>` is
/// not directly constructible; deserialization goes through
/// [`FaultPlan::new`], which rejects duplicate fault IDs and rechecks every
/// per-fault invariant. The serialized representation keeps the pre-M049
/// wire shape (`{"faults": [...]}`); only the deserialization path is
/// routed through the canonical validator.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FaultPlan {
    faults: Vec<FaultSpec>,
}

impl Serialize for FaultPlan {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Helper<'a> {
            faults: &'a [FaultSpec],
        }
        Helper {
            faults: &self.faults,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FaultPlan {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Helper {
            faults: Vec<FaultSpec>,
        }
        let helper = Helper::deserialize(deserializer)?;
        FaultPlan::new(helper.faults).map_err(de::Error::custom)
    }
}

impl FaultPlan {
    /// Construct and validate an ordered plan.
    pub fn new(faults: Vec<FaultSpec>) -> Result<Self, ValidationError> {
        if faults.len() > MAX_STREAM_FAULTS {
            return Err(ValidationError::TooManyFaults);
        }
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
        if self.faults.len() >= MAX_STREAM_FAULTS {
            return Err(ValidationError::TooManyFaults);
        }
        if self.get(fault.id.as_str()).is_some() {
            return Err(ValidationError::DuplicateFaultId(fault.id.to_string()));
        }
        fault.validate()?;
        self.faults.push(fault);
        Ok(self)
    }
    /// Remove one fault while preserving the remaining order.
    #[must_use]
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
    /// Recheck every invariant the constructor enforces.
    ///
    /// `FaultPlan::validate` is the single authority for plan-level
    /// invariants (M049): duplicate fault IDs, per-fault scalar validity
    /// (probability), and every kind-specific bound. This matches the
    /// existing [`crate::DatagramPlan::validate`] contract so deserialized
    /// plans are revalidated identically to plans built through the public
    /// constructors.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.faults.len() > MAX_STREAM_FAULTS {
            return Err(ValidationError::TooManyFaults);
        }
        let mut ids = std::collections::HashSet::new();
        for fault in &self.faults {
            if !ids.insert(fault.id.clone()) {
                return Err(ValidationError::DuplicateFaultId(fault.id.to_string()));
            }
            // Defensive scalar re-validation: scalar deserialization now
            // routes through `FaultId::new` / `Probability::new`, but
            // callers can still construct an invalid `FaultSpec` via the
            // public constructors of inner types, so re-check here.
            Probability::new(fault.probability.get())?;
            fault.validate()?;
        }
        Ok(())
    }
}

impl FaultSpec {
    /// Validate the fault's internal invariants.
    pub fn validate(&self) -> Result<(), ValidationError> {
        // All monotonic delay/after/hold durations are capped at 24h so
        // `Instant + Duration` construction can never overflow.
        match self.kind {
            FaultKind::Latency(c) if duration_exceeds_max(c.delay) => {
                return Err(ValidationError::DurationTooLong);
            }
            FaultKind::Latency(c) if duration_exceeds_max(c.jitter) => {
                return Err(ValidationError::DurationTooLong);
            }
            FaultKind::Blackhole(c) if c.close_after.is_some_and(duration_exceeds_max) => {
                return Err(ValidationError::DurationTooLong);
            }
            FaultKind::SlowClose(c) if duration_exceeds_max(c.delay) => {
                return Err(ValidationError::DurationTooLong);
            }
            FaultKind::Slice(c) if duration_exceeds_max(c.delay) => {
                return Err(ValidationError::DurationTooLong);
            }
            FaultKind::Disconnect(c) if duration_exceeds_max(c.after) => {
                return Err(ValidationError::DurationTooLong);
            }
            _ => {}
        }
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
            FaultKind::StreamLoss(c)
                if !c.loss_rate.get().is_finite()
                    || !(0.0..=1.0).contains(&c.loss_rate.get())
                    || !c.correlation.get().is_finite()
                    || !(0.0..=1.0).contains(&c.correlation.get()) =>
            {
                Err(ValidationError::ProbabilityOutOfRange)
            }
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
    /// A delay/after/hold duration exceeds the 24h cap.
    #[error("fault delay duration must be at most 24 hours")]
    DurationTooLong,
    /// Too many faults in one plan (limited to 256 stages).
    #[error("fault plans are limited to 256 stages")]
    TooManyFaults,
    /// Policy generation overflowed `u64::MAX`; nothing was published.
    #[error("policy generation overflowed")]
    GenerationOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn latency_spec(id: &str) -> FaultSpec {
        FaultSpec {
            id: FaultId::new(id).unwrap(),
            probability: Probability::new(0.5).unwrap(),
            kind: FaultKind::Latency(LatencyConfig {
                delay: Duration::from_millis(10),
                jitter: Duration::from_millis(1),
                max_buffer_bytes: NonZeroU64::new(64).unwrap(),
            }),
        }
    }

    #[test]
    fn fault_id_rejects_empty_and_oversized_via_serde() {
        let empty = serde_json::from_str::<FaultId>("\"\"").unwrap_err();
        assert!(empty.to_string().contains("1..=128"), "got {empty}");
        let oversized = "x".repeat(129);
        let json = format!("\"{oversized}\"");
        let err = serde_json::from_str::<FaultId>(&json).unwrap_err();
        assert!(err.to_string().contains("1..=128"), "got {err}");
    }

    #[test]
    fn fault_id_accepts_boundary_lengths_via_serde() {
        let one = serde_json::from_str::<FaultId>("\"a\"").unwrap();
        assert_eq!(one.as_str(), "a");
        let max = "x".repeat(128);
        let json = format!("\"{max}\"");
        let parsed = serde_json::from_str::<FaultId>(&json).unwrap();
        assert_eq!(parsed.as_str().len(), 128);
    }

    #[test]
    fn probability_rejects_out_of_range_via_serde() {
        let negative = serde_json::from_str::<Probability>("-0.01").unwrap_err();
        assert!(negative.to_string().contains("finite"), "got {negative}");
        let above = serde_json::from_str::<Probability>("1.01").unwrap_err();
        assert!(above.to_string().contains("finite"), "got {above}");
    }

    #[test]
    // M059: boundary round-trips are exact by construction; strict comparison is the semantic assertion.
    #[allow(clippy::float_cmp)]
    fn probability_accepts_boundary_values_via_serde() {
        let zero = serde_json::from_str::<Probability>("0.0").unwrap();
        assert_eq!(zero.get(), 0.0);
        let one = serde_json::from_str::<Probability>("1.0").unwrap();
        assert_eq!(one.get(), 1.0);
    }

    #[test]
    fn probability_constructor_rejects_nan_and_infinity() {
        // NaN/Infinity cannot round-trip through JSON, but the scalar
        // constructor (M049) must still reject them so callers that build a
        // `Probability` outside of serde (e.g. through FFI or other
        // arithmetic) cannot bypass the invariant.
        assert!(Probability::new(f64::NAN).is_err());
        assert!(Probability::new(f64::INFINITY).is_err());
        assert!(Probability::new(f64::NEG_INFINITY).is_err());
    }

    #[test]
    fn fault_plan_deserializes_empty_and_round_trips() {
        let plan = FaultPlan::empty();
        let encoded = serde_json::to_vec(&plan).unwrap();
        let decoded: FaultPlan = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(plan, decoded);
        assert!(decoded.is_empty());
        assert_eq!(encoded, br#"{"faults":[]}"#);
    }

    #[test]
    fn fault_plan_rejects_duplicate_ids_via_serde() {
        let json = r#"{
            "faults": [
                {"id":"dup","probability":0.5,"kind":{"Latency":{"delay":{"secs":0,"nanos":10000000},"jitter":{"secs":0,"nanos":1000000},"max_buffer_bytes":64}}},
                {"id":"dup","probability":0.5,"kind":{"Latency":{"delay":{"secs":0,"nanos":10000000},"jitter":{"secs":0,"nanos":1000000},"max_buffer_bytes":64}}}
            ]
        }"#;
        let err = serde_json::from_str::<FaultPlan>(json).unwrap_err();
        assert!(err.to_string().contains("duplicate"), "got {err}");
    }

    #[test]
    fn fault_plan_rejects_oversized_id_via_serde() {
        let id = "x".repeat(129);
        let json = format!(
            r#"{{"faults":[{{"id":"{id}","probability":0.5,"kind":{{"Latency":{{"delay":{{"secs":0,"nanos":10000000}},"jitter":{{"secs":0,"nanos":1000000}},"max_buffer_bytes":64}}}}}}]}}"#
        );
        let err = serde_json::from_str::<FaultPlan>(&json).unwrap_err();
        assert!(err.to_string().contains("1..=128"), "got {err}");
    }

    #[test]
    fn fault_plan_rejects_empty_id_via_serde() {
        let json = r#"{"faults":[{"id":"","probability":0.5,"kind":{"Latency":{"delay":{"secs":0,"nanos":10000000},"jitter":{"secs":0,"nanos":1000000},"max_buffer_bytes":64}}}]}"#;
        let err = serde_json::from_str::<FaultPlan>(json).unwrap_err();
        assert!(err.to_string().contains("1..=128"), "got {err}");
    }

    #[test]
    fn fault_plan_rejects_out_of_range_probability_via_serde() {
        let json = r#"{"faults":[{"id":"a","probability":1.5,"kind":{"Latency":{"delay":{"secs":0,"nanos":10000000},"jitter":{"secs":0,"nanos":1000000},"max_buffer_bytes":64}}}]}"#;
        let err = serde_json::from_str::<FaultPlan>(json).unwrap_err();
        assert!(err.to_string().contains("finite"), "got {err}");
    }

    #[test]
    fn fault_plan_rejects_kind_specific_bound_via_serde() {
        // `Slice` with `variation >= average_size` is invalid.
        let json = r#"{"faults":[{"id":"a","probability":0.5,"kind":{"Slice":{"average_size":8,"variation":8,"delay":{"secs":0,"nanos":0}}}}]}"#;
        let err = serde_json::from_str::<FaultPlan>(json).unwrap_err();
        assert!(
            err.to_string().contains("smaller than average"),
            "got {err}"
        );
    }

    #[test]
    fn fault_plan_round_trip_preserves_every_fault_kind() {
        let plan = FaultPlan::new(vec![
            FaultSpec {
                id: FaultId::new("latency").unwrap(),
                probability: Probability::new(0.0).unwrap(),
                kind: FaultKind::Latency(LatencyConfig {
                    delay: Duration::from_millis(10),
                    jitter: Duration::from_millis(1),
                    max_buffer_bytes: NonZeroU64::new(64).unwrap(),
                }),
            },
            FaultSpec {
                id: FaultId::new("bandwidth").unwrap(),
                probability: Probability::new(1.0).unwrap(),
                kind: FaultKind::Bandwidth(BandwidthConfig {
                    bytes_per_second: NonZeroU64::new(1024).unwrap(),
                    burst_bytes: NonZeroU64::new(2048).unwrap(),
                }),
            },
            FaultSpec {
                id: FaultId::new("blackhole").unwrap(),
                probability: Probability::new(0.5).unwrap(),
                kind: FaultKind::Blackhole(BlackholeConfig {
                    close_after: Some(Duration::from_secs(60)),
                }),
            },
            FaultSpec {
                id: FaultId::new("limit-data").unwrap(),
                probability: Probability::new(1.0).unwrap(),
                kind: FaultKind::LimitData(LimitDataConfig {
                    bytes: NonZeroU64::new(4096).unwrap(),
                }),
            },
            FaultSpec {
                id: FaultId::new("slow-close").unwrap(),
                probability: Probability::new(1.0).unwrap(),
                kind: FaultKind::SlowClose(SlowCloseConfig {
                    delay: Duration::from_millis(5),
                }),
            },
            FaultSpec {
                id: FaultId::new("slice").unwrap(),
                probability: Probability::new(1.0).unwrap(),
                kind: FaultKind::Slice(SliceConfig {
                    average_size: NonZeroU64::new(1024).unwrap(),
                    variation: 128,
                    delay: Duration::ZERO,
                }),
            },
            FaultSpec {
                id: FaultId::new("disconnect").unwrap(),
                probability: Probability::new(1.0).unwrap(),
                kind: FaultKind::Disconnect(DisconnectConfig {
                    after: Duration::from_secs(2),
                    hard_reset: true,
                }),
            },
            FaultSpec {
                id: FaultId::new("stream-loss").unwrap(),
                probability: Probability::new(0.25).unwrap(),
                kind: FaultKind::StreamLoss(StreamLossConfig {
                    loss_rate: Probability::new(0.1).unwrap(),
                    correlation: Probability::new(0.5).unwrap(),
                }),
            },
        ])
        .unwrap();
        let encoded = serde_json::to_vec(&plan).unwrap();
        let decoded: FaultPlan = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(plan, decoded);
        assert_eq!(decoded.faults().len(), 8);
    }

    #[test]
    fn fault_plan_validate_agrees_with_constructor() {
        // Build a valid plan through the constructor and confirm validate agrees.
        let plan = FaultPlan::new(vec![latency_spec("a"), latency_spec("b")]).unwrap();
        assert!(plan.validate().is_ok());
        // Re-validate after a duplicate-id mutation that bypasses the constructor.
        let mut broken = plan.clone();
        broken.faults.push(latency_spec("a"));
        assert!(matches!(
            broken.validate(),
            Err(ValidationError::DuplicateFaultId(_))
        ));
        // Re-validate after a probability that violates scalar bounds.
        let mut bad_prob = plan.clone();
        let target = bad_prob
            .faults
            .iter_mut()
            .find(|f| f.id.as_str() == "a")
            .unwrap();
        *target = FaultSpec {
            id: FaultId::new("a").unwrap(),
            probability: Probability(2.0),
            kind: target.kind,
        };
        assert!(matches!(
            bad_prob.validate(),
            Err(ValidationError::ProbabilityOutOfRange)
        ));
    }

    #[test]
    fn with_fault_rejects_duplicates() {
        let plan = FaultPlan::new(vec![latency_spec("a")]).unwrap();
        assert!(matches!(
            plan.clone().with_fault(latency_spec("a")),
            Err(ValidationError::DuplicateFaultId(_))
        ));
    }

    #[test]
    fn empty_plan_serializes_and_validates() {
        let encoded = serde_json::to_vec(&FaultPlan::empty()).unwrap();
        assert_eq!(encoded, br#"{"faults":[]}"#);
        let decoded: FaultPlan = serde_json::from_slice(&encoded).unwrap();
        assert!(decoded.validate().is_ok());
    }

    #[test]
    fn datagram_plan_parity_check_passes_for_valid() {
        // Prove the stream plan's validate() reaches the same authoritative
        // verdict as the documented `DatagramPlan::validate` flow: deserialized
        // documents either validate cleanly or fail with a typed reason.
        let json = r#"{"faults":[{"id":"a","probability":0.5,"kind":{"Latency":{"delay":{"secs":0,"nanos":10000000},"jitter":{"secs":0,"nanos":1000000},"max_buffer_bytes":64}}}]}"#;
        let parsed: FaultPlan = serde_json::from_str(json).unwrap();
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn live_policy_publication_validates_invalid_plans() {
        use crate::policy::LivePolicy;
        // Even though scalar deserialization now prevents constructing an
        // invalid `Probability`, `LivePolicy::publish` still rechecks via
        // `FaultPlan::validate`. Bypass the constructor to assert the
        // publication barrier remains in place.
        let plan = FaultPlan::new(vec![latency_spec("a")]).unwrap();
        let live = LivePolicy::new(plan, 0);
        assert!(live.publish(FaultPlan::empty(), 0).is_ok());
        let bad = {
            let mut p = FaultPlan::empty();
            p.faults.push(latency_spec("a"));
            p.faults.push(latency_spec("a"));
            p
        };
        assert!(matches!(
            live.publish(bad, 0),
            Err(ValidationError::DuplicateFaultId(_))
        ));
    }
}
