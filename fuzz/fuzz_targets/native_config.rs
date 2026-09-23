#![no_main]

use eggchaos_server::NativeConfig;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    let Ok(text) = std::str::from_utf8(input) else {
        return;
    };
    if let Ok(config) = NativeConfig::parse(text) {
        let _ = config.compile_proxies();
    }
});
