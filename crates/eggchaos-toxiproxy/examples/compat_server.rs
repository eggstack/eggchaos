//! Standalone Toxiproxy v2.12 compatibility server for smoke testing.
//!
//! Usage: `cargo run -p eggchaos-toxiproxy --example compat_server -- 127.0.0.1:8474`
//!
//! Serves the compatibility route family with a default control authority.
//! Loopback only unless an explicit bind address is given.

use eggchaos_server::ControlState;
use eggchaos_toxiproxy::{ToxiproxyAdapter, ToxiproxyHttp};

#[tokio::main]
async fn main() {
    let bind: std::net::SocketAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8474".into())
        .parse()
        .expect("bind address parses");
    let adapter = ToxiproxyAdapter::new(ControlState::default());
    let handle = ToxiproxyHttp::start(bind, adapter)
        .await
        .expect("compat server starts");
    println!("compat server on {}", handle.local_addr());
    handle.wait().await;
}
