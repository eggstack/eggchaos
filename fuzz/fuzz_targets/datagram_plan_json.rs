#![no_main]
use eggchaos_core::DatagramPlan;
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = serde_json::from_slice::<Value>(data) {
        if let Ok(plan) = serde_json::from_value::<DatagramPlan>(value) {
            if let Ok(validated) = DatagramPlan::new(plan.faults().to_vec()) {
                let encoded = serde_json::to_vec(&validated).expect("serialize validated datagram plan");
                let decoded: DatagramPlan = serde_json::from_slice(&encoded).expect("round-trip datagram plan");
                assert_eq!(validated, decoded);
            }
        }
    }
});
