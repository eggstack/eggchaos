#![no_main]

use eggchaos_core::{FaultPlan, LivePolicy, PublishError};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    if input.len() < 2 {
        return;
    }
    let plan_len = usize::from(u16::from_le_bytes([input[0], input[1]]));
    let Some(plan_end) = 2usize.checked_add(plan_len) else {
        return;
    };
    let Some(plan_bytes) = input.get(2..plan_end) else {
        return;
    };
    let Ok(plan) = serde_json::from_slice::<FaultPlan>(plan_bytes) else {
        return;
    };
    if plan.validate().is_err() {
        return;
    }

    let policy = LivePolicy::new(FaultPlan::empty(), 0);
    for (index, operation) in input[plan_end..].iter().copied().enumerate() {
        let before = policy.snapshot();
        if operation & 0b100 != 0 {
            let stale = before.generation.saturating_sub(1);
            let result = policy.publish_expected(plan.clone(), u64::from(operation), stale);
            assert!(matches!(result, Err(PublishError::Conflict(_))));
            assert_eq!(policy.snapshot().generation, before.generation);
            continue;
        }

        let next_plan = if operation & 1 == 0 {
            plan.clone()
        } else {
            FaultPlan::empty()
        };
        let published = policy
            .publish_expected(next_plan.clone(), u64::from(operation), before.generation)
            .expect("current generation publication succeeds");
        assert_eq!(published.generation, before.generation + 1);
        assert_eq!(*published.plan, next_plan);
        assert_eq!(published.seed_namespace, u64::from(operation));
        assert_eq!(policy.snapshot().generation, before.generation + 1);

        // Exercise bounded repeat transitions without tying outcomes to task scheduling.
        assert!(index < input.len());
    }
});
