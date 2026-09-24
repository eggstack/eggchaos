#![no_main]

use eggchaos_core::FaultPlan;
use eggchaos_server::{
    ClosedConnection, ConnectionSnapshot, DatagramAssociationSnapshot,
    NativeDatagramAssociationViewV1,
};
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

    if let Ok(snapshot) = serde_json::from_slice::<DatagramAssociationSnapshot>(input) {
        let first = serde_json::to_value(snapshot).expect("datagram association serializes");
        let decoded: DatagramAssociationSnapshot =
            serde_json::from_value(first.clone()).expect("datagram association parses");
        let second = serde_json::to_value(decoded).expect("datagram association serializes again");
        assert_eq!(first, second);
    }

    if let Ok(view) = serde_json::from_slice::<NativeDatagramAssociationViewV1>(input) {
        let first = serde_json::to_value(view).expect("native datagram view serializes");
        let decoded: NativeDatagramAssociationViewV1 =
            serde_json::from_value(first.clone()).expect("native datagram view parses");
        let second = serde_json::to_value(decoded).expect("native datagram view serializes again");
        assert_eq!(first, second);
    }
});
