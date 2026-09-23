#![no_main]

use eggchaos_core::FaultPlan;
use eggchaos_server::{ClosedConnection, ConnectionSnapshot};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    if let Ok(plan) = serde_json::from_slice::<FaultPlan>(input) {
        if plan.validate().is_ok() {
            let encoded = serde_json::to_vec(&plan).expect("plan serializes");
            let decoded: FaultPlan = serde_json::from_slice(&encoded).expect("plan parses");
            assert_eq!(plan, decoded);
        }
    }

    if let Ok(snapshot) = serde_json::from_slice::<ConnectionSnapshot>(input) {
        let first = serde_json::to_value(snapshot).expect("snapshot serializes");
        let decoded: ConnectionSnapshot =
            serde_json::from_value(first.clone()).expect("snapshot parses");
        let second = serde_json::to_value(decoded).expect("snapshot serializes again");
        assert_eq!(first, second);
    }

    if let Ok(closed) = serde_json::from_slice::<ClosedConnection>(input) {
        let first = serde_json::to_value(closed).expect("closed evidence serializes");
        let decoded: ClosedConnection =
            serde_json::from_value(first.clone()).expect("closed evidence parses");
        let second = serde_json::to_value(decoded).expect("closed evidence serializes again");
        assert_eq!(first, second);
    }
});
