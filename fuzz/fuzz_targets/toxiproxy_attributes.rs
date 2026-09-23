#![no_main]

use eggchaos_toxiproxy::{fault_to_toxic, Toxic};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    let Ok(toxic) = serde_json::from_slice::<Toxic>(input) else {
        return;
    };
    let Ok((direction, fault)) = toxic.to_fault() else {
        return;
    };
    let normalized = fault_to_toxic(direction, &fault);
    let Ok(round_trip) = serde_json::from_value::<Toxic>(normalized.clone()) else {
        panic!("adapter emits a toxic that cannot be parsed");
    };
    let Ok((round_trip_direction, round_trip_fault)) = round_trip.to_fault() else {
        panic!("adapter emits a toxic that cannot be translated");
    };
    assert_eq!(round_trip_direction, direction);
    assert_eq!(
        fault_to_toxic(round_trip_direction, &round_trip_fault),
        normalized
    );
});
