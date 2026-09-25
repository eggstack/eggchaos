//! JSON golden fixtures: every discriminated request family parses,
//! converts, and re-serializes byte-identically; unknown fields and bad
//! discriminators are rejected; error envelopes keep their shape.

use eggchaos_protocol::{
    DatagramFaultSpecV1, ErrorEnvelopeV1, FaultUpsertV1, NativeDatagramProxyRequestV1,
    NativeProxyPatchV1, NativeProxyRequestV1, ScenarioScheduleV2Dto, ScenarioV1,
};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR").to_owned() + "/tests"
    ))
    .expect("fixture must exist")
}

#[test]
fn stream_fault_fixtures_round_trip_and_convert() {
    let fixtures: Vec<serde_json::Value> =
        serde_json::from_str(&fixture("stream_faults.json")).expect("valid fixture JSON");
    assert_eq!(fixtures.len(), 7);
    for value in fixtures {
        let upsert: FaultUpsertV1 = serde_json::from_value(value.clone()).expect("upsert parses");
        upsert.validate().expect("upsert validates");
        assert_eq!(
            serde_json::to_value(&upsert).expect("upsert serializes"),
            value,
            "wire shape drift"
        );
        let serialized = serde_json::to_string(&upsert).expect("upsert serializes");
        let upsert2: FaultUpsertV1 = serde_json::from_str(&serialized).expect("upsert re-parses");
        let _ = upsert2.into_core().expect("upsert converts to core");
        assert_eq!(
            serde_json::to_string(&upsert).expect("reserialize"),
            serialized
        );
    }
}

#[test]
fn datagram_fault_fixtures_round_trip_and_convert() {
    let fixtures: Vec<serde_json::Value> =
        serde_json::from_str(&fixture("datagram_faults.json")).expect("valid fixture JSON");
    assert_eq!(fixtures.len(), 6);
    for value in fixtures {
        let spec: DatagramFaultSpecV1 = serde_json::from_value(value.clone()).expect("spec parses");
        assert_eq!(
            serde_json::to_value(&spec).expect("spec serializes"),
            value,
            "wire shape drift"
        );
        let serialized = serde_json::to_string(&spec).expect("spec serializes");
        let parsed: DatagramFaultSpecV1 =
            serde_json::from_str(&serialized).expect("spec re-parses");
        let _ = parsed.into_core().expect("spec converts to core");
        assert_eq!(
            serde_json::to_string(&spec).expect("reserialize"),
            serialized
        );
    }
}

#[test]
fn proxy_fixtures_validate_and_reject_unknown_fields() {
    let value: serde_json::Value =
        serde_json::from_str(&fixture("proxy.json")).expect("valid JSON");
    let create: NativeProxyRequestV1 =
        serde_json::from_value(value["create"].clone()).expect("proxy create parses");
    create.validate().expect("proxy create validates");
    let patch: NativeProxyPatchV1 =
        serde_json::from_value(value["patch"].clone()).expect("proxy patch parses");
    patch.validate().expect("proxy patch validates");
    let datagram: NativeDatagramProxyRequestV1 =
        serde_json::from_value(value["datagram_create"].clone()).expect("datagram create parses");
    datagram.validate().expect("datagram create validates");
    assert!(serde_json::from_str::<NativeProxyRequestV1>(
        r#"{"name":"a","listen":"127.0.0.1:0","upstream":"127.0.0.1:1","bogus":1}"#
    )
    .is_err());
    assert!(serde_json::from_str::<FaultUpsertV1>(
        r#"{"direction":"upstream","id":"x","probability":1.0,"kind":{"type":"nope"}}"#
    )
    .is_err());
}

#[test]
fn scenario_v1_fixture_converts_to_core_actions() {
    let scenario: ScenarioV1 =
        serde_json::from_str(&fixture("scenario_v1.json")).expect("scenario parses");
    scenario.validate().expect("scenario validates");
    let actions = scenario.into_core_actions().expect("actions convert");
    assert_eq!(actions.len(), 4);
    assert_eq!(actions[0].0, 0);
    assert_eq!(actions[3].0, 75);
}

#[test]
fn scenario_v2_fixture_matches_toml_semantics_and_compiles() {
    let dto: ScenarioScheduleV2Dto =
        serde_json::from_str(&fixture("scenario_v2.json")).expect("schedule parses");
    let source = dto.into_internal().expect("schedule converts");
    assert_eq!(source.seed, 7);
    assert_eq!(source.execution_key, 11);
    let compiled = eggchaos_experiment::compile_schedule(&source).expect("schedule compiles");
    assert_eq!(compiled.events.len(), 1);
    let canonical = eggchaos_protocol::to_canonical_json(&source).expect("canonical JSON");
    let reparsed_source =
        ScenarioScheduleV2Dto::from_json_str(&canonical).expect("canonical JSON parses");
    let recompiled = eggchaos_experiment::compile_schedule(&reparsed_source).expect("recompiles");
    assert_eq!(
        eggchaos_experiment::compiled_fingerprint(&compiled),
        eggchaos_experiment::compiled_fingerprint(&recompiled)
    );
}

#[test]
fn error_envelope_fixtures_keep_their_shape() {
    let fixtures: Vec<serde_json::Value> =
        serde_json::from_str(&fixture("errors.json")).expect("valid fixture JSON");
    assert_eq!(fixtures.len(), 5);
    for value in fixtures {
        let canonical = serde_json::to_string(&value).expect("canonical form");
        let envelope: ErrorEnvelopeV1 = serde_json::from_str(&canonical).expect("envelope parses");
        assert!(!envelope.error.code.is_empty());
        assert_eq!(
            serde_json::to_string(&envelope).expect("reserialize"),
            canonical
        );
    }
}
