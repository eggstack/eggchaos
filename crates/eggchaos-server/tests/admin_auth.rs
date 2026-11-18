//! M059 WP1: native admin authentication baseline regressions.
//!
//! The 2026-10-01 audit corrections under test: a non-loopback admin
//! listener requires explicit `public_admin` opt-in plus a non-empty
//! bearer token, and request authorization compares fixed-width
//! SHA-256 digests instead of raw token bytes. These tests pin that
//! behavior before dependency/workflow edits so later maintenance
//! cannot regress the already-landed correctness fixes.

use eggchaos_server::{AdminConfig, ControlState, NativeAdmin, NativeConfig};

/// Documentation TEST-NET-1 address: never loopback, never routable.
/// The insecure-bind guard fires before any socket bind, so this test
/// needs no host network capability beyond loopback for the matrix.
fn public_config(public_admin: bool, auth_token: Option<&str>) -> AdminConfig {
    AdminConfig {
        bind: "192.0.2.1:0".parse().expect("documentation test address"),
        public_admin,
        auth_token: auth_token.map(str::to_owned),
    }
}

#[tokio::test]
async fn public_bind_without_token_is_rejected_before_listen() {
    let result = NativeAdmin::start(public_config(true, None), ControlState::default()).await;
    let Err(error) = result else {
        panic!("public admin without a token must not start")
    };
    assert!(
        error.to_string().contains("public_admin=true"),
        "unexpected guard error: {error}"
    );
}

#[tokio::test]
async fn public_bind_without_opt_in_is_rejected_despite_token() {
    let result = NativeAdmin::start(
        public_config(false, Some("operator-token")),
        ControlState::default(),
    )
    .await;
    let Err(error) = result else {
        panic!("non-loopback admin without opt-in must not start")
    };
    assert!(
        error.to_string().contains("public_admin=true"),
        "unexpected guard error: {error}"
    );
}

#[tokio::test]
async fn public_bind_with_empty_token_is_rejected() {
    let result = NativeAdmin::start(public_config(true, Some("")), ControlState::default()).await;
    let Err(error) = result else {
        panic!("public admin with an empty token must not start")
    };
    assert!(
        error.to_string().contains("public_admin=true"),
        "unexpected guard error: {error}"
    );
}

/// Request-level bearer matrix against a loopback listener with a
/// configured token. Authorization applies whenever a token is set,
/// regardless of loopback status.
#[tokio::test]
async fn bearer_authorization_matrix_enforces_configured_token() {
    let token = "m059-baseline-operator-token";
    let mut handle = NativeAdmin::start(
        AdminConfig {
            bind: "127.0.0.1:0".parse().expect("loopback test address"),
            public_admin: false,
            auth_token: Some(token.to_owned()),
        },
        ControlState::default(),
    )
    .await
    .expect("loopback admin with a token starts");
    let health = format!("http://{}/v1/health", handle.local_addr());
    let client = eggfetch_core::Client::builder().build();

    // (authorization header value, expected status). `None` means the
    // header is omitted entirely.
    let cases: &[(&str, u16)] = &[
        ("Basic b3BlcmF0b3I=", 403),
        ("Bearer wrong-token", 403),
        ("Bearer ", 403),
        ("bearer m059-baseline-operator-token", 200),
        ("BEARER m059-baseline-operator-token", 200),
        ("Bearer m059-baseline-operator-token", 200),
    ];
    for (header, expected) in cases {
        let mut response = client
            .get(&health)
            .expect("health request builds")
            .header("authorization", header)
            .send()
            .await
            .expect("admin responds");
        assert_eq!(response.status(), *expected, "header: {header:?}");
        let body = response.bytes().await.expect("body reads");
        let text = String::from_utf8_lossy(&body);
        assert!(
            !text.contains(token),
            "response must never echo token material"
        );
    }

    let mut response = client
        .get(&health)
        .expect("health request builds")
        .send()
        .await
        .expect("admin responds");
    assert_eq!(response.status(), 403);
    let body = response.bytes().await.expect("body reads");
    assert!(!String::from_utf8_lossy(&body).contains(token));

    handle.shutdown();
    handle.wait().await;
}

#[tokio::test]
async fn auth_failures_are_throttled_per_peer_and_valid_tokens_are_not_counted() {
    let token = "01234567890123456789012345678901";
    let mut handle = NativeAdmin::start(
        AdminConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            auth_token: Some(token.to_owned()),
            ..AdminConfig::default()
        },
        ControlState::default(),
    )
    .await
    .expect("loopback admin starts");
    let url = format!("http://{}/v1/health", handle.local_addr());
    let client = eggfetch_core::Client::builder().build();

    for attempt in 0..5 {
        let mut response = client
            .get(&url)
            .unwrap()
            .header("authorization", "Bearer wrong-token")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403, "failure {attempt} should use burst");
        let _ = response.bytes().await.unwrap();
    }

    let authorization = format!("Bearer {token}");
    let mut valid = client
        .get(&url)
        .unwrap()
        .header("authorization", authorization.as_str())
        .send()
        .await
        .unwrap();
    assert_eq!(valid.status(), 200, "valid tokens are never throttled");
    let _ = valid.bytes().await.unwrap();

    let mut response = client
        .get(&url)
        .unwrap()
        .header("authorization", "Bearer wrong-token")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 429);
    assert_eq!(
        response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok()),
        Some("1")
    );
    let _ = response.bytes().await.unwrap();

    handle.shutdown();
    handle.wait().await;
}

#[tokio::test]
async fn short_public_token_is_rejected_again_at_startup() {
    let result = NativeAdmin::start(
        public_config(true, Some("short-token")),
        ControlState::default(),
    )
    .await;
    let Err(error) = result else {
        panic!("non-loopback admin with a short token must not start")
    };
    assert!(error.to_string().contains("auth_token"));
}

#[test]
fn file_config_rejects_insecure_public_admin_bind() {
    let missing =
        NativeConfig::parse("version = 1\n[admin]\nbind = '192.0.2.1:8475'\npublic_admin = true\n");
    assert!(missing.is_err(), "public bind without a token must fail");
    let empty = NativeConfig::parse(
        "version = 1\n[admin]\nbind = '192.0.2.1:8475'\npublic_admin = true\nauth_token = ''\n",
    );
    assert!(empty.is_err(), "public bind with an empty token must fail");
    let no_opt_in = NativeConfig::parse(
        "version = 1\n[admin]\nbind = '192.0.2.1:8475'\nauth_token = 'operator-token'\n",
    );
    assert!(no_opt_in.is_err(), "public bind without opt-in must fail");
    let weak = NativeConfig::parse(
        "version = 1\n[admin]\nbind = '192.0.2.1:8475'\npublic_admin = true\nauth_token = 'operator-token'\n",
    );
    assert!(weak.is_err(), "short public tokens must fail config parse");
    let token = "01234567890123456789012345678901";
    let secured = NativeConfig::parse(&format!(
        "version = 1\n[admin]\nbind = '192.0.2.1:8475'\npublic_admin = true\nauth_token = '{token}'\n"
    ))
    .expect("secured public bind with a strong token parses");
    let debug = format!("{secured:?}");
    assert!(
        !debug.contains(token),
        "file config Debug must redact token material"
    );
    assert!(debug.contains("[REDACTED]"));
}
