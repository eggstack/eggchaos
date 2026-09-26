//! Bounded, deterministic, protocol-neutral datagram impairment.
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, HashSet},
    num::NonZeroU64,
    sync::Arc,
    time::Duration,
};

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::{derive_seed, DeterministicRng, Direction, FaultId, Probability, RngVersion};

/// Stable names and evidence-counter order for datagram fault kinds.
pub const DATAGRAM_FAULT_TYPE_NAMES: [&str; 6] = [
    "delay",
    "loss",
    "duplicate",
    "reorder",
    "payload-corrupt",
    "bandwidth",
];

/// Derive a datagram-only fault stream, separated from all stream RNG domains.
pub fn derive_datagram_seed(
    seed_namespace: u64,
    proxy: &str,
    association: u64,
    direction: Direction,
    fault: &FaultId,
) -> u64 {
    derive_seed(seed_namespace, proxy, association, direction, fault) ^ 0xd47a_6a4a_0000_0001
}

/// Bounds shared by one direction's pending datagram scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatagramQueueLimits {
    pub max_queued_datagrams: NonZeroU64,
    pub max_queued_bytes: NonZeroU64,
    pub max_datagram_bytes: NonZeroU64,
}
impl DatagramQueueLimits {
    pub fn validate(self) -> Result<Self, &'static str> {
        if self.max_queued_datagrams.get() > 1_000_000
            || self.max_queued_bytes.get() > 1_073_741_824
            || self.max_datagram_bytes.get() > 65_535
        {
            return Err("datagram queue limits exceed hard bounds");
        }
        Ok(self)
    }
}

/// One ordered datagram fault stage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatagramFaultSpec {
    pub id: FaultId,
    pub probability: Probability,
    pub kind: DatagramFaultKind,
}

/// Supported datagram transformations and scheduling stages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum DatagramFaultKind {
    Delay {
        delay: Duration,
        jitter: Duration,
    },
    Loss,
    Duplicate {
        additional_copies: u8,
    },
    Reorder {
        hold: Duration,
    },
    PayloadCorrupt {
        bytes: NonZeroU64,
    },
    Bandwidth {
        bytes_per_second: NonZeroU64,
        burst_bytes: NonZeroU64,
    },
}

impl DatagramFaultKind {
    pub const fn type_name(&self) -> &'static str {
        DATAGRAM_FAULT_TYPE_NAMES[fault_type_index(self)]
    }
}

/// Validated, ordered datagram plan.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DatagramPlan {
    faults: Vec<DatagramFaultSpec>,
}

impl DatagramPlan {
    pub fn new(faults: Vec<DatagramFaultSpec>) -> Result<Self, &'static str> {
        let plan = Self { faults };
        plan.validate()?;
        Ok(plan)
    }
    /// Recheck plans created through deserialization before publication/use.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.faults.len() > 256 {
            return Err("datagram plans are limited to 256 stages");
        }
        let mut ids = HashSet::new();
        let mut maximum_candidates = 1u64;
        for f in &self.faults {
            if f.id.as_str().is_empty() || f.id.as_str().len() > 128 {
                return Err("fault id must be 1..=128 bytes");
            }
            if Probability::new(f.probability.get()).is_err() {
                return Err("probability must be finite and within 0..=1");
            }
            if !ids.insert(f.id.as_str()) {
                return Err("duplicate fault id");
            }
            match &f.kind {
                DatagramFaultKind::Duplicate { additional_copies }
                    if *additional_copies == 0 || *additional_copies > 16 =>
                {
                    return Err("duplicate count must be 1..=16")
                }
                DatagramFaultKind::Duplicate { additional_copies } => {
                    maximum_candidates =
                        maximum_candidates.saturating_mul(u64::from(*additional_copies) + 1);
                    if maximum_candidates > 4096 {
                        return Err("plan duplication amplification exceeds 4096 candidates");
                    }
                }
                DatagramFaultKind::Delay { delay, jitter }
                    if delay.as_secs() > 86_400 || jitter.as_secs() > 86_400 =>
                {
                    return Err("delay and jitter must be at most 24 hours")
                }
                DatagramFaultKind::Reorder { hold } if hold.as_secs() > 86_400 => {
                    return Err("reorder hold must be at most 24 hours")
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub fn empty() -> Self {
        Self::default()
    }
    pub fn faults(&self) -> &[DatagramFaultSpec] {
        &self.faults
    }
}

/// Immutable plan generation used when a datagram enters the engine.
#[derive(Debug, Clone, PartialEq)]
pub struct PublishedDatagramPolicy {
    pub generation: u64,
    pub plan: Arc<DatagramPlan>,
    pub seed_namespace: u64,
}

/// Read-mostly datagram policy publisher.
#[derive(Clone, Debug)]
pub struct DatagramLivePolicy {
    current: Arc<arc_swap::ArcSwap<PublishedDatagramPolicy>>,
}

impl DatagramLivePolicy {
    pub fn new(plan: DatagramPlan, seed_namespace: u64) -> Result<Self, &'static str> {
        plan.validate()?;
        Ok(Self {
            current: Arc::new(arc_swap::ArcSwap::new(Arc::new(PublishedDatagramPolicy {
                generation: 1,
                plan: Arc::new(plan),
                seed_namespace,
            }))),
        })
    }
    pub fn snapshot(&self) -> Arc<PublishedDatagramPolicy> {
        self.current.load_full()
    }
    pub fn publish(
        &self,
        plan: DatagramPlan,
        seed_namespace: u64,
    ) -> Result<Arc<PublishedDatagramPolicy>, &'static str> {
        plan.validate()?;
        let next = Arc::new(PublishedDatagramPolicy {
            generation: self.snapshot().generation.saturating_add(1),
            plan: Arc::new(plan),
            seed_namespace,
        });
        self.current.store(next.clone());
        Ok(next)
    }
}

/// Evidence for a direction-local datagram engine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatagramEvidence {
    pub admitted_datagrams: u64,
    pub admitted_bytes: u64,
    pub emitted_datagrams: u64,
    pub emitted_bytes: u64,
    pub configured_loss: u64,
    pub queue_overflow: u64,
    pub oversize_datagrams: u64,
    pub duplicated_copies: u64,
    pub corrupted_candidates: u64,
    pub reorder_activations: u64,
    pub queued_datagrams: u64,
    pub queued_bytes: u64,
    pub high_water_datagrams: u64,
    pub high_water_bytes: u64,
    pub fault_activations: [u64; 6],
    pub injected_delay_nanos: u128,
    pub bandwidth_delay_nanos: u128,
    pub last_generation: u64,
    pub last_seed_namespace: u64,
    pub rng_version: RngVersion,
}

/// A datagram ready for transport emission, preserving identity and generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatagramScheduled {
    pub payload: Bytes,
    pub ingress_ordinal: u64,
    pub copy_index: u16,
    pub generation: u64,
}

#[derive(Debug, Clone)]
struct Candidate {
    at: Instant,
    ordinal: u64,
    copy: u16,
    payload: Bytes,
    generation: u64,
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.at == other.at && self.ordinal == other.ordinal && self.copy == other.copy
    }
}
impl Eq for Candidate {}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
/// Total deterministic scheduling order: `(release_at, ingress_ordinal,
/// copy_index)`. Payload and generation never participate, so equal keys are
/// impossible (`(ordinal, copy)` is unique per candidate) and heap pop order
/// is byte-for-byte identical to the previous full-queue sort.
impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.at, self.ordinal, self.copy).cmp(&(other.at, other.ordinal, other.copy))
    }
}
#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: u64,
    updated: Instant,
}

/// Admission disposition for one datagram. `Immediate` items are fully
/// accounted (admitted, queued/high-water, then emitted) and must be sent
/// without entering the deadline heap; `Queued` items are owned by the
/// scheduler and surface through [`DatagramDirectionEngine::take_ready`].
#[derive(Debug)]
pub enum DatagramAdmission {
    /// Oversize, fully configured-loss, or fully overflowed: nothing to send.
    Consumed,
    /// No release delay and the scheduler was empty: send now.
    Immediate(Vec<DatagramScheduled>),
    /// The scheduler owns one or more candidates.
    Queued,
}

/// Single-owner deterministic direction engine. Call `admit` in ingress order and
/// `take_ready` from the embedding runtime's timer/select loop. The pending
/// queue is a min-heap keyed by `(release_at, ingress_ordinal, copy_index)`,
/// so deadline inspection is O(1), insertion is O(log n), and draining k
/// ready candidates is O(k log n) with exact stable ordering.
#[derive(Debug)]
pub struct DatagramDirectionEngine {
    limits: DatagramQueueLimits,
    proxy: String,
    association: u64,
    direction: Direction,
    rng_version: RngVersion,
    ordinal: u64,
    queue: BinaryHeap<Reverse<Candidate>>,
    evidence: DatagramEvidence,
    buckets: HashMap<String, Bucket>,
}

impl DatagramDirectionEngine {
    pub fn new(
        limits: DatagramQueueLimits,
        proxy: impl Into<String>,
        association: u64,
        direction: Direction,
        rng_version: RngVersion,
    ) -> Result<Self, &'static str> {
        let limits = limits.validate()?;
        let evidence = DatagramEvidence {
            rng_version,
            ..DatagramEvidence::default()
        };
        Ok(Self {
            limits,
            proxy: proxy.into(),
            association,
            direction,
            rng_version,
            ordinal: 0,
            queue: BinaryHeap::new(),
            evidence,
            buckets: HashMap::new(),
        })
    }
    pub fn evidence(&self) -> &DatagramEvidence {
        &self.evidence
    }
    pub fn next_deadline(&self) -> Option<Instant> {
        self.queue.peek().map(|item| item.0.at)
    }
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
    pub fn discard_all(&mut self) -> usize {
        let n = self.queue.len();
        self.evidence.queued_datagrams = 0;
        self.evidence.queued_bytes = 0;
        self.queue.clear();
        n
    }

    /// Admit one whole payload under one atomic policy snapshot.
    ///
    /// An empty plan never allocates a candidate vector and never enters the
    /// deadline heap when the scheduler is empty: the datagram is accounted
    /// exactly as if it had been queued and immediately drained, and is
    /// returned as [`DatagramAdmission::Immediate`]. The same immediate path
    /// applies to any fault combination whose candidates all carry zero
    /// release delay while the scheduler is empty. Requiring an empty
    /// scheduler keeps immediate emission observably identical to
    /// queue-then-drain ordering; evidence, ingress ordinals, generation, and
    /// seed tracking are identical on every path.
    pub fn admit(
        &mut self,
        now: Instant,
        payload: Bytes,
        policy: &PublishedDatagramPolicy,
    ) -> DatagramAdmission {
        let ordinal = self.ordinal;
        self.ordinal = self.ordinal.saturating_add(1);
        self.evidence.admitted_datagrams = self.evidence.admitted_datagrams.saturating_add(1);
        self.evidence.admitted_bytes = self
            .evidence
            .admitted_bytes
            .saturating_add(payload.len() as u64);
        if payload.len() as u64 > self.limits.max_datagram_bytes.get() {
            self.evidence.oversize_datagrams = self.evidence.oversize_datagrams.saturating_add(1);
            return DatagramAdmission::Consumed;
        }
        self.evidence.last_generation = policy.generation;
        self.evidence.last_seed_namespace = policy.seed_namespace;
        if policy.plan.faults().is_empty() {
            if self.queue.is_empty() && self.fits(payload.len() as u64) {
                let scheduled = self.emit_immediate_single(ordinal, payload, policy.generation);
                return DatagramAdmission::Immediate(vec![scheduled]);
            }
            if self.fits(payload.len() as u64) {
                self.enqueue(now, ordinal, 0, payload, policy.generation);
                return DatagramAdmission::Queued;
            }
            self.evidence.queue_overflow = self.evidence.queue_overflow.saturating_add(1);
            return DatagramAdmission::Consumed;
        }
        let mut candidates = vec![(0u16, payload, Duration::ZERO)];
        let mut next_copy_index = 1u16;
        for (stage, fault) in policy.plan.faults().iter().enumerate() {
            // M044 WP5 — size the next-stage buffer from the validated
            // bounded amplification of this stage. Duplicate stages can
            // emit `additional_copies + 1` outputs per input; other
            // stages emit at most one per input. Saturating arithmetic
            // keeps the existing 4,096 candidate hard bound intact.
            let next_capacity = match &fault.kind {
                DatagramFaultKind::Duplicate { additional_copies } => candidates
                    .len()
                    .saturating_mul(usize::from(*additional_copies).saturating_add(1))
                    .max(1),
                _ => candidates.len().max(1),
            };
            let mut next = Vec::with_capacity(next_capacity);
            for (copy, mut bytes, delay) in candidates {
                let seed = derive_datagram_seed(
                    policy.seed_namespace ^ u64::from(copy),
                    &self.proxy,
                    self.association ^ ordinal.rotate_left(9) ^ stage as u64,
                    self.direction,
                    &fault.id,
                ) ^ match self.rng_version {
                    RngVersion::V1 => 0,
                };
                let mut rng = DeterministicRng::new(seed);
                if !rng.bernoulli(fault.probability.get()) {
                    next.push((copy, bytes, delay));
                    continue;
                }
                let fault_index = fault_type_index(&fault.kind);
                self.evidence.fault_activations[fault_index] =
                    self.evidence.fault_activations[fault_index].saturating_add(1);
                match &fault.kind {
                    DatagramFaultKind::Loss => {
                        self.evidence.configured_loss =
                            self.evidence.configured_loss.saturating_add(1);
                    }
                    DatagramFaultKind::Duplicate { additional_copies } => {
                        next.push((copy, bytes.clone(), delay));
                        for _ in 1..=*additional_copies {
                            let duplicate_index = next_copy_index;
                            next_copy_index = next_copy_index.saturating_add(1);
                            next.push((duplicate_index, bytes.clone(), delay));
                            self.evidence.duplicated_copies =
                                self.evidence.duplicated_copies.saturating_add(1);
                        }
                    }
                    DatagramFaultKind::PayloadCorrupt { bytes: count } => {
                        if !bytes.is_empty() {
                            let n = count.get().min(bytes.len() as u64);
                            let mut v = bytes.to_vec();
                            for i in 0..n as usize {
                                let selected = i + rng.below((v.len() - i) as u64) as usize;
                                v.swap(i, selected);
                                v[i] ^= 1 << rng.below(8);
                            }
                            bytes = Bytes::from(v);
                            self.evidence.corrupted_candidates =
                                self.evidence.corrupted_candidates.saturating_add(1);
                        }
                        next.push((copy, bytes, delay));
                    }
                    kind => {
                        let added = if let DatagramFaultKind::Bandwidth {
                            bytes_per_second,
                            burst_bytes,
                        } = kind
                        {
                            self.bandwidth_delay(
                                fault.id.as_str(),
                                now,
                                bytes.len() as u64,
                                bytes_per_second.get(),
                                burst_bytes.get(),
                            )
                        } else {
                            fault_delay(kind, &mut rng)
                        };
                        if matches!(kind, DatagramFaultKind::Bandwidth { .. }) {
                            self.evidence.bandwidth_delay_nanos = self
                                .evidence
                                .bandwidth_delay_nanos
                                .saturating_add(added.as_nanos());
                        } else {
                            self.evidence.injected_delay_nanos = self
                                .evidence
                                .injected_delay_nanos
                                .saturating_add(added.as_nanos());
                        }
                        next.push((copy, bytes, delay.saturating_add(added)));
                        if matches!(kind, DatagramFaultKind::Reorder { .. }) {
                            self.evidence.reorder_activations =
                                self.evidence.reorder_activations.saturating_add(1);
                        }
                    }
                }
            }
            candidates = next;
        }
        if candidates.iter().all(|(_, _, delay)| delay.is_zero())
            && self.queue.is_empty()
            && candidates
                .iter()
                .try_fold(0u64, |total, (_, bytes, _)| {
                    total.checked_add(bytes.len() as u64)
                })
                .is_some_and(|bytes| self.fits_all(candidates.len() as u64, bytes))
        {
            let mut out = Vec::with_capacity(candidates.len());
            for (copy, bytes, _) in candidates {
                out.push(self.emit_immediate(ordinal, bytes, policy.generation, copy));
            }
            // Candidates complete the fault loop in pipeline order, which is
            // not copy order after cascading duplication. The scheduler path
            // always drains in `(release_at, ordinal, copy)` order, so sort
            // the immediate items the same way to keep both paths identical.
            out.sort_by_key(|item| (item.ingress_ordinal, item.copy_index));
            return DatagramAdmission::Immediate(out);
        }
        if candidates.is_empty() {
            return DatagramAdmission::Consumed;
        }
        let mut queued_any = false;
        for (copy, bytes, delay) in candidates {
            if self.queue.len() as u64 >= self.limits.max_queued_datagrams.get()
                || self
                    .evidence
                    .queued_bytes
                    .saturating_add(bytes.len() as u64)
                    > self.limits.max_queued_bytes.get()
            {
                self.evidence.queue_overflow = self.evidence.queue_overflow.saturating_add(1);
                continue;
            }
            self.enqueue(now + delay, ordinal, copy, bytes, policy.generation);
            queued_any = true;
        }
        if queued_any {
            DatagramAdmission::Queued
        } else {
            DatagramAdmission::Consumed
        }
    }

    fn fits(&self, bytes: u64) -> bool {
        (self.queue.len() as u64) < self.limits.max_queued_datagrams.get()
            && self.evidence.queued_bytes.saturating_add(bytes)
                <= self.limits.max_queued_bytes.get()
    }

    fn fits_all(&self, datagrams: u64, bytes: u64) -> bool {
        (self.queue.len() as u64).saturating_add(datagrams)
            <= self.limits.max_queued_datagrams.get()
            && self.evidence.queued_bytes.saturating_add(bytes)
                <= self.limits.max_queued_bytes.get()
    }

    fn enqueue(&mut self, at: Instant, ordinal: u64, copy: u16, payload: Bytes, generation: u64) {
        self.evidence.queued_datagrams += 1;
        self.evidence.queued_bytes += payload.len() as u64;
        self.evidence.high_water_datagrams = self
            .evidence
            .high_water_datagrams
            .max(self.evidence.queued_datagrams);
        self.evidence.high_water_bytes = self
            .evidence
            .high_water_bytes
            .max(self.evidence.queued_bytes);
        self.queue.push(Reverse(Candidate {
            at,
            ordinal,
            copy,
            payload,
            generation,
        }));
    }

    /// Account one candidate exactly as queue-then-drain would: queued and
    /// high-water counters advance, then the candidate is emitted immediately.
    fn emit_immediate(
        &mut self,
        ordinal: u64,
        payload: Bytes,
        generation: u64,
        copy: u16,
    ) -> DatagramScheduled {
        self.evidence.queued_datagrams += 1;
        self.evidence.queued_bytes += payload.len() as u64;
        self.evidence.high_water_datagrams = self
            .evidence
            .high_water_datagrams
            .max(self.evidence.queued_datagrams);
        self.evidence.high_water_bytes = self
            .evidence
            .high_water_bytes
            .max(self.evidence.queued_bytes);
        let size = payload.len() as u64;
        self.evidence.queued_datagrams -= 1;
        self.evidence.queued_bytes -= size;
        self.evidence.emitted_datagrams += 1;
        self.evidence.emitted_bytes += size;
        DatagramScheduled {
            payload,
            ingress_ordinal: ordinal,
            copy_index: copy,
            generation,
        }
    }

    /// Account one empty-plan candidate with copy index zero.
    fn emit_immediate_single(
        &mut self,
        ordinal: u64,
        payload: Bytes,
        generation: u64,
    ) -> DatagramScheduled {
        self.emit_immediate(ordinal, payload, generation, 0)
    }

    fn bandwidth_delay(
        &mut self,
        id: &str,
        now: Instant,
        bytes: u64,
        rate: u64,
        burst: u64,
    ) -> Duration {
        // Avoid cloning the fault id on the steady-state path where the token
        // bucket already exists; only the first admission per fault allocates.
        if !self.buckets.contains_key(id) {
            self.buckets.insert(
                id.to_owned(),
                Bucket {
                    tokens: burst,
                    updated: now,
                },
            );
        }
        let bucket = self.buckets.get_mut(id).expect("inserted bucket");
        let virtual_now = now.max(bucket.updated);
        let elapsed = virtual_now
            .saturating_duration_since(bucket.updated)
            .as_nanos();
        let refill = elapsed.saturating_mul(u128::from(rate)) / 1_000_000_000;
        bucket.tokens = bucket
            .tokens
            .saturating_add(refill.min(u64::MAX as u128) as u64)
            .min(burst);
        bucket.updated = virtual_now;
        if bucket.tokens >= bytes {
            bucket.tokens -= bytes;
            virtual_now.saturating_duration_since(now)
        } else {
            let deficit = bytes - bucket.tokens;
            bucket.tokens = 0;
            let nanos = (u128::from(deficit)
                .saturating_mul(1_000_000_000)
                .saturating_add(u128::from(rate - 1)))
                / u128::from(rate);
            let service = Duration::from_nanos(nanos.min(u64::MAX as u128) as u64);
            bucket.updated = virtual_now + service;
            virtual_now
                .saturating_duration_since(now)
                .saturating_add(service)
        }
    }

    /// Remove all currently due candidates in deterministic deadline/identity
    /// order by popping the min-heap while its head is ready.
    pub fn take_ready(&mut self, now: Instant) -> Vec<DatagramScheduled> {
        let mut out = Vec::new();
        while self.queue.peek().is_some_and(|head| head.0.at <= now) {
            let candidate = self.queue.pop().expect("peeked candidate").0;
            self.evidence.queued_datagrams -= 1;
            self.evidence.queued_bytes -= candidate.payload.len() as u64;
            self.evidence.emitted_datagrams += 1;
            self.evidence.emitted_bytes += candidate.payload.len() as u64;
            out.push(DatagramScheduled {
                payload: candidate.payload,
                ingress_ordinal: candidate.ordinal,
                copy_index: candidate.copy,
                generation: candidate.generation,
            });
        }
        out
    }
}

const fn fault_type_index(kind: &DatagramFaultKind) -> usize {
    match kind {
        DatagramFaultKind::Delay { .. } => 0,
        DatagramFaultKind::Loss => 1,
        DatagramFaultKind::Duplicate { .. } => 2,
        DatagramFaultKind::Reorder { .. } => 3,
        DatagramFaultKind::PayloadCorrupt { .. } => 4,
        DatagramFaultKind::Bandwidth { .. } => 5,
    }
}

fn fault_delay(kind: &DatagramFaultKind, rng: &mut DeterministicRng) -> Duration {
    match kind {
        DatagramFaultKind::Delay { delay, jitter } => jittered(*delay, *jitter, rng),
        DatagramFaultKind::Reorder { hold } => *hold,
        DatagramFaultKind::Bandwidth {
            bytes_per_second, ..
        } => Duration::from_secs_f64(1.0 / bytes_per_second.get() as f64),
        _ => Duration::ZERO,
    }
}
fn jittered(delay: Duration, jitter: Duration, rng: &mut DeterministicRng) -> Duration {
    if jitter.is_zero() {
        return delay;
    }
    let range = jitter.as_nanos().min(u64::MAX as u128) as u64;
    let signed = rng.below(range.saturating_mul(2).saturating_add(1)) as i128 - range as i128;
    let total = delay.as_nanos() as i128 + signed;
    Duration::from_nanos(total.max(0).min(u64::MAX as i128) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn limits() -> DatagramQueueLimits {
        DatagramQueueLimits {
            max_queued_datagrams: NonZeroU64::new(8).unwrap(),
            max_queued_bytes: NonZeroU64::new(64).unwrap(),
            max_datagram_bytes: NonZeroU64::new(32).unwrap(),
        }
    }
    fn spec(id: &str, kind: DatagramFaultKind) -> DatagramFaultSpec {
        DatagramFaultSpec {
            id: FaultId::new(id).unwrap(),
            probability: Probability::new(1.0).unwrap(),
            kind,
        }
    }
    fn policy(plan: DatagramPlan, generation: u64) -> PublishedDatagramPolicy {
        PublishedDatagramPolicy {
            generation,
            plan: Arc::new(plan),
            seed_namespace: 7,
        }
    }

    #[test]
    fn datagram_rng_domain_is_stable_and_stream_separate() {
        let id = FaultId::new("loss").unwrap();
        assert_eq!(
            derive_datagram_seed(42, "proxy", 7, Direction::Upstream, &id),
            5_358_773_348_858_769_006
        );
    }

    #[test]
    fn plan_bounds_duplication_amplification() {
        let duplicate = || {
            spec(
                "dup",
                DatagramFaultKind::Duplicate {
                    additional_copies: 16,
                },
            )
        };
        let plan = DatagramPlan::new(vec![
            duplicate(),
            spec(
                "dup2",
                DatagramFaultKind::Duplicate {
                    additional_copies: 16,
                },
            ),
        ]);
        assert!(plan.is_ok());
        let too_many = DatagramPlan::new(vec![
            duplicate(),
            spec(
                "dup2",
                DatagramFaultKind::Duplicate {
                    additional_copies: 16,
                },
            ),
            spec(
                "dup3",
                DatagramFaultKind::Duplicate {
                    additional_copies: 16,
                },
            ),
        ]);
        assert!(too_many.is_err());
    }

    #[test]
    fn deserialized_fault_ids_are_revalidated_before_policy_publication() {
        let plan: DatagramPlan = serde_json::from_str(
            r#"{"faults":[{"id":"","probability":1.0,"kind":{"type":"loss"}}]}"#,
        )
        .unwrap();
        assert!(plan.validate().is_err());
        assert!(DatagramLivePolicy::new(plan, 0).is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn loss_duplication_and_ordered_stage_composition_are_exact() {
        let now = Instant::now();
        let mut e =
            DatagramDirectionEngine::new(limits(), "p", 1, Direction::Upstream, RngVersion::V1)
                .unwrap();
        let p = policy(
            DatagramPlan::new(vec![
                spec(
                    "dup",
                    DatagramFaultKind::Duplicate {
                        additional_copies: 2,
                    },
                ),
                spec("loss", DatagramFaultKind::Loss),
            ])
            .unwrap(),
            3,
        );
        e.admit(now, Bytes::from_static(b"x"), &p);
        assert_eq!(e.evidence().configured_loss, 3);
        assert_eq!(e.evidence().duplicated_copies, 2);
        assert!(e.take_ready(now).is_empty());
        let mut e =
            DatagramDirectionEngine::new(limits(), "p", 1, Direction::Upstream, RngVersion::V1)
                .unwrap();
        let p = policy(
            DatagramPlan::new(vec![
                spec("loss", DatagramFaultKind::Loss),
                spec(
                    "dup",
                    DatagramFaultKind::Duplicate {
                        additional_copies: 2,
                    },
                ),
            ])
            .unwrap(),
            3,
        );
        e.admit(now, Bytes::from_static(b"x"), &p);
        assert_eq!(e.evidence().configured_loss, 1);
        assert_eq!(e.evidence().duplicated_copies, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn cascading_duplication_has_unique_monotonic_copy_indices() {
        let now = Instant::now();
        let mut e =
            DatagramDirectionEngine::new(limits(), "p", 1, Direction::Upstream, RngVersion::V1)
                .unwrap();
        let p = policy(
            DatagramPlan::new(vec![
                spec(
                    "dup-a",
                    DatagramFaultKind::Duplicate {
                        additional_copies: 2,
                    },
                ),
                spec(
                    "dup-b",
                    DatagramFaultKind::Duplicate {
                        additional_copies: 1,
                    },
                ),
            ])
            .unwrap(),
            1,
        );
        let DatagramAdmission::Immediate(immediate) = e.admit(now, Bytes::from_static(b"x"), &p)
        else {
            panic!("zero-delay duplicates with an empty scheduler emit immediately");
        };
        let copies: Vec<_> = immediate.iter().map(|v| v.copy_index).collect();
        assert_eq!(copies, vec![0, 1, 2, 3, 4, 5]);
        assert!(e.take_ready(now).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn generation_snapshot_deadline_and_bounds_are_preserved() {
        let now = Instant::now();
        let mut e =
            DatagramDirectionEngine::new(limits(), "p", 1, Direction::Upstream, RngVersion::V1)
                .unwrap();
        let delayed = policy(
            DatagramPlan::new(vec![spec(
                "d",
                DatagramFaultKind::Delay {
                    delay: Duration::from_secs(2),
                    jitter: Duration::ZERO,
                },
            )])
            .unwrap(),
            1,
        );
        e.admit(now, Bytes::from_static(b"old"), &delayed);
        let ready = policy(DatagramPlan::empty(), 2);
        e.admit(now, Bytes::from_static(b"new"), &ready);
        let out = e.take_ready(now);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].generation, 2);
        tokio::time::advance(Duration::from_secs(2)).await;
        let out = e.take_ready(Instant::now());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].generation, 1);
        assert_eq!(e.evidence().high_water_datagrams, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn bandwidth_is_whole_datagram_and_queue_overflow_is_counted() {
        let now = Instant::now();
        let mut e = DatagramDirectionEngine::new(
            DatagramQueueLimits {
                max_queued_datagrams: NonZeroU64::new(1).unwrap(),
                max_queued_bytes: NonZeroU64::new(4).unwrap(),
                max_datagram_bytes: NonZeroU64::new(4).unwrap(),
            },
            "p",
            1,
            Direction::Upstream,
            RngVersion::V1,
        )
        .unwrap();
        let p = policy(
            DatagramPlan::new(vec![spec(
                "bw",
                DatagramFaultKind::Bandwidth {
                    bytes_per_second: NonZeroU64::new(2).unwrap(),
                    burst_bytes: NonZeroU64::new(2).unwrap(),
                },
            )])
            .unwrap(),
            1,
        );
        e.admit(now, Bytes::from_static(b"abcd"), &p);
        e.admit(now, Bytes::from_static(b"z"), &p);
        assert_eq!(e.evidence().queue_overflow, 1);
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(e.take_ready(Instant::now()).len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn delay_reorders_and_corruption_preserves_length_deterministically() {
        let now = Instant::now();
        let mut e =
            DatagramDirectionEngine::new(limits(), "p", 1, Direction::Upstream, RngVersion::V1)
                .unwrap();
        let plan = DatagramPlan::new(vec![
            spec(
                "delay",
                DatagramFaultKind::Delay {
                    delay: Duration::from_secs(2),
                    jitter: Duration::ZERO,
                },
            ),
            spec(
                "corrupt",
                DatagramFaultKind::PayloadCorrupt {
                    bytes: NonZeroU64::new(2).unwrap(),
                },
            ),
        ])
        .unwrap();
        e.admit(now, Bytes::from_static(b"slow"), &policy(plan, 1));
        e.admit(
            now,
            Bytes::from_static(b"fast"),
            &policy(DatagramPlan::empty(), 2),
        );
        assert_eq!(e.take_ready(now)[0].payload, Bytes::from_static(b"fast"));
        tokio::time::advance(Duration::from_secs(2)).await;
        let delayed = e.take_ready(Instant::now());
        assert_eq!(delayed[0].payload.len(), 4);
        assert_ne!(delayed[0].payload, Bytes::from_static(b"slow"));
        assert_eq!(e.evidence().corrupted_candidates, 1);
        assert_eq!(e.evidence().injected_delay_nanos, 2_000_000_000);
    }

    #[tokio::test(start_paused = true)]
    async fn heap_preserves_equal_and_mixed_deadline_ordering() {
        let now = Instant::now();
        let mut e =
            DatagramDirectionEngine::new(limits(), "p", 1, Direction::Upstream, RngVersion::V1)
                .unwrap();
        let hold = |secs| {
            policy(
                DatagramPlan::new(vec![spec(
                    "hold",
                    DatagramFaultKind::Delay {
                        delay: Duration::from_secs(secs),
                        jitter: Duration::ZERO,
                    },
                )])
                .unwrap(),
                1,
            )
        };
        // Same deadline: heap must drain in ingress-ordinal order.
        for ordinal in 0..8u64 {
            let mut payload = vec![0u8; 4];
            payload.copy_from_slice(&(ordinal as u32).to_be_bytes());
            assert!(matches!(
                e.admit(now, Bytes::from(payload), &hold(5)),
                DatagramAdmission::Queued
            ));
        }
        assert_eq!(e.next_deadline(), Some(now + Duration::from_secs(5)));
        assert!(e.take_ready(now).is_empty());
        tokio::time::advance(Duration::from_secs(5)).await;
        let drained = e.take_ready(Instant::now());
        assert_eq!(drained.len(), 8);
        for (index, item) in drained.iter().enumerate() {
            assert_eq!(item.ingress_ordinal, index as u64);
            assert_eq!(item.copy_index, 0);
        }
        // Mixed deadlines: earlier release drains first regardless of ingress.
        assert!(matches!(
            e.admit(Instant::now(), Bytes::from_static(b"slow"), &hold(2)),
            DatagramAdmission::Queued
        ));
        assert!(matches!(
            e.admit(Instant::now(), Bytes::from_static(b"fast"), &hold(1)),
            DatagramAdmission::Queued
        ));
        tokio::time::advance(Duration::from_secs(1)).await;
        let first = e.take_ready(Instant::now());
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].payload, Bytes::from_static(b"fast"));
        tokio::time::advance(Duration::from_secs(1)).await;
        let second = e.take_ready(Instant::now());
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].payload, Bytes::from_static(b"slow"));
    }

    #[tokio::test(start_paused = true)]
    async fn heap_enforces_count_and_byte_overflow_identically() {
        let now = Instant::now();
        let mut e = DatagramDirectionEngine::new(
            DatagramQueueLimits {
                max_queued_datagrams: NonZeroU64::new(2).unwrap(),
                max_queued_bytes: NonZeroU64::new(3).unwrap(),
                max_datagram_bytes: NonZeroU64::new(8).unwrap(),
            },
            "p",
            1,
            Direction::Upstream,
            RngVersion::V1,
        )
        .unwrap();
        let delayed = policy(
            DatagramPlan::new(vec![spec(
                "d",
                DatagramFaultKind::Delay {
                    delay: Duration::from_secs(60),
                    jitter: Duration::ZERO,
                },
            )])
            .unwrap(),
            1,
        );
        assert!(matches!(
            e.admit(now, Bytes::from_static(b"ab"), &delayed),
            DatagramAdmission::Queued
        ));
        // Two queued bytes plus two more exceed the three-byte bound.
        assert!(matches!(
            e.admit(now, Bytes::from_static(b"cd"), &delayed),
            DatagramAdmission::Consumed
        ));
        assert_eq!(e.evidence().queue_overflow, 1);
        assert_eq!(e.evidence().queued_datagrams, 1);
        assert_eq!(e.evidence().queued_bytes, 2);
        tokio::time::advance(Duration::from_secs(61)).await;
        let drained = e.take_ready(Instant::now());
        assert_eq!(drained.len(), 1);
        assert_eq!(e.evidence().queued_datagrams, 0);
        assert_eq!(e.evidence().queued_bytes, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn empty_plan_immediate_path_matches_queued_reference_evidence() {
        let now = Instant::now();
        let immediate_policy = policy(DatagramPlan::empty(), 9);
        let mut immediate =
            DatagramDirectionEngine::new(limits(), "p", 1, Direction::Upstream, RngVersion::V1)
                .unwrap();
        let DatagramAdmission::Immediate(first) =
            immediate.admit(now, Bytes::from_static(b"abcd"), &immediate_policy)
        else {
            panic!("empty plan with an empty scheduler emits immediately");
        };
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].ingress_ordinal, 0);
        assert_eq!(first[0].copy_index, 0);
        assert_eq!(first[0].generation, 9);
        let evidence = immediate.evidence().clone();
        assert_eq!(evidence.admitted_datagrams, 1);
        assert_eq!(evidence.admitted_bytes, 4);
        assert_eq!(evidence.emitted_datagrams, 1);
        assert_eq!(evidence.emitted_bytes, 4);
        assert_eq!(evidence.queued_datagrams, 0);
        assert_eq!(evidence.queued_bytes, 0);
        assert_eq!(evidence.high_water_datagrams, 1);
        assert_eq!(evidence.high_water_bytes, 4);
        assert_eq!(evidence.last_generation, 9);
        assert_eq!(evidence.last_seed_namespace, 7);
        assert_eq!(evidence.configured_loss, 0);
        assert_eq!(evidence.queue_overflow, 0);
        // Queue-then-drain reference: hold one delayed candidate so the empty
        // plan takes the scheduler path, then drain everything.
        let mut queued =
            DatagramDirectionEngine::new(limits(), "p", 1, Direction::Upstream, RngVersion::V1)
                .unwrap();
        let delayed = policy(
            DatagramPlan::new(vec![spec(
                "d",
                DatagramFaultKind::Delay {
                    delay: Duration::from_secs(30),
                    jitter: Duration::ZERO,
                },
            )])
            .unwrap(),
            9,
        );
        assert!(matches!(
            queued.admit(now, Bytes::from_static(b"held"), &delayed),
            DatagramAdmission::Queued
        ));
        assert!(matches!(
            queued.admit(now, Bytes::from_static(b"abcd"), &immediate_policy),
            DatagramAdmission::Queued
        ));
        let drained = queued.take_ready(now);
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].payload, Bytes::from_static(b"abcd"));
        let reference = queued.evidence();
        assert_eq!(reference.admitted_datagrams, 2);
        assert_eq!(reference.emitted_datagrams, 1);
        assert_eq!(reference.queued_datagrams, 1);
        // The immediately emitted datagram carries identical identity and
        // generation to its queued-then-drained counterpart.
        assert_eq!(drained[0].ingress_ordinal, 1);
        assert_eq!(drained[0].generation, 9);
    }

    #[tokio::test(start_paused = true)]
    async fn empty_plan_overflow_and_oversize_are_consumed_not_queued() {
        let now = Instant::now();
        let mut e = DatagramDirectionEngine::new(
            DatagramQueueLimits {
                max_queued_datagrams: NonZeroU64::new(8).unwrap(),
                max_queued_bytes: NonZeroU64::new(2).unwrap(),
                max_datagram_bytes: NonZeroU64::new(8).unwrap(),
            },
            "p",
            1,
            Direction::Upstream,
            RngVersion::V1,
        )
        .unwrap();
        let empty = policy(DatagramPlan::empty(), 1);
        // Three bytes exceed the two-byte queue bound while fitting the
        // per-datagram bound: consumed as overflow, never queued.
        assert!(matches!(
            e.admit(now, Bytes::from_static(b"abc"), &empty),
            DatagramAdmission::Consumed
        ));
        assert_eq!(e.evidence().queue_overflow, 1);
        assert_eq!(e.evidence().queued_datagrams, 0);
        assert!(e.next_deadline().is_none());
        assert!(matches!(
            e.admit(now, Bytes::from_static(b"way-too-long-payload"), &empty),
            DatagramAdmission::Consumed
        ));
        assert_eq!(e.evidence().oversize_datagrams, 1);
        // A non-empty full scheduler still enforces bounds on the empty-plan
        // queued path: the newcomer overflows instead of exceeding the cap.
        let mut full = DatagramDirectionEngine::new(
            DatagramQueueLimits {
                max_queued_datagrams: NonZeroU64::new(1).unwrap(),
                max_queued_bytes: NonZeroU64::new(1024).unwrap(),
                max_datagram_bytes: NonZeroU64::new(64).unwrap(),
            },
            "p",
            1,
            Direction::Upstream,
            RngVersion::V1,
        )
        .unwrap();
        let delayed = policy(
            DatagramPlan::new(vec![spec(
                "d",
                DatagramFaultKind::Delay {
                    delay: Duration::from_secs(60),
                    jitter: Duration::ZERO,
                },
            )])
            .unwrap(),
            1,
        );
        assert!(matches!(
            full.admit(now, Bytes::from_static(b"held"), &delayed),
            DatagramAdmission::Queued
        ));
        assert!(matches!(
            full.admit(now, Bytes::from_static(b"extra"), &empty),
            DatagramAdmission::Consumed
        ));
        assert_eq!(full.evidence().queue_overflow, 1);
        assert_eq!(full.evidence().queued_datagrams, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn high_queue_depth_drain_stays_ordered_and_bounded() {
        let now = Instant::now();
        let mut e = DatagramDirectionEngine::new(
            DatagramQueueLimits {
                max_queued_datagrams: NonZeroU64::new(4096).unwrap(),
                max_queued_bytes: NonZeroU64::new(4 * 1024 * 1024).unwrap(),
                max_datagram_bytes: NonZeroU64::new(64).unwrap(),
            },
            "p",
            1,
            Direction::Upstream,
            RngVersion::V1,
        )
        .unwrap();
        let delayed = policy(
            DatagramPlan::new(vec![spec(
                "d",
                DatagramFaultKind::Delay {
                    delay: Duration::from_secs(60),
                    jitter: Duration::ZERO,
                },
            )])
            .unwrap(),
            1,
        );
        for _ in 0..1024 {
            assert!(matches!(
                e.admit(now, Bytes::from_static(b"depth"), &delayed),
                DatagramAdmission::Queued
            ));
        }
        assert_eq!(e.evidence().queued_datagrams, 1024);
        assert_eq!(e.evidence().high_water_datagrams, 1024);
        // A not-ready drain must not disturb the heap or disturb evidence.
        assert!(e.take_ready(now).is_empty());
        assert_eq!(e.evidence().queued_datagrams, 1024);
        tokio::time::advance(Duration::from_secs(61)).await;
        let drained = e.take_ready(Instant::now());
        assert_eq!(drained.len(), 1024);
        for (index, item) in drained.iter().enumerate() {
            assert_eq!(item.ingress_ordinal, index as u64);
        }
        assert_eq!(e.evidence().queued_datagrams, 0);
        assert_eq!(e.evidence().emitted_datagrams, 1024);
    }
}
