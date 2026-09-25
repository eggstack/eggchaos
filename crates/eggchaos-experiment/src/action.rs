//! Consumer-neutral scenario actions shared by Scenario V1 documents and
//! Scenario V2 schedule events.
//!
//! These actions name directional stream/datagram policy targets only.
//! They carry fault specifications and proxy identities; they never
//! carry payload bytes, HTTP concepts, or downstream product models.

use eggchaos_core::{DatagramFaultSpec, Direction, FaultSpec};
use serde::{Deserialize, Serialize};

/// Supported scenario actions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScenarioAction {
    /// Replace one directional plan as a barrier generation.
    SetPlan {
        /// Target proxy or experiment resource name.
        proxy: String,
        /// Target direction.
        direction: Direction,
        /// Replacement fault specifications.
        faults: Vec<FaultSpec>,
    },
    /// Remove one fault from a directional plan.
    RemoveFault {
        /// Target proxy or experiment resource name.
        proxy: String,
        /// Target direction.
        direction: Direction,
        /// Stable fault identity to remove.
        id: String,
    },
    /// Replace one directional datagram fault plan.
    SetDatagramPlan {
        /// Target proxy or experiment resource name.
        proxy: String,
        /// Target direction.
        direction: Direction,
        /// Replacement datagram fault specifications.
        faults: Vec<DatagramFaultSpec>,
    },
    /// Remove one directional datagram fault by identity.
    RemoveDatagramFault {
        /// Target proxy or experiment resource name.
        proxy: String,
        /// Target direction.
        direction: Direction,
        /// Stable fault identity to remove.
        id: String,
    },
}
