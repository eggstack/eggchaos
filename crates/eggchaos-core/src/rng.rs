use std::num::Wrapping;

use serde::{Deserialize, Serialize};

use crate::{Direction, FaultId, RngVersion};

/// Replay metadata for a connection-local deterministic RNG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RngEvidence {
    /// Algorithm version.
    pub version: RngVersion,
    /// Stable derived seed.
    pub seed: u64,
}

/// Derive a stable sub-seed from explicit identity components.
pub fn derive_seed(
    run_seed: u64,
    proxy: &str,
    connection_key: u64,
    direction: Direction,
    fault: &FaultId,
) -> u64 {
    let mut h = Wrapping(run_seed) + Wrapping(0x9e37_79b9_7f4a_7c15);
    for byte in proxy.as_bytes().iter().chain(fault.as_str().as_bytes()) {
        h += Wrapping(u64::from(*byte) + 0x100);
        h ^= h >> 30;
        h *= Wrapping(0xbf58_476d_1ce4_e5b9);
    }
    h += Wrapping(connection_key.rotate_left(17));
    h += Wrapping(match direction {
        Direction::Upstream => 0x5550,
        Direction::Downstream => 0x444e,
    });
    splitmix(h.0)
}

fn splitmix(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = value;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Derive a policy seed namespace from explicit scenario identity.
///
/// The namespace feeds fault-local RNG compilation for every connection
/// that observes the published policy, so a scenario seed participates in
/// the deterministic decisions it claims to reproduce. Derivation is a
/// pure function of `(scenario_seed, run_id, event_index)`: it never
/// depends on task scheduling, wall time, or connection order. Manual
/// control updates retain the policy's current namespace instead.
pub fn derive_policy_seed(scenario_seed: u64, run_id: u64, event_index: u64) -> u64 {
    let mut h = Wrapping(scenario_seed) + Wrapping(0x9e37_79b9_7f4a_7c15);
    h += Wrapping(run_id.rotate_left(13) + 0x100);
    h ^= h >> 29;
    h *= Wrapping(0xbf58_476d_1ce4_e5b9);
    h += Wrapping(event_index.rotate_left(31) + 0x200);
    h ^= h >> 27;
    h *= Wrapping(0x94d0_49bb_1331_11eb);
    splitmix(h.0 ^ 0x51ed_ee15_5eed_5eed)
}
#[derive(Debug, Clone)]
pub struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    /// Create a generator from a frozen seed.
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }
    /// Return the next raw value.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        splitmix(self.state)
    }
    /// Return a value in `[0, upper)` (or zero for an empty range).
    pub fn below(&mut self, upper: u64) -> u64 {
        if upper == 0 {
            0
        } else {
            self.next_u64() % upper
        }
    }
    /// Return an unbiased-ish deterministic probability decision using integer thresholds.
    pub fn bernoulli(&mut self, probability: f64) -> bool {
        if probability <= 0.0 {
            return false;
        }
        if probability >= 1.0 {
            return true;
        }
        let threshold = (probability * (u64::MAX as f64)) as u64;
        self.next_u64() <= threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FaultId;

    #[test]
    fn golden_vectors_are_stable() {
        let mut rng = DeterministicRng::new(42);
        assert_eq!(rng.next_u64(), 2_949_826_092_126_892_291);
        assert_eq!(rng.next_u64(), 5_139_283_748_462_763_858);
        let id = FaultId::new("latency").unwrap();
        assert_eq!(
            derive_seed(42, "proxy", 7, Direction::Upstream, &id),
            11_882_912_530_514_077_282
        );
    }

    #[test]
    fn policy_seed_derivation_is_stable_and_sensitive() {
        // Golden vectors pin the derivation; sensitivity pins that each
        // identity component participates.
        assert_eq!(derive_policy_seed(7, 1, 0), 4_026_889_766_568_732_747);
        assert_eq!(derive_policy_seed(7, 1, 1), 11_250_473_848_183_634_583);
        assert_eq!(derive_policy_seed(8, 1, 0), 3_937_417_822_122_820_953);
    }
}
