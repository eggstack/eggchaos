//! Expanded Eggfetch regression matrix (M013 WP4).
//!
//! Each test drives real HTTP through [`ChaosDialer`] physical streams:
//! H1 keep-alive with live policy updates, HTTPS trust/rejection, H2
//! concurrency, blackhole timeouts, mid-response termination, downstream
//! shaping, idle-close redial, and dial error shape.

use std::{num::NonZeroU64, sync::Arc, time::Duration};

use eggchaos_core::{
    BandwidthConfig, BlackholeConfig, DisconnectConfig, FaultKind, FaultPlan, FaultSpec,
};
use eggchaos_eggfetch::ChaosDialer;
use eggfetch_core::Dialer;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::timeout,
};

const ROUND: Duration = Duration::from_secs(20);

fn fault(id: &str, kind: FaultKind) -> FaultSpec {
    FaultSpec {
        id: eggchaos_core::FaultId::new(id).unwrap(),
        probability: eggchaos_core::Probability::new(1.0).unwrap(),
        kind,
    }
}

fn downstream_plan(kind: FaultKind) -> FaultPlan {
    FaultPlan::new(vec![fault("down", kind)]).unwrap()
}

/// Minimal H1 origin server. `respond` maps a request target to the raw
/// response bytes; when `hits` is set it counts accepted connections.
async fn h1_origin(
    respond: impl Fn(String) -> Vec<u8> + Send + Sync + 'static,
    hits: Option<Arc<std::sync::atomic::AtomicUsize>>,
) -> std::net::SocketAddr {
    use std::sync::atomic::Ordering;
    let respond = Arc::new(respond);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            if let Some(hits) = &hits {
                hits.fetch_add(1, Ordering::AcqRel);
            }
            let respond = respond.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                loop {
                    let Ok(n) = stream.read(&mut buf).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let target = head
                        .lines()
                        .next()
                        .unwrap_or("")
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .to_owned();
                    let response = respond(target);
                    if stream.write_all(&response).await.is_err() {
                        return;
                    }
                    if response.windows(19).any(|w| w == b"Connection: close") {
                        return;
                    }
                }
            });
        }
    });
    addr
}

fn ok(body: &[u8]) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
        body.len()
    )
    .into_bytes()
    .into_iter()
    .chain(body.iter().copied())
    .collect()
}

#[tokio::test]
async fn h1_keepalive_survives_live_policy_update() {
    let addr = h1_origin(|_| ok(b"hello"), None).await;
    let dialer = ChaosDialer::new(11, "keepalive");
    let client = eggfetch_core::Client::builder()
        .dialer(dialer.clone())
        .build();
    let url = format!("http://127.0.0.1:{}/", addr.port());
    let mut first = client.get(&url).unwrap().send().await.unwrap();
    assert_eq!(first.bytes().await.unwrap().as_ref(), b"hello");
    // Live update on the pooled connection's policy: traffic keeps flowing,
    // now shaped by the new generation.
    dialer
        .publish_downstream(downstream_plan(FaultKind::Latency(
            eggchaos_core::LatencyConfig {
                delay: Duration::from_millis(150),
                jitter: Duration::ZERO,
                max_buffer_bytes: NonZeroU64::new(64 * 1024).unwrap(),
            },
        )))
        .unwrap();
    let start = std::time::Instant::now();
    let mut second = timeout(ROUND, client.get(&url).unwrap().send())
        .await
        .unwrap()
        .unwrap();
    let body = timeout(ROUND, second.bytes()).await.unwrap().unwrap();
    assert_eq!(body.as_ref(), b"hello");
    assert!(
        start.elapsed() >= Duration::from_millis(100),
        "live latency engages on keep-alive connection"
    );
}

#[tokio::test]
async fn https_valid_added_ca_is_trusted() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(
                certificate.cert.der().to_vec(),
            )],
            rustls::pki_types::PrivateKeyDer::from(rustls::pki_types::PrivatePkcs8KeyDer::from(
                certificate.key_pair.serialize_der(),
            )),
        )
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(server_config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = acceptor.accept(stream).await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await;
        stream.write_all(&ok(b"secure")).await.unwrap();
    });
    let tls = eggfetch_core::TlsConfig::builder()
        .additional_ca_certificate_der(vec![certificate.cert.der().to_vec()])
        .unwrap()
        .build();
    let client = eggfetch_core::Client::builder()
        .dialer(ChaosDialer::new(12, "tls-trust"))
        .tls_config(tls)
        .build();
    let mut response = timeout(
        ROUND,
        client
            .get(&format!("https://localhost:{}/", addr.port()))
            .unwrap()
            .send(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.bytes().await.unwrap().as_ref(), b"secure");
}

#[tokio::test]
async fn https_invalid_certificate_is_rejected() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(
                certificate.cert.der().to_vec(),
            )],
            rustls::pki_types::PrivateKeyDer::from(rustls::pki_types::PrivatePkcs8KeyDer::from(
                certificate.key_pair.serialize_der(),
            )),
        )
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(server_config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let _ = acceptor.accept(stream).await;
        }
    });
    // Default trust (no added CA, no danger flag): self-signed must fail.
    let client = eggfetch_core::Client::builder()
        .dialer(ChaosDialer::new(13, "tls-reject"))
        .build();
    let outcome = timeout(
        ROUND,
        client
            .get(&format!("https://localhost:{}/", addr.port()))
            .unwrap()
            .send(),
    )
    .await
    .unwrap();
    let error = outcome.expect_err("self-signed cert must be rejected");
    let text = format!("{error:?}");
    assert!(
        !text.contains("PRIVATE KEY"),
        "errors must not leak key material"
    );
}

#[cfg(feature = "http2")]
#[tokio::test]
async fn h2_concurrent_streams_share_one_chaos_connection() {
    use bytes::Bytes;
    use eggfetch_core::HttpVersionPolicy;

    let _ = rustls::crypto::ring::default_provider().install_default();
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let mut server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(
                certificate.cert.der().to_vec(),
            )],
            rustls::pki_types::PrivateKeyDer::from(rustls::pki_types::PrivatePkcs8KeyDer::from(
                certificate.key_pair.serialize_der(),
            )),
        )
        .unwrap();
    server_config.alpn_protocols = vec![b"h2".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(server_config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let stream = acceptor.accept(stream).await.unwrap();
        let mut connection = h2::server::handshake(stream).await.unwrap();
        while let Some(Ok((request, mut respond))) = connection.accept().await {
            tokio::spawn(async move {
                let path = request.uri().path().to_owned();
                let response = http::Response::new(());
                let mut send = respond.send_response(response, false).unwrap();
                send.send_data(Bytes::from(path.into_bytes()), true)
                    .unwrap();
            });
        }
    });
    let tls = eggfetch_core::TlsConfig::builder()
        .additional_ca_certificate_der(vec![certificate.cert.der().to_vec()])
        .unwrap()
        .build();
    let client = std::sync::Arc::new(
        eggfetch_core::Client::builder()
            .dialer(ChaosDialer::new(14, "h2-concurrent"))
            .http_version_policy(HttpVersionPolicy::Http2Only)
            .tls_config(tls)
            .build(),
    );
    let base = format!("https://localhost:{}", addr.port());
    let mut tasks = Vec::new();
    for index in 0..4 {
        let client = client.clone();
        let url = format!("{base}/s{index}");
        tasks.push(tokio::spawn(async move {
            let mut response = timeout(ROUND, client.get(&url).unwrap().send())
                .await
                .unwrap()
                .unwrap();
            timeout(ROUND, response.bytes()).await.unwrap().unwrap()
        }));
    }
    for (index, task) in tasks.into_iter().enumerate() {
        let body = task.await.unwrap();
        assert_eq!(body.as_ref(), format!("/s{index}").as_bytes());
    }
}

#[tokio::test]
async fn blackhole_response_blocks_without_hanging_the_client() {
    let addr = h1_origin(|_| ok(b"never"), None).await;
    let dialer = ChaosDialer::with_policies(
        15,
        "blackhole",
        FaultPlan::empty(),
        downstream_plan(FaultKind::Blackhole(BlackholeConfig { close_after: None })),
    );
    let client = eggfetch_core::Client::builder().dialer(dialer).build();
    // The indefinite blackhole discards the response: nothing completes
    // within the window, and the client is free (not hung) once the round
    // is abandoned.
    let outcome = timeout(
        Duration::from_secs(2),
        client
            .get(&format!("http://127.0.0.1:{}/", addr.port()))
            .unwrap()
            .send(),
    )
    .await;
    assert!(outcome.is_err(), "blackholed response must not complete");
}

#[tokio::test]
async fn mid_response_termination_is_an_error_not_truncation() {
    let body = vec![0x5Au8; 200];
    let addr = h1_origin(move |_| ok(&body), None).await;
    let dialer = ChaosDialer::with_policies(
        16,
        "cut",
        FaultPlan::empty(),
        downstream_plan(FaultKind::LimitData(eggchaos_core::LimitDataConfig {
            bytes: NonZeroU64::new(50).unwrap(),
        })),
    );
    let client = eggfetch_core::Client::builder().dialer(dialer).build();
    let outcome = timeout(
        ROUND,
        client
            .get(&format!("http://127.0.0.1:{}/", addr.port()))
            .unwrap()
            .send(),
    )
    .await
    .unwrap();
    // Either the send or the body read must fail: a 50-byte prefix of a
    // 200-byte body must never surface as a successful response.
    match outcome {
        Err(_) => {}
        Ok(mut response) => {
            let read = timeout(ROUND, response.bytes()).await.unwrap();
            assert!(read.is_err(), "truncated body must error");
        }
    }
}

#[tokio::test]
async fn downstream_bandwidth_shapes_http_body() {
    let body = vec![0x11u8; 8192];
    let addr = h1_origin(move |_| ok(&body), None).await;
    let dialer = ChaosDialer::with_policies(
        17,
        "shaped",
        FaultPlan::empty(),
        downstream_plan(FaultKind::Bandwidth(BandwidthConfig {
            bytes_per_second: NonZeroU64::new(4096).unwrap(),
            burst_bytes: NonZeroU64::new(512).unwrap(),
        })),
    );
    let client = eggfetch_core::Client::builder().dialer(dialer).build();
    let start = std::time::Instant::now();
    let mut response = timeout(
        Duration::from_secs(30),
        client
            .get(&format!("http://127.0.0.1:{}/", addr.port()))
            .unwrap()
            .send(),
    )
    .await
    .unwrap()
    .unwrap();
    let received = timeout(Duration::from_secs(30), response.bytes())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received.len(), 8192);
    assert!(received.iter().all(|byte| *byte == 0x11));
    assert!(
        start.elapsed() >= Duration::from_millis(1200),
        "shaping must pace delivery"
    );
}

#[tokio::test]
async fn server_closed_idle_connection_redials() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let hits = Arc::new(AtomicUsize::new(0));
    let addr = h1_origin(
        |_| b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec(),
        Some(hits.clone()),
    )
    .await;
    let client = eggfetch_core::Client::builder()
        .dialer(ChaosDialer::new(18, "redial"))
        .build();
    let url = format!("http://127.0.0.1:{}/", addr.port());
    for _ in 0..2 {
        let mut response = timeout(ROUND, client.get(&url).unwrap().send())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.bytes().await.unwrap().as_ref(), b"ok");
    }
    assert_eq!(hits.load(Ordering::Acquire), 2);
}

#[tokio::test]
async fn refused_dial_reports_shaped_error() {
    // Bind then drop a listener to obtain a certainly-closed port.
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dialer = ChaosDialer::new(19, "refused");
    let outcome = dialer
        .dial(eggfetch_core::DialTarget::new("127.0.0.1", closed))
        .await;
    let error = match outcome {
        Err(error) => error,
        Ok(_) => panic!("closed port must fail"),
    };
    assert!(
        error.message().contains("direct route"),
        "unexpected dial error: {}",
        error.message()
    );
}

#[tokio::test]
async fn disconnect_fault_terminates_http_stream() {
    // Origin sends headers then stalls with the connection open: the
    // graceful downstream disconnect must cut the stream (truncation error),
    // not deliver a forged-complete body and not hang.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nstalled")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let dialer = ChaosDialer::with_policies(
        20,
        "drop",
        FaultPlan::empty(),
        downstream_plan(FaultKind::Disconnect(DisconnectConfig {
            after: Duration::ZERO,
            hard_reset: false,
        })),
    );
    let client = eggfetch_core::Client::builder().dialer(dialer).build();
    let outcome = timeout(
        ROUND,
        client
            .get(&format!("http://127.0.0.1:{}/", addr.port()))
            .unwrap()
            .send(),
    )
    .await
    .unwrap();
    match outcome {
        Err(_) => {}
        Ok(mut response) => {
            let read = timeout(ROUND, response.bytes()).await.unwrap();
            assert!(read.is_err(), "terminated stream must error");
        }
    }
}
