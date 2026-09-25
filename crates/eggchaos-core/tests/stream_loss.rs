//! M036 deterministic stream-loss corpus: golden vectors, fragmentation
//! equivalence, burst correlation, multi-fault composition, existing-fault
//! interaction, bounds/ownership, generation replacement, and evidence
//! reconciliation.
//!
//! Stream loss is userspace logical-chunk loss, not IP/TCP packet loss. Every
//! test below keys decisions to the absolute accepted stream offset in fixed
//! 32 KiB grains; no test may depend on caller write fragmentation, Tokio
//! scheduling, wall time, or process-global RNG.

use std::{
    collections::HashMap,
    future::{poll_fn, Future},
    num::NonZeroU64,
    pin::Pin,
    task::Poll,
    time::Duration,
};

use eggchaos_core::{
    ChaosStream, Direction, DirectionEngine, FaultId, FaultKind, FaultPlan, FaultSpec, Probability,
    StreamLossConfig, FAULT_TYPE_NAMES, STREAM_LOSS_GRAIN_BYTES, STREAM_LOSS_TYPE_NAME,
};
use proptest::prelude::*;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};

const GRAIN: usize = STREAM_LOSS_GRAIN_BYTES as usize;
const SEED: u64 = 7;

fn loss(id: &str, rate: f64, correlation: f64) -> FaultSpec {
    FaultSpec {
        id: FaultId::new(id).unwrap(),
        probability: Probability::new(1.0).unwrap(),
        kind: FaultKind::StreamLoss(StreamLossConfig {
            loss_rate: Probability::new(rate).unwrap(),
            correlation: Probability::new(correlation).unwrap(),
        }),
    }
}

fn loss_prob(id: &str, probability: f64, rate: f64, correlation: f64) -> FaultSpec {
    FaultSpec {
        id: FaultId::new(id).unwrap(),
        probability: Probability::new(probability).unwrap(),
        kind: FaultKind::StreamLoss(StreamLossConfig {
            loss_rate: Probability::new(rate).unwrap(),
            correlation: Probability::new(correlation).unwrap(),
        }),
    }
}

fn engine_with_seed(plan: FaultPlan, seed: u64) -> DirectionEngine {
    DirectionEngine::new(plan, seed, "proxy", 41, Direction::Upstream).unwrap()
}

fn engine(plan: FaultPlan) -> DirectionEngine {
    engine_with_seed(plan, SEED)
}

/// Input where chunk `i` is filled with byte `i`: forwarded content then
/// records exactly which logical chunks survived.
fn chunked_input(chunks: usize) -> Vec<u8> {
    assert!(chunks <= 256, "chunk marker bytes must stay unique");
    let mut out = Vec::with_capacity(chunks * GRAIN);
    for chunk in 0..chunks {
        out.extend(std::iter::repeat_n(chunk as u8, GRAIN));
    }
    out
}

/// Count surviving bytes per chunk marker in forwarded content.
fn surviving_counts(forwarded: &[u8]) -> HashMap<u8, usize> {
    let mut counts = HashMap::new();
    for byte in forwarded {
        *counts.entry(*byte).or_insert(0) += 1;
    }
    counts
}

/// Every chunk must decide atomically: fully preserved or fully dropped.
fn assert_chunk_atomic(forwarded: &[u8], chunks: usize) {
    let counts = surviving_counts(forwarded);
    assert!(counts.len() <= chunks);
    for (marker, count) in &counts {
        assert_eq!(
            *count, GRAIN,
            "chunk {marker} must decide atomically, got {count} bytes"
        );
    }
}

struct TrafficOutcome {
    forwarded: Vec<u8>,
    summary: eggchaos_core::DirectionSummary,
}

/// Drive user fragments through a pumped stream: a background reader drains
/// the peer so writes never deadlock on bounded capacity, then flush and a
/// graceful shutdown deliver every survivor.
async fn run_traffic(plan: FaultPlan, seed: u64, fragments: &[Vec<u8>]) -> TrafficOutcome {
    let (peer, right) = tokio::io::duplex(4 << 20);
    let mut wrapped =
        ChaosStream::new(right, plan, seed, "proxy", 41, Direction::Upstream).unwrap();
    let reader = tokio::spawn(async move {
        let mut peer = peer;
        let mut out = Vec::new();
        peer.read_to_end(&mut out).await.expect("peer read");
        out
    });
    for fragment in fragments {
        wrapped.write_all(fragment).await.expect("write fragment");
    }
    wrapped.flush().await.expect("flush survivors");
    wrapped.shutdown().await.expect("graceful shutdown");
    let forwarded = reader.await.expect("reader task");
    let summary = wrapped.summary();
    TrafficOutcome { forwarded, summary }
}

fn assert_accounting(summary: &eggchaos_core::DirectionSummary) {
    assert_eq!(
        summary.bytes_accepted,
        summary
            .bytes_forwarded
            .saturating_add(summary.bytes_discarded)
            .saturating_add(summary.buffered_bytes),
        "accepted = forwarded + discarded + buffered"
    );
}

// ---------------------------------------------------------------------------
// WP1: frozen types, constants, validation
// ---------------------------------------------------------------------------

#[test]
fn stream_loss_names_and_grain_are_frozen() {
    assert_eq!(STREAM_LOSS_GRAIN_BYTES, 32 * 1024);
    assert_eq!(GRAIN, 32768);
    assert_eq!(STREAM_LOSS_TYPE_NAME, "stream-loss");
    // The seven-slot legacy contract is untouched.
    assert_eq!(FAULT_TYPE_NAMES.len(), 7);
    assert_eq!(
        FAULT_TYPE_NAMES,
        [
            "latency",
            "bandwidth",
            "blackhole",
            "limit-data",
            "slow-close",
            "slice",
            "disconnect"
        ]
    );
    let loss_kind = loss("l", 0.2, 0.4).kind;
    assert_eq!(loss_kind.type_name(), "stream-loss");
    assert_eq!(loss_kind.type_index(), None);
    assert_eq!(
        FaultKind::Blackhole(eggchaos_core::BlackholeConfig { close_after: None }).type_index(),
        Some(2)
    );
}

#[test]
fn stream_loss_validation_rejects_out_of_range_probabilities() {
    assert!(Probability::new(0.0).is_ok());
    assert!(Probability::new(1.0).is_ok());
    assert!(Probability::new(f64::NAN).is_err());
    assert!(Probability::new(f64::INFINITY).is_err());
    assert!(Probability::new(-0.25).is_err());
    assert!(Probability::new(1.25).is_err());
    // Deserialized values bypass `Probability::new`, so plan validation must
    // still reject them.
    let invalid = serde_json::json!({
        "faults": [{
            "id": "loss",
            "probability": 1.0,
            "kind": {"StreamLoss": {"loss_rate": 1.5, "correlation": 0.0}}
        }]
    });
    let plan: FaultPlan = serde_json::from_value(invalid).expect("shape decodes");
    assert_eq!(
        plan.validate(),
        Err(eggchaos_core::ValidationError::ProbabilityOutOfRange)
    );
    assert!(FaultPlan::new(vec![loss("ok", 0.0, 1.0)]).is_ok());
}

#[test]
fn stream_loss_json_round_trip_is_exact() {
    let plan = FaultPlan::new(vec![loss("loss", 0.2, 0.4)]).unwrap();
    let value = serde_json::to_value(&plan).unwrap();
    let decoded: FaultPlan = serde_json::from_value(value).unwrap();
    assert_eq!(decoded, plan);
}

// ---------------------------------------------------------------------------
// Edge cases: rate 0 preserves, rate 1 discards without retaining payload
// ---------------------------------------------------------------------------

#[tokio::test]
async fn loss_rate_zero_preserves_every_byte() {
    let input = chunked_input(4);
    let outcome = run_traffic(
        FaultPlan::new(vec![loss("loss", 0.0, 0.0)]).unwrap(),
        SEED,
        std::slice::from_ref(&input),
    )
    .await;
    assert_eq!(outcome.forwarded, input);
    let summary = outcome.summary;
    assert_eq!(summary.bytes_discarded, 0);
    assert_eq!(summary.stream_loss_bytes_discarded, 0);
    assert_eq!(summary.stream_loss_chunks_evaluated, 4);
    assert_eq!(summary.stream_loss_chunks_dropped, 0);
    assert_eq!(summary.activations, [0; 7]);
    assert_accounting(&summary);
}

#[tokio::test]
async fn loss_rate_one_discards_everything_without_retaining_payload() {
    let input = chunked_input(3);
    let outcome = run_traffic(
        FaultPlan::new(vec![loss("loss", 1.0, 0.0)]).unwrap(),
        SEED,
        std::slice::from_ref(&input),
    )
    .await;
    assert!(outcome.forwarded.is_empty());
    let summary = outcome.summary;
    assert_eq!(summary.bytes_accepted, input.len() as u64);
    assert_eq!(summary.bytes_discarded, input.len() as u64);
    assert_eq!(summary.stream_loss_bytes_discarded, input.len() as u64);
    assert_eq!(summary.stream_loss_chunks_evaluated, 3);
    assert_eq!(summary.stream_loss_chunks_dropped, 3);
    assert_eq!(summary.buffered_bytes, 0);
    assert_eq!(summary.high_water_bytes, 0);
    assert_eq!(summary.activations, [0; 7]);
    assert_accounting(&summary);
}

// ---------------------------------------------------------------------------
// Grain boundaries and fragmentation independence
// ---------------------------------------------------------------------------

#[test]
fn fixed_grain_boundary_cases_classify_exactly() {
    // rate 1 classifies without touching the bounded queue: every accepted
    // byte is a discard, so boundaries are exact with no saturation.
    let mut engine = engine(FaultPlan::new(vec![loss("loss", 1.0, 0.0)]).unwrap());
    assert_eq!(engine.accept(&vec![0xAA; 32767]), 32767);
    assert_eq!(engine.evidence().stream_loss_chunks_evaluated, 1);
    assert_eq!(engine.accept(&[0xAA; 1]), 1);
    assert_eq!(engine.evidence().stream_loss_chunks_evaluated, 1);
    assert_eq!(engine.accept(&[0xAA; 1]), 1);
    assert_eq!(engine.evidence().stream_loss_chunks_evaluated, 2);
    assert_eq!(engine.accept(&vec![0xAA; 32768]), 32768);
    let evidence = engine.evidence();
    assert_eq!(evidence.bytes_accepted, 32767 + 1 + 1 + 32768);
    assert_eq!(evidence.bytes_discarded, evidence.bytes_accepted);
    assert_eq!(evidence.stream_loss_chunks_evaluated, 3);
    assert_eq!(evidence.stream_loss_chunks_dropped, 3);
    assert_eq!(evidence.buffered_bytes, 0);
}

#[tokio::test]
async fn identical_stream_under_any_fragmentation_decides_identically() {
    let plan = || FaultPlan::new(vec![loss("loss", 0.3, 0.2)]).unwrap();
    let input = chunked_input(3);
    let whole = run_traffic(plan(), SEED, std::slice::from_ref(&input)).await;
    let mut kilobytes = Vec::new();
    for chunk in input.chunks(1024) {
        kilobytes.push(chunk.to_vec());
    }
    let fragmented = run_traffic(plan(), SEED, &kilobytes).await;
    let mut bytes = Vec::new();
    for byte in input.iter() {
        bytes.push(vec![*byte]);
    }
    let byte_at_a_time = run_traffic(plan(), SEED, &bytes).await;
    assert_eq!(whole.forwarded, fragmented.forwarded);
    assert_eq!(whole.forwarded, byte_at_a_time.forwarded);
    for outcome in [&whole, &fragmented, &byte_at_a_time] {
        assert_accounting(&outcome.summary);
    }
    assert_eq!(
        whole.summary.bytes_accepted,
        fragmented.summary.bytes_accepted
    );
    assert_eq!(
        whole.summary.bytes_discarded,
        fragmented.summary.bytes_discarded
    );
    assert_eq!(
        whole.summary.stream_loss_chunks_evaluated,
        fragmented.summary.stream_loss_chunks_evaluated
    );
    assert_eq!(
        whole.summary.stream_loss_chunks_dropped,
        fragmented.summary.stream_loss_chunks_dropped
    );
    assert_eq!(
        whole.summary.stream_loss_bytes_discarded,
        fragmented.summary.stream_loss_bytes_discarded
    );
    // Chunks decide atomically: every chunk is all preserved or all dropped.
    let counts = surviving_counts(&whole.forwarded);
    for (marker, count) in counts {
        assert_eq!(count, GRAIN, "chunk {marker} must decide atomically");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn randomized_fragmentation_preserves_loss_outcomes(
        cuts in prop::collection::vec(0..(3 * GRAIN), 0..8usize)
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let plan = || FaultPlan::new(vec![loss("loss", 0.3, 0.2)]).unwrap();
        let input = chunked_input(3);
        let mut boundaries: Vec<usize> = cuts.into_iter().filter(|c| *c < input.len()).collect();
        boundaries.sort_unstable();
        boundaries.dedup();
        let mut fragments = Vec::new();
        let mut start = 0;
        for cut in boundaries {
            if cut > start {
                fragments.push(input[start..cut].to_vec());
                start = cut;
            }
        }
        if start < input.len() {
            fragments.push(input[start..].to_vec());
        }
        let (whole, split) = runtime.block_on(async {
            let whole = run_traffic(plan(), SEED, std::slice::from_ref(&input)).await;
            let split = run_traffic(plan(), SEED, &fragments).await;
            (whole, split)
        });
        prop_assert_eq!(whole.forwarded, split.forwarded);
        prop_assert_eq!(whole.summary.bytes_discarded, split.summary.bytes_discarded);
        prop_assert_eq!(
            whole.summary.stream_loss_chunks_dropped,
            split.summary.stream_loss_chunks_dropped
        );
    }
}

// ---------------------------------------------------------------------------
// Burst correlation
// ---------------------------------------------------------------------------

/// Probe which of the first two chunks survive for a seed/config pair.
async fn probe_two_chunks(seed: u64, rate: f64, correlation: f64) -> Vec<bool> {
    let outcome = run_traffic(
        FaultPlan::new(vec![loss("loss", rate, correlation)]).unwrap(),
        seed,
        &[chunked_input(2)],
    )
    .await;
    let counts = surviving_counts(&outcome.forwarded);
    (0..2)
        .map(|chunk| {
            counts
                .get(&(chunk as u8))
                .is_some_and(|count| *count == GRAIN)
        })
        .collect()
}

async fn find_seed(rate: f64, correlation: f64, want: &[bool]) -> u64 {
    for seed in 0..10_000u64 {
        if probe_two_chunks(seed, rate, correlation).await == want {
            return seed;
        }
    }
    panic!("no seed in 0..10000 produced chunk pattern {want:?}");
}

#[tokio::test]
async fn correlation_zero_gives_a_deterministic_independent_baseline() {
    let first = probe_two_chunks(SEED, 0.5, 0.0).await;
    // Same identity replays the same decisions.
    assert_eq!(probe_two_chunks(SEED, 0.5, 0.0).await, first);
    // A different seed namespace decides differently somewhere in a wider
    // sweep (the streams are genuinely seed-dependent, not constant).
    let mut differs = false;
    for seed in [1u64, 2, 3, 11, 101, 2024] {
        if probe_two_chunks(seed, 0.5, 0.0).await != first {
            differs = true;
            break;
        }
    }
    assert!(differs, "loss decisions must depend on the seed namespace");
}

#[tokio::test]
async fn correlation_one_sticks_bursts_as_a_suffix() {
    // With correlation 1, the first drop locks every later chunk to drop:
    // survivors always form a (possibly empty) chunk prefix.
    for seed in [SEED, 1, 2, 3, 11, 101, 2024] {
        let outcome = run_traffic(
            FaultPlan::new(vec![loss("loss", 0.5, 1.0)]).unwrap(),
            seed,
            &[chunked_input(6)],
        )
        .await;
        let counts = surviving_counts(&outcome.forwarded);
        let mut seen_drop = false;
        for chunk in 0..6u8 {
            match counts.get(&chunk) {
                Some(count) => {
                    assert_eq!(*count, GRAIN, "chunk {chunk} must decide atomically");
                    assert!(
                        !seen_drop,
                        "correlation 1 must never preserve chunk {chunk} after a drop"
                    );
                }
                None => seen_drop = true,
            }
        }
        assert_accounting(&outcome.summary);
    }
}

// ---------------------------------------------------------------------------
// Connection-level activation and RNG isolation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn connection_probability_zero_never_activates_and_one_always_does() {
    let input = chunked_input(2);
    let inactive = run_traffic(
        FaultPlan::new(vec![loss_prob("loss", 0.0, 1.0, 0.0)]).unwrap(),
        SEED,
        std::slice::from_ref(&input),
    )
    .await;
    assert_eq!(inactive.forwarded, input);
    assert_eq!(inactive.summary.stream_loss_chunks_evaluated, 0);
    let active = run_traffic(
        FaultPlan::new(vec![loss_prob("loss", 1.0, 1.0, 0.0)]).unwrap(),
        SEED,
        std::slice::from_ref(&input),
    )
    .await;
    assert!(active.forwarded.is_empty());
    assert_eq!(active.summary.stream_loss_chunks_evaluated, 2);
}

#[tokio::test]
async fn unrelated_fault_addition_does_not_perturb_loss_decisions() {
    // The loss chunk RNG stream is fault-local and domain-separated from
    // activation draws and per-segment draws, so appending a latency fault
    // leaves every loss decision (and every loss counter) unchanged.
    let input = chunked_input(3);
    let loss_only = run_traffic(
        FaultPlan::new(vec![loss("loss", 0.3, 0.2)]).unwrap(),
        SEED,
        std::slice::from_ref(&input),
    )
    .await;
    let with_latency = run_traffic(
        FaultPlan::new(vec![
            loss("loss", 0.3, 0.2),
            FaultSpec {
                id: FaultId::new("latency").unwrap(),
                probability: Probability::new(1.0).unwrap(),
                kind: FaultKind::Latency(eggchaos_core::LatencyConfig {
                    delay: Duration::ZERO,
                    jitter: Duration::ZERO,
                    max_buffer_bytes: NonZeroU64::new(1024).unwrap(),
                }),
            },
        ])
        .unwrap(),
        SEED,
        std::slice::from_ref(&input),
    )
    .await;
    assert_eq!(loss_only.forwarded, with_latency.forwarded);
    assert_eq!(
        loss_only.summary.stream_loss_chunks_evaluated,
        with_latency.summary.stream_loss_chunks_evaluated
    );
    assert_eq!(
        loss_only.summary.stream_loss_chunks_dropped,
        with_latency.summary.stream_loss_chunks_dropped
    );
    assert_eq!(
        loss_only.summary.stream_loss_bytes_discarded,
        with_latency.summary.stream_loss_bytes_discarded
    );
}

// ---------------------------------------------------------------------------
// Multiple stream-loss faults: frozen union composition
// ---------------------------------------------------------------------------

#[tokio::test]
async fn two_loss_faults_compose_by_union_without_double_counting() {
    let input = chunked_input(4);
    let plan_a = FaultPlan::new(vec![loss("a", 0.3, 0.0)]).unwrap();
    let plan_b = FaultPlan::new(vec![loss("b", 0.4, 0.1)]).unwrap();
    let plan_both = FaultPlan::new(vec![loss("a", 0.3, 0.0), loss("b", 0.4, 0.1)]).unwrap();
    let a = run_traffic(plan_a, SEED, std::slice::from_ref(&input)).await;
    let b = run_traffic(plan_b, SEED, std::slice::from_ref(&input)).await;
    let both = run_traffic(plan_both, SEED, std::slice::from_ref(&input)).await;
    let set = |outcome: &TrafficOutcome| {
        assert_chunk_atomic(&outcome.forwarded, 4);
        surviving_counts(&outcome.forwarded)
            .into_keys()
            .collect::<std::collections::HashSet<_>>()
    };
    let (set_a, set_b, set_both) = (set(&a), set(&b), set(&both));
    // A byte range survives only when no active loss fault drops it.
    assert_eq!(set_both, set_a.intersection(&set_b).cloned().collect());
    // Aggregate evidence never double-counts shared discards.
    assert_eq!(
        both.summary.bytes_discarded,
        both.summary.stream_loss_bytes_discarded
    );
    assert_eq!(
        both.summary.bytes_accepted,
        both.summary.bytes_forwarded + both.summary.bytes_discarded
    );
    assert_eq!(both.summary.stream_loss_chunks_evaluated, 4);
    assert_accounting(&both.summary);
}

// ---------------------------------------------------------------------------
// Composition with every existing fault family
// ---------------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn loss_plus_latency_delays_only_survivors() {
    let plan = FaultPlan::new(vec![
        loss("loss", 0.4, 0.0),
        FaultSpec {
            id: FaultId::new("latency").unwrap(),
            probability: Probability::new(1.0).unwrap(),
            kind: FaultKind::Latency(eggchaos_core::LatencyConfig {
                delay: Duration::from_millis(50),
                jitter: Duration::ZERO,
                max_buffer_bytes: NonZeroU64::new(1 << 20).unwrap(),
            }),
        },
    ])
    .unwrap();
    let loss_only = run_traffic(
        FaultPlan::new(vec![loss("loss", 0.4, 0.0)]).unwrap(),
        SEED,
        &[chunked_input(2)],
    )
    .await;
    let (peer, right) = tokio::io::duplex(4 << 20);
    let mut wrapped =
        ChaosStream::new(right, plan, SEED, "proxy", 41, Direction::Upstream).unwrap();
    wrapped.write_all(&chunked_input(2)).await.unwrap();
    // Survivors are held for the latency deadline; drops already resolved.
    let summary = wrapped.summary();
    assert_eq!(summary.bytes_discarded, loss_only.summary.bytes_discarded);
    assert_eq!(summary.buffered_bytes, loss_only.summary.bytes_forwarded);
    tokio::time::advance(Duration::from_millis(50)).await;
    wrapped.flush().await.unwrap();
    wrapped.shutdown().await.unwrap();
    let mut forwarded = Vec::new();
    let mut peer = peer;
    peer.read_to_end(&mut forwarded).await.unwrap();
    assert_eq!(forwarded, loss_only.forwarded);
    let summary = wrapped.summary();
    assert!(summary.injected_delay_ms > 0 || loss_only.forwarded.is_empty());
    assert_accounting(&summary);
}

#[tokio::test(start_paused = true)]
async fn loss_plus_bandwidth_throttles_only_survivors() {
    let plan = FaultPlan::new(vec![
        loss("loss", 0.4, 0.0),
        FaultSpec {
            id: FaultId::new("throttle").unwrap(),
            probability: Probability::new(1.0).unwrap(),
            kind: FaultKind::Bandwidth(eggchaos_core::BandwidthConfig {
                bytes_per_second: NonZeroU64::new(1024).unwrap(),
                burst_bytes: NonZeroU64::new(1024).unwrap(),
            }),
        },
    ])
    .unwrap();
    let loss_only = run_traffic(
        FaultPlan::new(vec![loss("loss", 0.4, 0.0)]).unwrap(),
        SEED,
        &[chunked_input(2)],
    )
    .await;
    let (peer, right) = tokio::io::duplex(4 << 20);
    let mut wrapped =
        ChaosStream::new(right, plan, SEED, "proxy", 41, Direction::Upstream).unwrap();
    let reader = tokio::spawn(async move {
        let mut peer = peer;
        let mut out = Vec::new();
        peer.read_to_end(&mut out).await.unwrap();
        out
    });
    wrapped.write_all(&chunked_input(2)).await.unwrap();
    // Drain the throttle under paused time: each advance refills the bucket
    // and each single poll_flush call makes whatever progress is due.
    for _ in 0..400 {
        tokio::time::advance(Duration::from_millis(500)).await;
        let done = poll_fn(|cx| match Pin::new(&mut wrapped).poll_flush(cx) {
            Poll::Ready(result) => Poll::Ready(Some(result)),
            Poll::Pending => Poll::Ready(None),
        })
        .await;
        if let Some(result) = done {
            result.unwrap();
            break;
        }
    }
    wrapped.shutdown().await.unwrap();
    let forwarded = reader.await.unwrap();
    assert_eq!(forwarded, loss_only.forwarded);
    let summary = wrapped.summary();
    assert_accounting(&summary);
}

#[tokio::test]
async fn loss_plus_slicer_slices_survivors_without_moving_grain() {
    let slicer = FaultSpec {
        id: FaultId::new("slicer").unwrap(),
        probability: Probability::new(1.0).unwrap(),
        kind: FaultKind::Slice(eggchaos_core::SliceConfig {
            average_size: NonZeroU64::new(1024).unwrap(),
            variation: 0,
            delay: Duration::ZERO,
        }),
    };
    let input = chunked_input(3);
    let loss_only = run_traffic(
        FaultPlan::new(vec![loss("loss", 0.3, 0.2)]).unwrap(),
        SEED,
        std::slice::from_ref(&input),
    )
    .await;
    let with_slicer = run_traffic(
        FaultPlan::new(vec![loss("loss", 0.3, 0.2), slicer]).unwrap(),
        SEED,
        std::slice::from_ref(&input),
    )
    .await;
    // Slicer boundaries never change chunk identity: same survivors.
    assert_eq!(loss_only.forwarded, with_slicer.forwarded);
    if !with_slicer.forwarded.is_empty() {
        assert!(with_slicer.summary.slices > 0);
    }
    assert_accounting(&with_slicer.summary);
}

#[tokio::test]
async fn loss_plus_limit_counts_discards_toward_exact_termination() {
    // All dropped: the limit still terminates after exactly N accepted bytes.
    let (peer, right) = tokio::io::duplex(4 << 20);
    let plan = FaultPlan::new(vec![
        loss("loss", 1.0, 0.0),
        FaultSpec {
            id: FaultId::new("limit").unwrap(),
            probability: Probability::new(1.0).unwrap(),
            kind: FaultKind::LimitData(eggchaos_core::LimitDataConfig {
                bytes: NonZeroU64::new(40_000).unwrap(),
            }),
        },
    ])
    .unwrap();
    let mut wrapped =
        ChaosStream::new(right, plan, SEED, "proxy", 41, Direction::Upstream).unwrap();
    let accepted = wrapped.write(&chunked_input(3)).await.unwrap();
    assert_eq!(accepted, 40_000);
    wrapped.flush().await.unwrap();
    assert_eq!(
        wrapped.summary().termination,
        Some(eggchaos_core::TerminationRequest::Graceful)
    );
    // The queue is empty (all discards), so shutdown completes and the peer
    // observes EOF.
    wrapped.shutdown().await.unwrap();
    let mut peer = peer;
    let mut forwarded = Vec::new();
    peer.read_to_end(&mut forwarded).await.unwrap();
    assert!(forwarded.is_empty());
    let summary = wrapped.summary();
    assert_eq!(summary.bytes_accepted, 40_000);
    assert_eq!(summary.bytes_discarded, 40_000);
    assert_accounting(&summary);
}

#[tokio::test]
async fn loss_inside_limit_boundary_preserves_exact_prefix() {
    // Find a seed whose first chunk drops and second preserves, then set the
    // limit inside the preserved chunk: discards count, survivors forward.
    // A single `write` (not `write_all`) is used because the exhausted limit
    // legitimately pends further writes until survivors drain.
    let seed = find_seed(0.5, 0.0, &[false, true]).await;
    let limit = GRAIN as u64 + 100;
    let plan = FaultPlan::new(vec![
        loss("loss", 0.5, 0.0),
        FaultSpec {
            id: FaultId::new("limit").unwrap(),
            probability: Probability::new(1.0).unwrap(),
            kind: FaultKind::LimitData(eggchaos_core::LimitDataConfig {
                bytes: NonZeroU64::new(limit).unwrap(),
            }),
        },
    ])
    .unwrap();
    let (mut peer, right) = tokio::io::duplex(4 << 20);
    let mut wrapped =
        ChaosStream::new(right, plan, seed, "proxy", 41, Direction::Upstream).unwrap();
    let accepted = wrapped.write(&chunked_input(3)).await.unwrap();
    assert_eq!(accepted, limit as usize);
    wrapped.flush().await.unwrap();
    wrapped.shutdown().await.unwrap();
    let mut forwarded = Vec::new();
    peer.read_to_end(&mut forwarded).await.unwrap();
    assert_eq!(forwarded.len(), 100);
    let summary = wrapped.summary();
    assert_eq!(summary.bytes_accepted, limit);
    assert_eq!(summary.bytes_discarded, GRAIN as u64);
    assert_eq!(
        summary.termination,
        Some(eggchaos_core::TerminationRequest::Graceful)
    );
    assert_accounting(&summary);
}

#[tokio::test(start_paused = true)]
async fn loss_plus_blackhole_keeps_blackhole_dominant_and_freezes_loss_state() {
    let plan = FaultPlan::new(vec![
        FaultSpec {
            id: FaultId::new("hole").unwrap(),
            probability: Probability::new(1.0).unwrap(),
            kind: FaultKind::Blackhole(eggchaos_core::BlackholeConfig { close_after: None }),
        },
        loss("loss", 0.5, 0.5),
    ])
    .unwrap();
    let mut engine = engine_with_seed(plan, SEED);
    assert_eq!(engine.accept(&chunked_input(2)), 2 * GRAIN);
    let evidence = engine.evidence();
    assert_eq!(evidence.bytes_discarded, 2 * GRAIN as u64);
    // Blackholed bytes never advance stream-loss chunk state.
    assert_eq!(evidence.stream_loss_chunks_evaluated, 0);
    assert_eq!(evidence.stream_loss_chunks_dropped, 0);
    assert_eq!(evidence.stream_loss_bytes_discarded, 0);
}

#[tokio::test]
async fn loss_plus_graceful_disconnect_still_terminates() {
    let plan = FaultPlan::new(vec![
        loss("loss", 0.0, 0.0),
        FaultSpec {
            id: FaultId::new("bye").unwrap(),
            probability: Probability::new(1.0).unwrap(),
            kind: FaultKind::Disconnect(eggchaos_core::DisconnectConfig {
                after: Duration::ZERO,
                hard_reset: false,
            }),
        },
    ])
    .unwrap();
    let mut engine = engine(plan);
    assert_eq!(engine.accept(b"hello"), 5);
    assert_eq!(
        engine.termination_request(),
        Some(eggchaos_core::TerminationRequest::Graceful)
    );
    assert_eq!(engine.evidence().bytes_accepted, 5);
    assert_eq!(engine.evidence().bytes_discarded, 0);
}

#[tokio::test]
async fn loss_plus_hard_disconnect_still_requests_reset() {
    let plan = FaultPlan::new(vec![
        loss("loss", 1.0, 0.0),
        FaultSpec {
            id: FaultId::new("rst").unwrap(),
            probability: Probability::new(1.0).unwrap(),
            kind: FaultKind::Disconnect(eggchaos_core::DisconnectConfig {
                after: Duration::ZERO,
                hard_reset: true,
            }),
        },
    ])
    .unwrap();
    let mut engine = engine(plan);
    assert_eq!(engine.accept(&vec![0xCC; 1000]), 1000);
    assert_eq!(
        engine.termination_request(),
        Some(eggchaos_core::TerminationRequest::HardReset)
    );
    assert_eq!(engine.evidence().bytes_discarded, 1000);
}

#[tokio::test(start_paused = true)]
async fn loss_plus_slow_close_delays_shutdown_only() {
    let plan = FaultPlan::new(vec![
        loss("loss", 0.0, 0.0),
        FaultSpec {
            id: FaultId::new("slow").unwrap(),
            probability: Probability::new(1.0).unwrap(),
            kind: FaultKind::SlowClose(eggchaos_core::SlowCloseConfig {
                delay: Duration::from_millis(100),
            }),
        },
    ])
    .unwrap();
    let (mut peer, right) = tokio::io::duplex(1024);
    let mut wrapped =
        ChaosStream::new(right, plan, SEED, "proxy", 41, Direction::Upstream).unwrap();
    wrapped.write_all(b"payload").await.unwrap();
    wrapped.flush().await.unwrap();
    let mut out = [0; 7];
    peer.read_exact(&mut out).await.unwrap();
    assert_eq!(&out, b"payload");
    // Ordinary writes are never delayed by slow-close; shutdown arms the
    // delay on first poll and completes after it passes.
    let mut shutdown = Box::pin(wrapped.shutdown());
    let pending = poll_fn(|cx| match shutdown.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(true),
        Poll::Ready(result) => {
            result.unwrap();
            Poll::Ready(false)
        }
    })
    .await;
    assert!(pending, "slow-close must hold shutdown for its delay");
    tokio::time::advance(Duration::from_millis(100)).await;
    shutdown.await.unwrap();
}

// ---------------------------------------------------------------------------
// Flush, shutdown, saturation, generation replacement
// ---------------------------------------------------------------------------

#[tokio::test]
async fn flush_after_mixed_traffic_delivers_every_survivor() {
    let outcome = run_traffic(
        FaultPlan::new(vec![loss("loss", 0.3, 0.2)]).unwrap(),
        SEED,
        &[chunked_input(3)],
    )
    .await;
    assert_eq!(outcome.summary.buffered_bytes, 0);
    let counts = surviving_counts(&outcome.forwarded);
    let dropped = outcome.summary.stream_loss_chunks_dropped as usize;
    assert_eq!(counts.len() + dropped, 3);
    assert_accounting(&outcome.summary);
}

#[tokio::test(start_paused = true)]
async fn shutdown_with_queued_survivors_delivers_before_close() {
    let plan = FaultPlan::new(vec![
        loss("loss", 0.0, 0.0),
        FaultSpec {
            id: FaultId::new("latency").unwrap(),
            probability: Probability::new(1.0).unwrap(),
            kind: FaultKind::Latency(eggchaos_core::LatencyConfig {
                delay: Duration::from_millis(60),
                jitter: Duration::ZERO,
                max_buffer_bytes: NonZeroU64::new(1 << 20).unwrap(),
            }),
        },
    ])
    .unwrap();
    let (mut peer, right) = tokio::io::duplex(1024);
    let mut wrapped =
        ChaosStream::new(right, plan, SEED, "proxy", 41, Direction::Upstream).unwrap();
    wrapped.write_all(b"queued").await.unwrap();
    assert_eq!(wrapped.summary().buffered_bytes, 6);
    let mut shutdown = Box::pin(wrapped.shutdown());
    // Shutdown first drains the queued survivors; the latency deadline has
    // not passed yet, so the first poll must pend.
    let pending = poll_fn(|cx| match shutdown.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(true),
        Poll::Ready(result) => {
            result.unwrap();
            Poll::Ready(false)
        }
    })
    .await;
    assert!(pending, "shutdown must wait for queued survivors");
    tokio::time::advance(Duration::from_millis(60)).await;
    shutdown.await.unwrap();
    let mut out = [0; 6];
    peer.read_exact(&mut out).await.unwrap();
    assert_eq!(&out, b"queued");
}

#[tokio::test]
async fn saturation_stops_at_the_first_unowned_preserving_prefix() {
    // First chunk drops, second preserves: acceptance must cover the whole
    // dropped chunk plus exactly the bounded preserving prefix, and must
    // never skip ahead to classify later dropped ranges early.
    let seed = find_seed(0.5, 0.0, &[false, true]).await;
    let plan = FaultPlan::new(vec![
        loss("loss", 0.5, 0.0),
        FaultSpec {
            id: FaultId::new("bound").unwrap(),
            probability: Probability::new(1.0).unwrap(),
            kind: FaultKind::Latency(eggchaos_core::LatencyConfig {
                delay: Duration::ZERO,
                jitter: Duration::ZERO,
                max_buffer_bytes: NonZeroU64::new(100).unwrap(),
            }),
        },
    ])
    .unwrap();
    let mut engine = engine_with_seed(plan, seed);
    let input = chunked_input(3);
    assert_eq!(engine.accept(&input), GRAIN + 100);
    let evidence = engine.evidence();
    assert_eq!(evidence.bytes_accepted, (GRAIN + 100) as u64);
    assert_eq!(evidence.bytes_discarded, GRAIN as u64);
    assert_eq!(evidence.buffered_bytes, 100);
    // Saturated: nothing more is accepted until the queue drains.
    assert_eq!(engine.accept(&input[GRAIN + 100..]), 0);
}

#[tokio::test(start_paused = true)]
async fn generation_replacement_drains_survivors_and_restarts_loss_state() {
    // Gen1 carries loss plus latency so survivors stay queued at publish
    // time; gen2 carries the same loss config without latency. After the
    // barrier the loss state restarts, so the second input replays gen1's
    // exact drop pattern instead of continuing the chunk sequence.
    let gen1 = FaultPlan::new(vec![
        loss("loss", 0.5, 0.0),
        FaultSpec {
            id: FaultId::new("latency").unwrap(),
            probability: Probability::new(1.0).unwrap(),
            kind: FaultKind::Latency(eggchaos_core::LatencyConfig {
                delay: Duration::from_millis(60),
                jitter: Duration::ZERO,
                max_buffer_bytes: NonZeroU64::new(1 << 20).unwrap(),
            }),
        },
    ])
    .unwrap();
    let gen2 = FaultPlan::new(vec![loss("loss", 0.5, 0.0)]).unwrap();
    let policy = eggchaos_core::LivePolicy::new(gen1, SEED);
    let (peer, right) = tokio::io::duplex(4 << 20);
    let mut wrapped =
        ChaosStream::new_live(right, policy.clone(), "proxy", 41, Direction::Upstream).unwrap();
    let reader = tokio::spawn(async move {
        let mut peer = peer;
        let mut out = Vec::new();
        peer.read_to_end(&mut out).await.unwrap();
        out
    });
    wrapped.write_all(&chunked_input(2)).await.unwrap();
    policy.publish(gen2, SEED).unwrap();
    // The next write observes the new generation: old survivors must drain
    // before the swap, which needs clock movement under paused time.
    let input2 = chunked_input(2);
    let writer = async {
        wrapped.write_all(&input2).await.unwrap();
        wrapped.flush().await.unwrap();
        wrapped.shutdown().await.unwrap();
    };
    let driver = async {
        for _ in 0..10 {
            tokio::time::advance(Duration::from_millis(60)).await;
            tokio::task::yield_now().await;
        }
    };
    tokio::join!(writer, driver);
    let forwarded = reader.await.unwrap();
    // Reference: the same loss identity/config decides the same pattern for
    // any two-chunk input, so a restarted state replays it exactly.
    let reference = run_traffic(
        FaultPlan::new(vec![loss("loss", 0.5, 0.0)]).unwrap(),
        SEED,
        &[chunked_input(2)],
    )
    .await;
    let mut expected = reference.forwarded.clone();
    expected.extend_from_slice(&reference.forwarded);
    assert_eq!(forwarded, expected);
    assert_eq!(wrapped.stream_evidence().transitions(), 1);
    assert_eq!(wrapped.observed_generation(), 2);
    assert_accounting(&wrapped.summary());
}

// ---------------------------------------------------------------------------
// Golden vectors and evidence compatibility
// ---------------------------------------------------------------------------

#[test]
fn stream_loss_chunk_seed_helper_is_stable() {
    let first = eggchaos_core::derive_stream_loss_seed(
        SEED,
        "proxy",
        41,
        Direction::Upstream,
        &FaultId::new("loss").unwrap(),
    );
    println!("M036 chunk-seed golden: {first}");
    // Pinned golden: any change here is a replay break, not a refresh.
    assert_eq!(first, 196256752510381490);
    assert_eq!(
        first,
        eggchaos_core::derive_stream_loss_seed(
            SEED,
            "proxy",
            41,
            Direction::Upstream,
            &FaultId::new("loss").unwrap()
        )
    );
    // Domain separation: the chunk stream never equals the activation seed,
    // and every identity component participates.
    let activation = eggchaos_core::derive_seed(
        SEED,
        "proxy",
        41,
        Direction::Upstream,
        &FaultId::new("loss").unwrap(),
    );
    assert_ne!(first, activation);
    assert_ne!(
        first,
        eggchaos_core::derive_stream_loss_seed(
            SEED + 1,
            "proxy",
            41,
            Direction::Upstream,
            &FaultId::new("loss").unwrap()
        )
    );
    assert_ne!(
        first,
        eggchaos_core::derive_stream_loss_seed(
            SEED,
            "proxy",
            42,
            Direction::Upstream,
            &FaultId::new("loss").unwrap()
        )
    );
    // Existing golden vectors are unchanged (rng.rs suite covers them).
}

#[tokio::test]
async fn golden_mixed_loss_trace_is_pinned() {
    // Canonical mixed trace: 3 chunks at rate 0.3 / correlation 0.2 under
    // the frozen (seed 7, "proxy", key 41, upstream, fault "loss") identity.
    // Any drift in chunk RNG, grain, thresholds, or composition changes
    // these values and must be treated as a replay break, not a fixture
    // refresh.
    let outcome = run_traffic(
        FaultPlan::new(vec![loss("loss", 0.3, 0.2)]).unwrap(),
        SEED,
        &[chunked_input(3)],
    )
    .await;
    let counts = surviving_counts(&outcome.forwarded);
    let survived: Vec<u8> = {
        let mut markers: Vec<u8> = counts.keys().cloned().collect();
        markers.sort_unstable();
        markers
    };
    for marker in &survived {
        assert_eq!(counts[marker], GRAIN, "chunk {marker} decides atomically");
    }
    let summary = outcome.summary;
    assert_eq!(summary.stream_loss_chunks_evaluated, 3);
    assert_eq!(
        summary.stream_loss_chunks_dropped as usize,
        3 - survived.len()
    );
    assert_eq!(
        summary.stream_loss_bytes_discarded,
        (3 - survived.len()) as u64 * GRAIN as u64
    );
    assert_eq!(summary.bytes_discarded, summary.stream_loss_bytes_discarded);
    assert_eq!(
        summary.bytes_accepted,
        summary.bytes_forwarded + summary.bytes_discarded
    );
    // Frozen exact outcome for this identity/config: chunks 0 and 2
    // survive, chunk 1 drops (accepted 98304, forwarded 65536, discarded
    // 32768). Deterministic by construction; any drift is a replay break,
    // not a fixture refresh.
    assert_eq!(survived, vec![0u8, 2u8]);
    assert_eq!(summary.bytes_accepted, 98304);
    assert_eq!(summary.bytes_forwarded, 65536);
    assert_eq!(summary.bytes_discarded, 32768);
    assert_eq!(summary.stream_loss_chunks_dropped, 1);
}

#[test]
fn legacy_evidence_stays_decodable_and_seven_slots_stable() {
    // Documents recorded before stream loss existed (no `stream_loss_*`
    // keys) still decode, with loss counters defaulting to zero.
    let legacy = serde_json::json!({
        "bytes_accepted": 3,
        "bytes_forwarded": 3,
        "bytes_discarded": 0,
        "segments": 1,
        "slices": 0,
        "buffered_bytes": 0,
        "high_water_bytes": 3,
        "injected_delay_ms": 10,
        "throttled_delay_ms": 0,
        "activations": [1, 0, 0, 0, 0, 0, 0],
        "termination": "Graceful",
        "rng_version": "V1"
    });
    let summary: eggchaos_core::DirectionSummary = serde_json::from_value(legacy).unwrap();
    assert_eq!(summary.activations.len(), 7);
    assert_eq!(summary.stream_loss_chunks_evaluated, 0);
    assert_eq!(summary.stream_loss_chunks_dropped, 0);
    assert_eq!(summary.stream_loss_bytes_discarded, 0);
}
