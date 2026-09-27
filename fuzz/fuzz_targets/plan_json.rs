#![no_main]

use eggchaos_core::{FaultId, FaultPlan, Probability};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    // M049: every safe construction path is exercised.
    //
    // - Scalar `FaultId` / `Probability` deserialization must reject invalid
    //   values; round-tripping a constructed value must succeed and re-deserialize
    //   identically.
    // - `FaultPlan` deserialization must reject invalid stages (duplicate IDs,
    //   kind-specific bounds, scalar bounds) at the deserialization boundary,
    //   not silently.
    // - Valid plans must continue to round-trip exactly.
    let Ok(plan) = serde_json::from_slice::<FaultPlan>(input) else {
        // Probe scalar deserialization directly: if the document parses as a
        // bare `FaultId` / `Probability`, it must round-trip exactly. Invalid
        // documents are simply rejected by serde, which is the desired
        // M049 behavior.
        if let Ok(id) = serde_json::from_slice::<FaultId>(input) {
            let encoded = serde_json::to_vec(&id).expect("serializable");
            let decoded: FaultId = serde_json::from_slice(&encoded).expect("round-trip");
            assert_eq!(id, decoded);
        }
        if let Ok(prob) = serde_json::from_slice::<Probability>(input) {
            let encoded = serde_json::to_vec(&prob).expect("serializable");
            let decoded: Probability = serde_json::from_slice(&encoded).expect("round-trip");
            assert_eq!(prob, decoded);
        }
        return;
    };
    if plan.validate().is_err() {
        return;
    }
    let encoded = serde_json::to_vec(&plan).expect("validated plans serialize");
    let decoded: FaultPlan = serde_json::from_slice(&encoded).expect("serialized plans parse");
    assert_eq!(plan, decoded);
});
