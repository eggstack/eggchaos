#![no_main]

use eggchaos_server::{
    DatagramFaultKindV1, DatagramFaultPatchV1, DatagramFaultSpecV1, DatagramFaultUpsertV1,
    DatagramRuntimeConfigV1, FaultKindV1, FaultPatchV1, FaultUpsertV1,
    NativeDatagramAssociationViewV1, NativeDatagramProxyPatchV1, NativeDatagramProxyRequestV1,
    NativeProxyPatchV1, NativeProxyRequestV1, RuntimeConfigV1, ScenarioScheduleV2Dto, ScenarioV1,
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
    stable_round_trip::<DatagramFaultKindV1>(input);
    stable_round_trip::<DatagramFaultSpecV1>(input);
    stable_round_trip::<DatagramFaultUpsertV1>(input);
    stable_round_trip::<DatagramFaultPatchV1>(input);
    stable_round_trip::<NativeDatagramProxyRequestV1>(input);
    stable_round_trip::<NativeDatagramProxyPatchV1>(input);
    stable_round_trip::<NativeDatagramAssociationViewV1>(input);
    stable_round_trip::<DatagramRuntimeConfigV1>(input);
    stable_round_trip::<ScenarioScheduleV2Dto>(input);

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
        // `validate` is the current admission-bounds entry (the pre-M032
        // `limits()` assembler was removed by the protocol extraction);
        // conversion must never panic on arbitrary DTO bytes.
        let _ = value.validate();
        let _ = value.clone().relay_buffer();
        let _ = value.termination_grace();
    }
    if let Ok(value) = serde_json::from_slice::<ScenarioV1>(input) {
        let _ = value.validate();
    }
    if let Ok(value) = serde_json::from_slice::<ScenarioScheduleV2Dto>(input) {
        // Parsing must never panic; conversion and compilation are
        // total over the DTO and report bounded errors.
        if let Ok(source) = value.into_internal() {
            let _ = eggchaos_server::compile_schedule(&source);
        }
    }
});
