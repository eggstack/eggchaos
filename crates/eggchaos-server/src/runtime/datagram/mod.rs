//! Fixed-target UDP runtime. This is deliberately a sibling to the TCP
//! connection supervisor: UDP associations own message boundaries and one
//! connected upstream socket each.
//!
//! The implementation is split into cohesive private modules behind one
//! [`DatagramRuntime`] authority: `model` (validated configuration, views,
//! and evidence), `registry` (proxy/listener/association registry and control
//! surface), `association` (setup, worker loop, and teardown), `supervisor`
//! (listener loop and admission), and `tests` (runtime regression suite).

use std::time::Duration;

pub(crate) mod association;
pub(crate) mod model;
pub(crate) mod registry;
pub(crate) mod supervisor;
#[cfg(test)]
mod tests;

pub use model::{
    DatagramAssociationSnapshot, DatagramProxySpec, DatagramProxyView, DatagramRuntimeError,
    DatagramRuntimeLimits,
};
pub use registry::DatagramRuntime;

pub(crate) const UDP_RECEIVE_BUFFER_BYTES: usize = 65_536;
pub(crate) const MAX_DATAGRAM_ASSOCIATIONS: usize = 65_536;
pub(crate) const MAX_DATAGRAM_PROXIES: usize = 1024;
pub(crate) const MAX_ASSOCIATION_HISTORY: usize = 65_536;
pub(crate) const MAX_INGRESS_QUEUE: usize = 1024;
pub(crate) const MAX_INGRESS_BUFFER_BYTES: usize = 1_073_741_824;
pub(crate) const MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(86_400);
