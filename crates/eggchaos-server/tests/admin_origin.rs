//! M063 regression tests for Host/Origin and JSON media-type policy.

use std::{fmt::Write as _, time::Duration};

use eggchaos_server::{AdminConfig, ControlState, NativeAdmin};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

async fn send_request(
    address: std::net::SocketAddr,
    host: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, String) {
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        write!(request, "{name}: {value}\r\n").expect("write request header");
    }
    request.push_str("\r\n");
    request.push_str(body);

    let mut stream = TcpStream::connect(address).await.expect("connect to admin");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write raw request");
    let mut response = Vec::new();
    timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
        .await
        .expect("admin response closes promptly")
        .expect("read response");
    let response = String::from_utf8_lossy(&response).into_owned();
    let status = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse().ok())
        .expect("HTTP status line");
    (status, response)
}

#[tokio::test]
async fn host_origin_and_media_type_guards_reject_audit_shapes_before_mutation() {
    let state = ControlState::default();
    let mut admin = NativeAdmin::start_with_allowed_hosts(
        AdminConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            ..AdminConfig::default()
        },
        state.clone(),
        Some(vec!["api.example".into()]),
    )
    .await
    .expect("start loopback admin");
    let address = admin.local_addr();
    let local_host = format!("127.0.0.1:{}", address.port());
    let proxy_body = r#"{"name":"csrf-proof","listen":"127.0.0.1:0","upstream":"127.0.0.1:9"}"#;

    let (status, _) = send_request(
        address,
        &format!("evil.example:{}", address.port()),
        "GET",
        "/v1/health",
        &[("Origin", "https://evil.example")],
        "",
    )
    .await;
    assert_eq!(status, 403, "foreign Host must be rejected on reads");

    let (status, _) = send_request(
        address,
        &local_host,
        "POST",
        "/v1/proxies",
        &[
            ("Origin", "https://evil.example"),
            ("Content-Type", "application/json"),
        ],
        proxy_body,
    )
    .await;
    assert_eq!(status, 403, "foreign Origin must be rejected on mutations");
    assert!(
        state.list().await.is_empty(),
        "foreign Origin must not mutate state"
    );

    let (status, _) = send_request(
        address,
        &local_host,
        "POST",
        "/v1/reset",
        &[("Origin", "null")],
        "",
    )
    .await;
    assert_eq!(status, 403, "null Origin must be rejected on mutations");

    // The audit's text/plain JSON probe is rejected as unsupported media type.
    // Media type is checked before Origin so the probe's documented 415 stays stable.
    let (status, _) = send_request(
        address,
        &local_host,
        "POST",
        "/v1/proxies",
        &[
            ("Origin", "https://evil.example"),
            ("Content-Type", "text/plain"),
        ],
        proxy_body,
    )
    .await;
    assert_eq!(status, 415, "non-JSON body must be rejected");
    assert!(
        state.list().await.is_empty(),
        "bad media type must not mutate state"
    );

    let (status, _) =
        send_request(address, &local_host, "POST", "/v1/proxies", &[], proxy_body).await;
    assert_eq!(status, 415, "missing media type must be rejected");
    assert!(
        state.list().await.is_empty(),
        "missing media type must not mutate state"
    );

    let (status, _) = send_request(
        address,
        "api.example",
        "POST",
        "/v1/proxies",
        &[
            ("Origin", "https://api.example"),
            ("Content-Type", "application/json"),
        ],
        proxy_body,
    )
    .await;
    assert_eq!(
        status, 201,
        "explicit reverse-proxy host and origin should work"
    );
    assert_eq!(state.list().await.len(), 1);

    admin.shutdown();
    admin.wait().await;
}
