#![no_main]
use std::num::NonZeroU64;

use bytes::Bytes;
use eggchaos_core::{
    DatagramDirectionEngine, DatagramPlan, DatagramQueueLimits, Direction, RngVersion,
};
use libfuzzer_sys::fuzz_target;
use serde_json::Value;
use tokio::time::Instant;

fuzz_target!(|data: &[u8]| {
    let Ok(value) = serde_json::from_slice::<Value>(data) else { return };
    let Ok(plan) = serde_json::from_value::<DatagramPlan>(value) else { return };
    let Ok(plan) = DatagramPlan::new(plan.faults().to_vec()) else { return };
    let limits = DatagramQueueLimits {
        max_queued_datagrams: NonZeroU64::new(4096).unwrap(),
        max_queued_bytes: NonZeroU64::new(16 * 1024 * 1024).unwrap(),
        max_datagram_bytes: NonZeroU64::new(65_535).unwrap(),
    };
    let mut engine = DatagramDirectionEngine::new(limits, "fuzz", 1, Direction::Upstream, RngVersion::V1).unwrap();
    let policy = eggchaos_core::DatagramLivePolicy::new(plan, 17).unwrap().snapshot();
    let payload = Bytes::copy_from_slice(&data[..data.len().min(65_535)]);
    engine.admit(Instant::now(), payload, &policy);
    let evidence = engine.evidence();
    assert!(evidence.queued_datagrams <= limits.max_queued_datagrams.get());
    assert!(evidence.queued_bytes <= limits.max_queued_bytes.get());
});
