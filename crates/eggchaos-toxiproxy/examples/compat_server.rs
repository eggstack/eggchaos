//! Standalone Toxiproxy compatibility server for smoke testing.
//!
//! Usage:
//!
//! ```text
//! cargo run -p eggchaos-toxiproxy --example compat_server -- 127.0.0.1:8474
//! cargo run -p eggchaos-toxiproxy --example compat_server -- 127.0.0.1:8475 post-v2.12-2026-09-25
//! ```
//!
//! The first positional argument is the bind address (default
//! `127.0.0.1:8474`). The optional second argument selects the
//! compatibility profile:
//!
//! - `strict-v2.12` (default): the frozen v2.12 toxic surface. `GET
//!   /version` returns `{"version":"2.12.0"}`.
//! - `post-v2.12-2026-09-25`: opt-in pinned post-v2.12 snapshot
//!   profile. Adds `packet_loss` and reports `{"version":"git"}` from
//!   the source-build oracle.
//!
//! Loopback only unless an explicit bind address is given.

use eggchaos_server::ControlState;
use eggchaos_toxiproxy::{CompatProfile, ToxiproxyAdapter, ToxiproxyHttp};

#[tokio::main]
async fn main() {
    let bind: std::net::SocketAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8474".into())
        .parse()
        .expect("bind address parses");
    let profile = match std::env::args().nth(2).as_deref() {
        None => CompatProfile::default(),
        Some("strict-v2.12") => CompatProfile::StrictV2_12,
        Some("post-v2.12-2026-09-25") => CompatProfile::PostV2_12_2026_09_25,
        Some(other) => panic!("unknown profile {other:?}: strict-v2.12 or post-v2.12-2026-09-25"),
    };
    let adapter = ToxiproxyAdapter::with_profile(ControlState::default(), profile.clone());
    let handle = ToxiproxyHttp::start(bind, adapter)
        .await
        .expect("compat server starts");
    println!(
        "compat server on {} (profile={})",
        handle.local_addr(),
        profile.as_str()
    );
    handle.wait().await;
}
