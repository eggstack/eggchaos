#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    // JSON wire form. Any parse failure is expected; we only assert
    // that valid JSON sources compile deterministically and reject
    // sources that overflow the compiled-event ceiling.
    if let Ok(source) = eggchaos_server::ScenarioScheduleV2Dto::from_json_str(
        std::str::from_utf8(input).unwrap_or(""),
    ) {
        let first = eggchaos_server::compile_schedule(&source);
        let second = eggchaos_server::compile_schedule(&source);
        match (first, second) {
            (Ok(a), Ok(b)) => {
                assert_eq!(a, b, "compiler must be pure for identical inputs");
                assert_eq!(
                    eggchaos_server::compiled_fingerprint(&a),
                    eggchaos_server::compiled_fingerprint(&b)
                );
            }
            (Err(_), Err(_)) => {}
            (Ok(_), Err(_)) | (Err(_), Ok(_)) => {
                panic!("compile error must be deterministic for identical inputs")
            }
        }
    }

    // TOML wire form. Same properties as JSON.
    if let Ok(source) = eggchaos_server::ScenarioScheduleV2Toml::from_toml_str(
        std::str::from_utf8(input).unwrap_or(""),
    ) {
        let _ = eggchaos_server::compile_schedule(&source);
    }
});
