use std::num::NonZeroU64;

use bytes::Bytes;
use eggchaos_core::{
    DatagramDirectionEngine, DatagramPlan, DatagramQueueLimits, Direction, PublishedDatagramPolicy,
    RngVersion,
};
use serde::Deserialize;
use std::sync::Arc;
use tokio::time::{advance, Duration, Instant};

const CORPUS: &str = include_str!("fixtures/datagram_golden_traces.json");

#[derive(Deserialize)]
struct Corpus {
    version: u32,
    rng_version: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    seed_namespace: u64,
    plan: DatagramPlan,
    #[serde(default)]
    policy_sequence: Vec<Policy>,
    inputs_hex: Vec<String>,
    max_queued_datagrams: u64,
    max_queued_bytes: u64,
    ready_after_ns: u64,
    #[serde(default)]
    followup_after_ns: u64,
    #[serde(default)]
    expected_first_emissions: Option<Vec<(u64, u16, u64, String)>>,
    expected_emissions: Vec<(u64, u16, u64, String)>,
    expected_configured_loss: u64,
    #[serde(default)]
    expected_duplicated_copies: u64,
    #[serde(default)]
    expected_corrupted_candidates: u64,
    #[serde(default)]
    expected_reorder_activations: u64,
    #[serde(default)]
    expected_bandwidth_delay_nanos: u128,
    expected_queue_overflow: u64,
}

#[derive(Deserialize)]
struct Policy {
    generation: u64,
    seed_namespace: u64,
    plan: DatagramPlan,
}

#[tokio::test(start_paused = true)]
async fn frozen_datagram_golden_trace_corpus_matches_exactly() {
    let corpus: Corpus = serde_json::from_str(CORPUS).expect("golden corpus JSON");
    assert_eq!(corpus.version, 1);
    assert_eq!(corpus.rng_version, "v1");
    assert_eq!(corpus.cases.len(), 14);
    let case_count = corpus.cases.len();

    for (case_index, case) in corpus.cases.into_iter().enumerate() {
        let limits = DatagramQueueLimits {
            max_queued_datagrams: NonZeroU64::new(case.max_queued_datagrams).unwrap(),
            max_queued_bytes: NonZeroU64::new(case.max_queued_bytes).unwrap(),
            max_datagram_bytes: NonZeroU64::new(65_535).unwrap(),
        };
        let policy = Arc::new(PublishedDatagramPolicy {
            generation: 1,
            plan: Arc::new(case.plan),
            seed_namespace: case.seed_namespace,
        });
        let start = Instant::now();
        let mut engine = DatagramDirectionEngine::new(
            limits,
            "golden-proxy",
            7,
            Direction::Upstream,
            RngVersion::V1,
        )
        .unwrap();
        for (index, input) in case.inputs_hex.iter().enumerate() {
            let selected = case.policy_sequence.get(index).map(|selected| {
                Arc::new(PublishedDatagramPolicy {
                    generation: selected.generation,
                    plan: Arc::new(selected.plan.clone()),
                    seed_namespace: selected.seed_namespace,
                })
            });
            engine.admit(
                start,
                Bytes::from(decode_hex(input)),
                selected.as_deref().unwrap_or(policy.as_ref()),
            );
        }
        if case.ready_after_ns > 0 {
            advance(Duration::from_nanos(case.ready_after_ns)).await;
        }
        let first: Vec<_> = engine
            .take_ready(Instant::now())
            .into_iter()
            .map(as_trace)
            .collect();
        if let Some(expected) = case.expected_first_emissions {
            assert_eq!(first, expected, "first emissions for {}", case.name);
        }
        let mut actual = first;
        if case.followup_after_ns > 0 {
            advance(Duration::from_nanos(case.followup_after_ns)).await;
            actual.extend(engine.take_ready(Instant::now()).into_iter().map(as_trace));
        }
        assert_eq!(actual, case.expected_emissions, "case {}", case.name);
        assert_eq!(
            engine.evidence().configured_loss,
            case.expected_configured_loss,
            "case {}",
            case.name
        );
        assert_eq!(
            engine.evidence().queue_overflow,
            case.expected_queue_overflow,
            "case {}",
            case.name
        );
        assert_eq!(
            engine.evidence().duplicated_copies,
            case.expected_duplicated_copies,
            "case {}",
            case.name
        );
        assert_eq!(
            engine.evidence().corrupted_candidates,
            case.expected_corrupted_candidates,
            "case {}",
            case.name
        );
        assert_eq!(
            engine.evidence().reorder_activations,
            case.expected_reorder_activations,
            "case {}",
            case.name
        );
        assert_eq!(
            engine.evidence().bandwidth_delay_nanos,
            case.expected_bandwidth_delay_nanos,
            "case {}",
            case.name
        );
        assert_eq!(engine.evidence().queued_datagrams, 0, "case {}", case.name);
        assert_eq!(engine.evidence().queued_bytes, 0, "case {}", case.name);
        if case_index + 1 != case_count {
            // Isolate paused Tokio time between cases while preserving each
            // trace's exact logical timestamps.
            advance(Duration::ZERO).await;
        }
    }
}

fn as_trace(datagram: eggchaos_core::DatagramScheduled) -> (u64, u16, u64, String) {
    (
        datagram.ingress_ordinal,
        datagram.copy_index,
        datagram.generation,
        encode_hex(&datagram.payload),
    )
}

fn decode_hex(input: &str) -> Vec<u8> {
    input
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("fixture hex is ASCII");
            u8::from_str_radix(text, 16).expect("fixture hex is valid")
        })
        .collect()
}

fn encode_hex(input: &[u8]) -> String {
    input.iter().map(|byte| format!("{byte:02x}")).collect()
}
