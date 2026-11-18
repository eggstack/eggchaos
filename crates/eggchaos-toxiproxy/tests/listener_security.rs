//! Regression tests for the Toxiproxy listener exposure guard.

use eggchaos_server::ControlState;
use eggchaos_toxiproxy::{ToxiproxyAdapter, ToxiproxyHttp};

#[tokio::test]
async fn default_start_refuses_non_loopback_before_binding() {
    let adapter = ToxiproxyAdapter::new(ControlState::default());
    let result = ToxiproxyHttp::start("192.0.2.1:0".parse().unwrap(), adapter).await;
    let Err(error) = result else {
        panic!("non-loopback bind must require explicit opt-in")
    };
    assert!(error.to_string().contains("start_with_public_bind"));
}

#[tokio::test]
async fn explicit_public_bind_constructor_remains_additive_on_loopback() {
    let adapter = ToxiproxyAdapter::new(ControlState::default());
    let handle = ToxiproxyHttp::start_with_public_bind("127.0.0.1:0".parse().unwrap(), adapter)
        .await
        .expect("explicit constructor can start a loopback listener");
    assert!(handle.local_addr().ip().is_loopback());
    handle.shutdown();
    handle.wait().await;
}
