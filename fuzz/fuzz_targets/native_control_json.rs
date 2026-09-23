#![no_main]

use eggchaos_server::{
    FaultKindV1, FaultPatchV1, FaultUpsertV1, NativeProxyPatchV1, NativeProxyRequestV1,
    RuntimeConfigV1, ScenarioV1,
};
use libfuzzer_sys::fuzz_target;

fn stable_round_trip<T>(input: &[u8])
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let Ok(value) = serde_json::from_slice::<T>(input) else {
        return;
    };
    let encoded = serde_json::to_vec(&value).expect("DTO serialization is infallible");
    let _: T = serde_json::from_slice(&encoded).expect("serialized DTO parses");
}

fuzz_target!(|input: &[u8]| {
    stable_round_trip::<FaultKindV1>(input);
    stable_round_trip::<FaultUpsertV1>(input);
    stable_round_trip::<FaultPatchV1>(input);
    stable_round_trip::<NativeProxyRequestV1>(input);
    stable_round_trip::<NativeProxyPatchV1>(input);
    stable_round_trip::<RuntimeConfigV1>(input);
    stable_round_trip::<ScenarioV1>(input);

    if let Ok(value) = serde_json::from_slice::<FaultKindV1>(input) {
        let _ = value.validate();
    }
    if let Ok(value) = serde_json::from_slice::<FaultUpsertV1>(input) {
        let _ = value.validate();
    }
    if let Ok(value) = serde_json::from_slice::<FaultPatchV1>(input) {
        let _ = value.validate();
    }
    if let Ok(value) = serde_json::from_slice::<NativeProxyRequestV1>(input) {
        let _ = value.validate();
    }
    if let Ok(value) = serde_json::from_slice::<NativeProxyPatchV1>(input) {
        let _ = value.validate();
    }
    if let Ok(value) = serde_json::from_slice::<RuntimeConfigV1>(input) {
        let _ = value.clone().limits();
        let _ = value.clone().relay_buffer();
        let _ = value.termination_grace();
    }
    if let Ok(value) = serde_json::from_slice::<ScenarioV1>(input) {
        let _ = value.validate();
    }
});
