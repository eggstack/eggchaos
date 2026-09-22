#![no_main]

use eggchaos_core::FaultPlan;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    let Ok(plan) = serde_json::from_slice::<FaultPlan>(input) else {
        return;
    };
    if plan.validate().is_err() {
        return;
    }
    let encoded = serde_json::to_vec(&plan).expect("validated plans serialize");
    let decoded: FaultPlan = serde_json::from_slice(&encoded).expect("serialized plans parse");
    assert_eq!(plan, decoded);
});
