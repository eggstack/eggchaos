//! Bounded, deterministic, protocol-neutral datagram impairment.
use std::{
    collections::{HashMap, HashSet},
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
#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: u64,
    updated: Instant,
}

/// Single-owner deterministic direction engine. Call `admit` in ingress order and
/// `take_ready` from the embedding runtime's timer/select loop.
#[derive(Debug)]
pub struct DatagramDirectionEngine {
    limits: DatagramQueueLimits,
    proxy: String,
    association: u64,
    direction: Direction,
    rng_version: RngVersion,
    ordinal: u64,
    queue: Vec<Candidate>,
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
            queue: Vec::new(),
            evidence,
            buckets: HashMap::new(),
        })
    }
    pub fn evidence(&self) -> &DatagramEvidence {
        &self.evidence
    }
    pub fn next_deadline(&self) -> Option<Instant> {
        self.queue.iter().map(|x| x.at).min()
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
    pub fn admit(&mut self, now: Instant, payload: Bytes, policy: &PublishedDatagramPolicy) {
        let ordinal = self.ordinal;
        self.ordinal = self.ordinal.saturating_add(1);
        self.evidence.admitted_datagrams = self.evidence.admitted_datagrams.saturating_add(1);
        self.evidence.admitted_bytes = self
            .evidence
            .admitted_bytes
            .saturating_add(payload.len() as u64);
        if payload.len() as u64 > self.limits.max_datagram_bytes.get() {
            self.evidence.oversize_datagrams = self.evidence.oversize_datagrams.saturating_add(1);
            return;
        }
        let mut candidates = vec![(0u16, payload, Duration::ZERO)];
        let mut next_copy_index = 1u16;
        for (stage, fault) in policy.plan.faults().iter().enumerate() {
            let mut next = Vec::new();
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
        self.evidence.last_generation = policy.generation;
        self.evidence.last_seed_namespace = policy.seed_namespace;
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
            self.evidence.queued_datagrams += 1;
            self.evidence.queued_bytes += bytes.len() as u64;
            self.evidence.high_water_datagrams = self
                .evidence
                .high_water_datagrams
                .max(self.evidence.queued_datagrams);
            self.evidence.high_water_bytes = self
                .evidence
                .high_water_bytes
                .max(self.evidence.queued_bytes);
            self.queue.push(Candidate {
                at: now + delay,
                ordinal,
                copy,
                payload: bytes,
                generation: policy.generation,
            });
        }
    }

    fn bandwidth_delay(
        &mut self,
        id: &str,
        now: Instant,
        bytes: u64,
        rate: u64,
        burst: u64,
    ) -> Duration {
        let bucket = self.buckets.entry(id.to_owned()).or_insert(Bucket {
            tokens: burst,
            updated: now,
        });
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

    /// Remove all currently due candidates in deterministic deadline/identity order.
    pub fn take_ready(&mut self, now: Instant) -> Vec<DatagramScheduled> {
        self.queue.sort_by_key(|x| (x.at, x.ordinal, x.copy));
        let split = self.queue.partition_point(|x| x.at <= now);
        let ready: Vec<_> = self.queue.drain(..split).collect();
        let mut out = Vec::with_capacity(ready.len());
        for x in ready {
            self.evidence.queued_datagrams -= 1;
            self.evidence.queued_bytes -= x.payload.len() as u64;
            self.evidence.emitted_datagrams += 1;
            self.evidence.emitted_bytes += x.payload.len() as u64;
            out.push(DatagramScheduled {
                payload: x.payload,
                ingress_ordinal: x.ordinal,
                copy_index: x.copy,
                generation: x.generation,
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
        e.admit(now, Bytes::from_static(b"x"), &p);
        let copies: Vec<_> = e.take_ready(now).iter().map(|v| v.copy_index).collect();
        assert_eq!(copies, vec![0, 1, 2, 3, 4, 5]);
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
}
