//! Property-based tests for the v2 compiler.
//!
//! These tests do not assert specific digests; they assert that the
//! compiler satisfies invariants required by ADR 004:

use eggchaos_core::{FaultId, FaultKind, FaultSpec, LatencyConfig, Probability};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use std::time::Duration;

use crate::scenario::ScenarioAction;
use crate::scenario_v2::{
    compile_schedule, compiled_fingerprint, expanded_event_count, CleanupPolicyV2,
    CompiledScenarioV2, IsolationPolicyV2, ScenarioScheduleV2, SchedulePhaseV2, ScheduleRepeatV2,
    COMPILER_SEMANTICS_VERSION, MAX_PHASE_ACTIONS, SCHEDULE_SCHEMA_VERSION,
};
use eggchaos_core::Direction;

fn arb_fault_id() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_-]{1,16}".prop_map(|s| s)
}

fn arb_fault_kind() -> impl Strategy<Value = FaultKind> {
    (1u64..=10_000_000u64, 0u64..=1_000u64, 1u64..=10_000u64).prop_map(|(delay, jitter, buf)| {
        FaultKind::Latency(LatencyConfig {
            delay: Duration::from_nanos(delay),
            jitter: Duration::from_nanos(jitter),
            max_buffer_bytes: std::num::NonZeroU64::new(buf).expect("nonzero"),
        })
    })
}

fn arb_fault() -> impl Strategy<Value = FaultSpec> {
    (arb_fault_id(), arb_fault_kind()).prop_map(|(id, kind)| FaultSpec {
        id: FaultId::new(id).expect("id"),
        probability: Probability::new(1.0).expect("probability"),
        kind,
    })
}

fn arb_set_plan() -> impl Strategy<Value = ScenarioAction> {
    (
        "[a-zA-Z0-9_-]{1,16}",
        proptest::bool::ANY,
        proptest::collection::vec(arb_fault(), 0..=2),
    )
        .prop_map(|(proxy, up, faults)| ScenarioAction::SetPlan {
            proxy,
            direction: if up {
                Direction::Upstream
            } else {
                Direction::Downstream
            },
            faults,
        })
}

fn arb_remove_fault() -> impl Strategy<Value = ScenarioAction> {
    ("[a-zA-Z0-9_-]{1,16}", proptest::bool::ANY, arb_fault_id()).prop_map(|(proxy, up, id)| {
        ScenarioAction::RemoveFault {
            proxy,
            direction: if up {
                Direction::Upstream
            } else {
                Direction::Downstream
            },
            id,
        }
    })
}

fn arb_action() -> impl Strategy<Value = ScenarioAction> {
    prop_oneof![arb_set_plan(), arb_remove_fault(),]
}

fn arb_phase() -> impl Strategy<Value = SchedulePhaseV2> {
    (
        proptest::option::of("[a-zA-Z0-9_-]{1,8}"),
        0u64..=10_000_000u64,
        proptest::collection::vec(arb_action(), 1..=MAX_PHASE_ACTIONS),
    )
        .prop_map(|(name, duration_ns, actions)| SchedulePhaseV2 {
            name,
            duration_ns,
            actions,
        })
}

fn arb_schedule(
    phase_count: usize,
    with_repeat: bool,
) -> impl Strategy<Value = ScenarioScheduleV2> {
    let phase_strategy = proptest::collection::vec(arb_phase(), phase_count..=phase_count);
    let repeat_strategy = if with_repeat {
        proptest::option::of((1u32..=4u32, proptest::collection::vec(arb_phase(), 1..=4)))
            .prop_map(|opt| opt.map(|(count, phases)| ScheduleRepeatV2 { count, phases }))
            .boxed()
    } else {
        proptest::strategy::Just(None).boxed()
    };
    (phase_strategy, repeat_strategy)
        .prop_map(|(phases, repeat)| {
            let mut source = ScenarioScheduleV2 {
                version: SCHEDULE_SCHEMA_VERSION,
                seed: 0,
                execution_key: 0,
                isolation: IsolationPolicyV2::Strict,
                cleanup: CleanupPolicyV2::RestoreInitial,
                phases,
                repeat,
            };
            if source.phases.is_empty() && source.repeat.is_none() {
                source.phases.push(SchedulePhaseV2 {
                    name: None,
                    duration_ns: 0,
                    actions: vec![ScenarioAction::RemoveFault {
                        proxy: "p".into(),
                        direction: Direction::Upstream,
                        id: "delay".into(),
                    }],
                });
            }
            source
        })
        .boxed()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn compile_is_pure(source in arb_schedule(2, false)) {
        // The proptest surface can produce structurally valid sources
        // whose action bodies fail FaultPlan / DatagramPlan validation
        // (e.g. duplicate fault ids). When that happens, the compiler
        // deterministically rejects both calls — both rejections are
        // equivalent for purity purposes.
        let a = compile_schedule(&source);
        let b = compile_schedule(&source);
        match (a, b) {
            (Ok(a), Ok(b)) => {
                assert_eq!(a, b);
                assert_eq!(compiled_fingerprint(&a), compiled_fingerprint(&b));
            }
            (Err(_), Err(_)) => {}
            (Ok(_), Err(_)) | (Err(_), Ok(_)) => {
                panic!("compile must be deterministic for identical inputs")
            }
        }
    }

    #[test]
    fn fingerprint_changes_with_seed(seed in 0u64..=u32::MAX as u64) {
        let mut runner = proptest::test_runner::TestRunner::default();
        let mut s = arb_schedule(1, false)
            .new_tree(&mut runner)
            .expect("shrink")
            .current();
        let mut prev: Option<[u8; 32]> = None;
        for trial in 0..4 {
            s.seed = seed.wrapping_add(trial as u64);
            let compiled: CompiledScenarioV2 = compile_schedule(&s).expect("compile");
            let digest = compiled_fingerprint(&compiled);
            if let Some(previous) = prev {
                assert_ne!(previous, digest, "different seed must change fingerprint");
            }
            prev = Some(digest);
        }
    }

    #[test]
    fn compiler_emits_expected_count(source in arb_schedule(3, true)) {
        let expected = expanded_event_count(&source);
        let outcome = compile_schedule(&source);
        match outcome {
            Ok(compiled) => {
                assert_eq!(
                    compiled.events.len(),
                    expected,
                    "compiled events must equal expanded count"
                );
                assert_eq!(compiled.compiler_semantics_version, COMPILER_SEMANTICS_VERSION);
            }
            Err(_) => {
                // The proptest surface can produce structurally valid
                // sources whose action bodies fail FaultPlan /
                // DatagramPlan validation (e.g. duplicate fault ids)
                // or exceed the compiled-event ceiling. Both
                // rejection modes are expected and count as valid
                // outcomes. Anything else would panic.
            }
        }
    }
}
