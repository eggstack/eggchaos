#![no_main]

use eggchaos_toxiproxy::{fault_to_toxic, CompatProfile, Toxic};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    let Ok(toxic) = serde_json::from_slice::<Toxic>(input) else {
        return;
    };
    let profile = if toxic.r#type == "packet_loss" {
        CompatProfile::PostV2_12_2026_09_25
    } else {
        CompatProfile::StrictV2_12
    };
    let Ok((direction, fault)) = toxic.to_fault_with_profile(&profile) else {
        return;
    };
    if toxic.r#type == "packet_loss" {
        assert!(
            toxic.to_fault().is_err(),
            "strict default accepted packet_loss"
        );
    }
    let normalized = fault_to_toxic(direction, &fault, profile.clone()).unwrap();
    let Ok(round_trip) = serde_json::from_value::<Toxic>(normalized.clone()) else {
        panic!("adapter emits a toxic that cannot be parsed");
    };
    let Ok((round_trip_direction, round_trip_fault)) = round_trip.to_fault_with_profile(&profile)
    else {
        panic!("adapter emits a toxic that cannot be translated");
    };
    assert_eq!(round_trip_direction, direction);
    assert_eq!(
        fault_to_toxic(round_trip_direction, &round_trip_fault, profile).unwrap(),
        normalized
    );
});
