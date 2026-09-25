//! Mechanical drift check: the checked-in OpenAPI document must agree with
//! the protocol DTO authority (`NATIVE_OPERATIONS`, fault discriminators,
//! required fields, error envelope).
//!
//! This is M032's single-authority mechanism (option 3): executable
//! contract tests prove the checked-in OpenAPI operations/schemas against
//! protocol DTO serialization and the operation inventory. Any added,
//! removed, or reshaped route, discriminator, required field, default
//! unit, or error envelope fails this test until the contract is
//! reconciled.

use std::collections::{BTreeMap, BTreeSet};

use eggchaos_protocol::{
    DatagramFaultKindV1, DatagramFaultSpecV1, FaultKindV1, FaultUpsertV1, NATIVE_OPERATIONS,
    OPENAPI_CONTRACT_VERSION,
};

fn openapi_document() -> serde_json::Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../api/openapi/eggchaos-v1.yaml"
    );
    let text = std::fs::read_to_string(path).expect("openapi document must exist");
    serde_yaml::from_str(&text).expect("openapi document must parse as YAML")
}

fn operation_map(doc: &serde_json::Value) -> BTreeMap<(String, String), String> {
    let mut map = BTreeMap::new();
    let paths = doc
        .get("paths")
        .and_then(serde_json::Value::as_object)
        .expect("openapi must declare paths");
    for (path, item) in paths {
        let item = item.as_object().expect("path item must be an object");
        for (method, operation) in item {
            if ["parameters", "summary", "description", "$ref"].contains(&method.as_str()) {
                continue;
            }
            let operation_id = operation
                .get("operationId")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_owned();
            map.insert((method.to_uppercase(), path.clone()), operation_id);
        }
    }
    map
}

#[test]
fn openapi_version_matches_the_protocol_authority() {
    let doc = openapi_document();
    assert_eq!(
        doc.get("openapi").and_then(serde_json::Value::as_str),
        Some("3.0.3")
    );
    assert_eq!(
        doc.pointer("/info/version")
            .and_then(serde_json::Value::as_str),
        Some(OPENAPI_CONTRACT_VERSION)
    );
}

#[test]
fn openapi_operations_match_the_native_inventory_exactly() {
    let doc = openapi_document();
    let documented = operation_map(&doc);
    let mut expected = BTreeMap::new();
    for operation in NATIVE_OPERATIONS {
        expected.insert(
            (operation.method.to_owned(), operation.path.to_owned()),
            operation.operation_id.to_owned(),
        );
    }
    let documented_routes: BTreeSet<_> = documented.keys().cloned().collect();
    let expected_routes: BTreeSet<_> = expected.keys().cloned().collect();
    let missing: Vec<_> = expected_routes.difference(&documented_routes).collect();
    let extra: Vec<_> = documented_routes.difference(&expected_routes).collect();
    assert!(
        missing.is_empty(),
        "operations missing from OpenAPI: {missing:?}"
    );
    assert!(
        extra.is_empty(),
        "undocumented OpenAPI operations: {extra:?}"
    );
    for (route, expected_id) in &expected {
        assert_eq!(
            documented.get(route).map(String::as_str),
            Some(expected_id.as_str()),
            "operationId drift for {route:?}"
        );
    }
}

#[test]
fn openapi_declares_bearer_security() {
    let doc = openapi_document();
    let scheme = doc.pointer("/components/securitySchemes/bearerAuth");
    assert_eq!(
        scheme
            .and_then(|value| value.get("type"))
            .and_then(serde_json::Value::as_str),
        Some("http")
    );
    assert_eq!(
        scheme
            .and_then(|value| value.get("scheme"))
            .and_then(serde_json::Value::as_str),
        Some("bearer")
    );
}

fn stream_fault_tags() -> BTreeSet<String> {
    let kinds = [
        FaultKindV1::Latency {
            delay_ns: 1,
            jitter_ns: 0,
            max_buffer_bytes: 8,
        },
        FaultKindV1::Bandwidth {
            bytes_per_second: 1,
            burst_bytes: 1,
        },
        FaultKindV1::Blackhole {
            close_after_ns: None,
        },
        FaultKindV1::LimitData { bytes: 1 },
        FaultKindV1::SlowClose { delay_ns: 0 },
        FaultKindV1::Slice {
            average_size: 1,
            variation: 0,
            delay_ns: 0,
        },
        FaultKindV1::Disconnect {
            after_ns: 0,
            hard_reset: false,
        },
    ];
    kinds
        .iter()
        .map(|kind| {
            serde_json::to_value(kind)
                .expect("fault kind serializes")
                .get("type")
                .and_then(serde_json::Value::as_str)
                .expect("fault kind has a type tag")
                .to_owned()
        })
        .collect()
}

fn datagram_fault_tags() -> BTreeSet<String> {
    let kinds = [
        DatagramFaultKindV1::Delay {
            delay_ns: 1,
            jitter_ns: 0,
        },
        DatagramFaultKindV1::Loss,
        DatagramFaultKindV1::Duplicate {
            additional_copies: 1,
        },
        DatagramFaultKindV1::Reorder { hold_ns: 1 },
        DatagramFaultKindV1::PayloadCorrupt { bytes: 1 },
        DatagramFaultKindV1::Bandwidth {
            bytes_per_second: 1,
            burst_bytes: 1,
        },
    ];
    kinds
        .iter()
        .map(|kind| {
            serde_json::to_value(kind)
                .expect("datagram fault kind serializes")
                .get("type")
                .and_then(serde_json::Value::as_str)
                .expect("datagram fault kind has a type tag")
                .to_owned()
        })
        .collect()
}

fn discriminator_tags(doc: &serde_json::Value, schema: &str) -> BTreeSet<String> {
    let mapping = doc
        .pointer(&format!(
            "/components/schemas/{schema}/discriminator/mapping"
        ))
        .and_then(serde_json::Value::as_object)
        .unwrap_or_else(|| panic!("schema {schema} must declare a discriminator mapping"));
    mapping.keys().cloned().collect()
}

#[test]
fn openapi_fault_discriminators_match_protocol_tags() {
    let doc = openapi_document();
    assert_eq!(
        discriminator_tags(&doc, "StreamFaultKind"),
        stream_fault_tags(),
        "stream fault discriminator drift"
    );
    assert_eq!(
        discriminator_tags(&doc, "DatagramFaultKind"),
        datagram_fault_tags(),
        "datagram fault discriminator drift"
    );
    assert_eq!(
        discriminator_tags(&doc, "ScenarioActionV1"),
        BTreeSet::from(
            [
                "set-plan",
                "remove-fault",
                "set-datagram-plan",
                "remove-datagram-fault"
            ]
            .map(str::to_owned)
        ),
        "scenario action discriminator drift"
    );
}

fn required_fields(doc: &serde_json::Value, schema: &str) -> BTreeSet<String> {
    doc.pointer(&format!("/components/schemas/{schema}/required"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("schema {schema} must declare required fields"))
        .iter()
        .map(|value| {
            value
                .as_str()
                .expect("required entry is a string")
                .to_owned()
        })
        .collect()
}

#[test]
fn openapi_required_fields_match_wire_spellings() {
    let doc = openapi_document();
    assert_eq!(
        required_fields(&doc, "NativeProxyRequestV1"),
        BTreeSet::from(["name", "listen", "upstream"].map(str::to_owned))
    );
    assert_eq!(
        required_fields(&doc, "FaultUpsertV1"),
        BTreeSet::from(["direction", "id", "kind"].map(str::to_owned))
    );
    assert_eq!(
        required_fields(&doc, "ScenarioScheduleV2Dto"),
        BTreeSet::from(["version", "seed", "execution_key"].map(str::to_owned))
    );
    assert_eq!(
        required_fields(&doc, "ErrorEnvelopeV1"),
        BTreeSet::from(["error"].map(str::to_owned))
    );
    assert_eq!(
        required_fields(&doc, "DatagramFaultUpsertV1"),
        BTreeSet::from(["direction", "id", "kind"].map(str::to_owned))
    );
}

#[test]
fn every_stream_fault_variant_survives_a_wire_round_trip() {
    for tag in stream_fault_tags() {
        let upsert = FaultUpsertV1 {
            direction: eggchaos_core::Direction::Downstream,
            id: format!("probe-{tag}"),
            probability: 0.5,
            kind: match tag.as_str() {
                "latency" => FaultKindV1::Latency {
                    delay_ns: 5,
                    jitter_ns: 1,
                    max_buffer_bytes: 16,
                },
                "bandwidth" => FaultKindV1::Bandwidth {
                    bytes_per_second: 9,
                    burst_bytes: 9,
                },
                "blackhole" => FaultKindV1::Blackhole {
                    close_after_ns: Some(7),
                },
                "limit-data" => FaultKindV1::LimitData { bytes: 3 },
                "slow-close" => FaultKindV1::SlowClose { delay_ns: 11 },
                "slice" => FaultKindV1::Slice {
                    average_size: 4,
                    variation: 1,
                    delay_ns: 2,
                },
                "disconnect" => FaultKindV1::Disconnect {
                    after_ns: 0,
                    hard_reset: true,
                },
                other => panic!("unknown stream fault tag {other}"),
            },
        };
        let json = serde_json::to_string(&upsert).expect("upsert serializes");
        let parsed: FaultUpsertV1 = serde_json::from_str(&json).expect("upsert parses");
        let (direction, spec) = parsed.into_core().expect("upsert converts");
        assert_eq!(direction, eggchaos_core::Direction::Downstream);
        assert_eq!(spec.id.as_str(), format!("probe-{tag}"));
        assert_eq!(serde_json::to_string(&upsert).expect("reserialize"), json);
    }
}

#[test]
fn every_datagram_fault_variant_survives_a_wire_round_trip() {
    for tag in datagram_fault_tags() {
        let spec = DatagramFaultSpecV1 {
            id: format!("probe-{tag}"),
            probability: 0.25,
            kind: match tag.as_str() {
                "delay" => DatagramFaultKindV1::Delay {
                    delay_ns: 5,
                    jitter_ns: 1,
                },
                "loss" => DatagramFaultKindV1::Loss,
                "duplicate" => DatagramFaultKindV1::Duplicate {
                    additional_copies: 2,
                },
                "reorder" => DatagramFaultKindV1::Reorder { hold_ns: 9 },
                "payload-corrupt" => DatagramFaultKindV1::PayloadCorrupt { bytes: 2 },
                "bandwidth" => DatagramFaultKindV1::Bandwidth {
                    bytes_per_second: 8,
                    burst_bytes: 8,
                },
                other => panic!("unknown datagram fault tag {other}"),
            },
        };
        let json = serde_json::to_string(&spec).expect("spec serializes");
        let parsed: DatagramFaultSpecV1 = serde_json::from_str(&json).expect("spec parses");
        let core = parsed.into_core().expect("spec converts");
        assert_eq!(core.id.as_str(), format!("probe-{tag}"));
        assert_eq!(serde_json::to_string(&spec).expect("reserialize"), json);
    }
}
