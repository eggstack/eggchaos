use eggchaos_server::ControlState;
use eggchaos_toxiproxy::{ToxiproxyAdapter, ToxiproxyHttp};

#[tokio::main]
async fn main() {
    let handle = ToxiproxyHttp::start(
        "127.0.0.1:0".parse().expect("static loopback address"),
        ToxiproxyAdapter::new(ControlState::default()),
    )
    .await
    .expect("compatibility listener starts");
    println!("listen={}", handle.local_addr());
    tokio::signal::ctrl_c().await.expect("ctrl-c handler");
    handle.shutdown();
    handle.wait().await;
}
