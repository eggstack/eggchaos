//! M028 golden schedule corpus.
//!
//! Each committed fixture under `schedule_corpus/` freezes its
//! canonical compiled representation, SHA-256 fingerprint, compiled
//! indices/offsets, and v2 policy namespace vectors in
//! `schedule_corpus/expected.json`. Any fingerprint change requires a
//! compiler-semantics version bump plus a documented regen; the test
//! fails loudly otherwise.
//!
//! To regenerate after an approved version bump only:
//! `EGGCHAOS_CORPUS_DUMP=1 cargo test -p eggchaos-server --test
//! schedule_corpus`, then review the diff before committing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use eggchaos_server::{
    compile_schedule, compiled_fingerprint, fingerprint_hex, ScenarioScheduleV2Dto,
    ScenarioScheduleV2Toml,
};

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/schedule_corpus")
}

fn hex_decode(hex: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * index..2 * index + 2], 16).unwrap();
    }
    out
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ExpectedCase {
    fingerprint: String,
    event_count: usize,
    offsets: Vec<u64>,
    namespaces: Vec<u64>,
    phases: Vec<String>,
}

fn compute_case(name: &str, text: &str) -> ExpectedCase {
    let source = ScenarioScheduleV2Dto::from_json_str(text)
        .unwrap_or_else(|error| panic!("corpus[{name}] parses: {error}"));
    let compiled = compile_schedule(&source)
        .unwrap_or_else(|error| panic!("corpus[{name}] compiles: {error}"));
    let digest = compiled_fingerprint(&compiled);
    let namespaces = compiled
        .events
        .iter()
        .map(|event| {
            eggchaos_core::derive_schedule_policy_seed(
                compiled.seed,
                compiled.execution_key,
                digest,
                u64::from(event.compiled_index),
            )
        })
        .collect();
    ExpectedCase {
        fingerprint: fingerprint_hex(&digest),
        event_count: compiled.events.len(),
        offsets: compiled
            .events
            .iter()
            .map(|event| event.offset_ns)
            .collect(),
        namespaces,
        phases: compiled
            .events
            .iter()
            .map(|event| eggchaos_server::phase_identity_string(event.phase))
            .collect(),
    }
}

const JSON_CASES: &[&str] = &[
    "minimal_stream",
    "minimal_datagram",
    "mixed_phases",
    "equal_deadline",
    "repeat",
    "defaults_equal_explicit",
    "seed_sensitivity",
    "execkey_sensitivity",
    "offset_sensitivity",
];

#[test]
fn golden_schedule_corpus_is_frozen() {
    let dir = corpus_dir();
    let expected_path = dir.join("expected.json");
    let dump = std::env::var("EGGCHAOS_CORPUS_DUMP").is_ok();

    let mut computed: BTreeMap<String, ExpectedCase> = BTreeMap::new();
    for name in JSON_CASES {
        let text = std::fs::read_to_string(dir.join(format!("{name}.json"))).unwrap();
        computed.insert((*name).to_owned(), compute_case(name, &text));
    }

    if dump {
        let serialized = serde_json::to_string_pretty(&computed).unwrap();
        std::fs::write(&expected_path, serialized).unwrap();
        return;
    }

    let expected_text = std::fs::read_to_string(&expected_path).expect(
        "expected.json must be committed; regen only with EGGCHAOS_CORPUS_DUMP=1 after a version bump",
    );
    let expected: BTreeMap<String, ExpectedCase> = serde_json::from_str(&expected_text).unwrap();
    assert_eq!(
        expected.len(),
        computed.len(),
        "corpus case count changed; regen or restore fixtures"
    );
    for name in JSON_CASES {
        let want = expected
            .get(*name)
            .unwrap_or_else(|| panic!("missing case {name}"));
        let got = computed.get(*name).unwrap();
        assert_eq!(
            got.fingerprint, want.fingerprint,
            "corpus[{name}] fingerprint"
        );
        assert_eq!(
            got.event_count, want.event_count,
            "corpus[{name}] event_count"
        );
        assert_eq!(got.offsets, want.offsets, "corpus[{name}] offsets");
        assert_eq!(got.namespaces, want.namespaces, "corpus[{name}] namespaces");
        assert_eq!(got.phases, want.phases, "corpus[{name}] phases");
        assert_eq!(got.fingerprint.len(), 64);
    }

    // Semantic relations the freeze must preserve.
    let fp = |name: &str| computed.get(name).unwrap().fingerprint.clone();
    assert_eq!(
        fp("minimal_stream"),
        fp("defaults_equal_explicit"),
        "omitted isolation/cleanup must default without changing identity"
    );
    assert_ne!(
        fp("minimal_stream"),
        fp("seed_sensitivity"),
        "seed in fingerprint"
    );
    assert_ne!(
        fp("minimal_stream"),
        fp("execkey_sensitivity"),
        "execution_key in fingerprint"
    );
    assert_ne!(
        fp("mixed_phases"),
        fp("offset_sensitivity"),
        "offset in fingerprint"
    );
    assert_ne!(
        fp("minimal_stream"),
        fp("minimal_datagram"),
        "action/transport in fingerprint"
    );
    // Namespace vectors are run_id-independent by construction: the
    // derivation takes no run_id, and the frozen vectors pin that.
    let ns = |name: &str| computed.get(name).unwrap().namespaces.clone();
    assert_ne!(ns("minimal_stream"), ns("seed_sensitivity"));
    let _ = hex_decode(&fp("minimal_stream"));

    // TOML authoring form converges on the JSON fingerprint.
    let toml_text = std::fs::read_to_string(dir.join("minimal_stream.toml")).expect("toml fixture");
    let toml_source = ScenarioScheduleV2Toml::from_toml_str(&toml_text).expect("toml parses");
    let toml_compiled = compile_schedule(&toml_source).expect("toml compiles");
    assert_eq!(
        fingerprint_hex(&compiled_fingerprint(&toml_compiled)),
        fp("minimal_stream"),
        "JSON/TOML semantic equivalence"
    );

    // Repeat expansion pins exact offsets and phases.
    assert_eq!(
        computed["repeat"].offsets,
        vec![0, 100, 300, 600, 800, 1100, 1300]
    );
    assert_eq!(
        computed["repeat"].phases,
        vec![
            "top/0",
            "repeat/1/0",
            "repeat/1/1",
            "repeat/2/0",
            "repeat/2/1",
            "repeat/3/0",
            "repeat/3/1",
        ]
    );
    // Equal-deadline order is source order.
    assert_eq!(computed["equal_deadline"].offsets, vec![0, 0, 0]);
}

#[test]
fn corpus_bound_and_rejection_cases_hold() {
    // Exactly 1024 events compiles; 1025 fails before run creation.
    let phase = |index: usize| {
        serde_json::json!({
            "duration_ns": 0,
            "actions": [{
                "type": "set-plan", "proxy": "p", "direction": "downstream",
                "faults": [{"id": format!("id-{index}"), "probability": 1.0,
                    "kind": {"type": "latency", "delay_ns": 1, "jitter_ns": 0, "max_buffer_bytes": 1024}}]
            }]
        })
    };
    let mut phases: Vec<serde_json::Value> = (0..256).map(phase).collect();
    // 256 phases x 1 action = 256 events; add a repeat block to reach
    // exactly 1024: 1 iteration x 256 phases x 3 actions.
    let repeat_phase = |index: usize| {
        serde_json::json!({
            "duration_ns": 0,
            "actions": (0..3).map(|offset| serde_json::json!({
                "type": "set-plan", "proxy": "p", "direction": "downstream",
                "faults": [{"id": format!("r-{index}-{offset}"), "probability": 1.0,
                    "kind": {"type": "latency", "delay_ns": 1, "jitter_ns": 0, "max_buffer_bytes": 1024}}]
            })).collect::<Vec<_>>()
        })
    };
    let doc = serde_json::json!({
        "version": 2, "seed": 1, "execution_key": 1,
        "isolation": "strict", "cleanup": "restore-initial",
        "phases": phases,
        "repeat": {"count": 1, "phases": (0..256).map(repeat_phase).collect::<Vec<_>>()}
    });
    // 256 + 768 = 1024.
    let source =
        ScenarioScheduleV2Dto::from_json_str(&serde_json::to_string(&doc).unwrap()).unwrap();
    let compiled = compile_schedule(&source).expect("1024-event bound compiles");
    assert_eq!(compiled.events.len(), 1024);

    // One more action tips over the ceiling.
    phases.push(phase(9999));
    let over = serde_json::json!({
        "version": 2, "seed": 1, "execution_key": 1,
        "phases": phases,
        "repeat": {"count": 1, "phases": (0..256).map(repeat_phase).collect::<Vec<_>>()}
    });
    let over_source =
        ScenarioScheduleV2Dto::from_json_str(&serde_json::to_string(&over).unwrap()).unwrap();
    assert!(
        compile_schedule(&over_source).is_err(),
        "1025 events must fail"
    );

    // Duration overflow fails.
    let overflow = r#"{"version":2,"seed":1,"execution_key":1,"phases":[{"duration_ns":18446744073709551615,"actions":[{"type":"set-plan","proxy":"p","direction":"downstream","faults":[{"id":"a","probability":1.0,"kind":{"type":"latency","delay_ns":1,"jitter_ns":0,"max_buffer_bytes":1024}}]}]},{"duration_ns":1,"actions":[{"type":"set-plan","proxy":"p","direction":"downstream","faults":[{"id":"b","probability":1.0,"kind":{"type":"latency","delay_ns":1,"jitter_ns":0,"max_buffer_bytes":1024}}]}]}]}"#;
    assert!(ScenarioScheduleV2Dto::from_json_str(overflow)
        .map_or(true, |source| compile_schedule(&source).is_err()));

    // Malformed bodies fail bounded without panic.
    for bad in [
        r#"{"version":2,"seed":1}"#,
        r#"{"version":1,"seed":1,"execution_key":1,"phases":[]}"#,
        r#"{"version":2,"seed":1,"execution_key":1,"phases":[],"unknown":true}"#,
        r#"not json"#,
        r#"{"version":2,"seed":1,"execution_key":1,"phases":[{"duration_ns":0,"actions":[]}]}"#,
    ] {
        let parsed = ScenarioScheduleV2Dto::from_json_str(bad);
        assert!(
            parsed.is_err() || compile_schedule(&parsed.unwrap()).is_err(),
            "malformed input must reject: {bad}"
        );
    }
}
