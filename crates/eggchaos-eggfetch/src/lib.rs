//! In-process Eggfetch `Dialer` integration.
//!
//! Eggfetch remains the authority for HTTP framing, pooling, TLS, SNI, and
//! certificate verification. This adapter owns only raw direct TCP dialing
//! and the physical connection's deterministic fault policy.

use std::{
    io,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use eggchaos_core::{BidirectionalChaosStream, FaultPlan, LivePolicy};
use eggfetch_core::{DialError, DialErrorKind, DialFuture, DialStream, DialTarget, Dialer};
use tokio::{net::lookup_host, time::timeout};

/// A direct Eggfetch dialer carrying a live physical-stream fault policy.
#[derive(Clone)]
pub struct ChaosDialer {
    upstream: LivePolicy,
    downstream: LivePolicy,
    proxy_name: Arc<str>,
    connection_ordinal: Arc<AtomicU64>,
    connect_timeout: Duration,
}

impl ChaosDialer {
    /// Create a dialer with empty policies.
    pub fn new(run_seed: u64, proxy_name: impl Into<Arc<str>>) -> Self {
        Self::with_policies(run_seed, proxy_name, FaultPlan::empty(), FaultPlan::empty())
    }
    /// Create a dialer with validated initial directional plans. The seed
    /// is the namespace for both initial policy generations.
    pub fn with_policies(
        run_seed: u64,
        proxy_name: impl Into<Arc<str>>,
        upstream: FaultPlan,
        downstream: FaultPlan,
    ) -> Self {
        Self {
            upstream: LivePolicy::new(upstream, run_seed),
            downstream: LivePolicy::new(downstream, run_seed),
            proxy_name: proxy_name.into(),
            connection_ordinal: Arc::new(AtomicU64::new(1)),
            connect_timeout: Duration::from_secs(10),
        }
    }
    /// Set the bounded physical connect timeout.
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }
    /// Access the upstream live policy.
    pub fn upstream_policy(&self) -> LivePolicy {
        self.upstream.clone()
    }
    /// Access the downstream live policy.
    pub fn downstream_policy(&self) -> LivePolicy {
        self.downstream.clone()
    }
    /// Publish a new upstream generation, retaining the seed namespace.
    pub fn publish_upstream(&self, plan: FaultPlan) -> Result<u64, eggchaos_core::ValidationError> {
        let namespace = self.upstream.seed_namespace();
        self.upstream
            .publish(plan, namespace)
            .map(|snapshot| snapshot.generation)
    }
    /// Publish a new downstream generation, retaining the seed namespace.
    pub fn publish_downstream(
        &self,
        plan: FaultPlan,
    ) -> Result<u64, eggchaos_core::ValidationError> {
        let namespace = self.downstream.seed_namespace();
        self.downstream
            .publish(plan, namespace)
            .map(|snapshot| snapshot.generation)
    }
}

impl Dialer for ChaosDialer {
    fn dial(&self, target: DialTarget) -> DialFuture<'_> {
        let timeout_duration = self.connect_timeout;
        let proxy_name = self.proxy_name.clone();
        let upstream = self.upstream.clone();
        let downstream = self.downstream.clone();
        let ordinal = self.connection_ordinal.fetch_add(1, Ordering::AcqRel);
        Box::pin(async move {
            let addresses = lookup_host((target.host(), target.port()))
                .await
                .map_err(|error| {
                    DialError::with_source(
                        DialErrorKind::Connection,
                        "direct route resolution failed",
                        error,
                    )
                })?;
            let mut last_error = None;
            for address in addresses {
                match timeout(timeout_duration, tokio::net::TcpStream::connect(address)).await {
                    Ok(Ok(stream)) => {
                        let stream = BidirectionalChaosStream::new_live(
                            stream,
                            upstream,
                            downstream,
                            &*proxy_name,
                            ordinal,
                        )
                        .map_err(|error| DialError::new(DialErrorKind::Other, error.to_string()))?;
                        return Ok(Box::new(stream) as DialStream);
                    }
                    Ok(Err(error)) => last_error = Some(error),
                    Err(_) => {
                        return Err(DialError::new(
                            DialErrorKind::Timeout,
                            "direct route connect timed out",
                        ))
                    }
                }
            }
            Err(DialError::with_source(
                DialErrorKind::Connection,
                "direct route connection failed",
                last_error.unwrap_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "no resolved address")
                }),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggfetch_core::Dialer;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn dialer_connects_without_owning_http_or_tls() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            stream.write_all(b"ok").await.unwrap();
        });
        let dialer = ChaosDialer::new(42, "test");
        let target = DialTarget::new("127.0.0.1", address.port());
        let mut stream = dialer.dial(target).await.unwrap();
        let mut bytes = [0; 2];
        tokio::io::AsyncReadExt::read_exact(&mut stream, &mut bytes)
            .await
            .unwrap();
        assert_eq!(&bytes, b"ok");
        task.await.unwrap();
    }

    #[tokio::test]
    async fn eggfetch_owns_http_over_the_physical_chaos_stream() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            for _ in 0..2 {
                let mut request = [0; 256];
                let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut request)
                    .await
                    .unwrap();
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                    .await
                    .unwrap();
            }
        });
        let dialer = ChaosDialer::new(42, "test");
        let client = eggfetch_core::Client::builder().dialer(dialer).build();
        let mut response = client
            .get(&format!("http://127.0.0.1:{}/", address.port()))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.bytes().await.unwrap().as_ref(), b"ok");
        let mut second = client
            .get(&format!("http://127.0.0.1:{}/second", address.port()))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(second.bytes().await.unwrap().as_ref(), b"ok");
        task.await.unwrap();
    }

    #[cfg(feature = "http2")]
    #[tokio::test]
    async fn eggfetch_owns_tls_and_http2_over_the_physical_chaos_stream() {
        use bytes::Bytes;
        use eggfetch_core::{HttpVersionPolicy, TlsConfig};
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        use tokio_rustls::TlsAcceptor;

        let _ = rustls::crypto::ring::default_provider().install_default();
        let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(certificate.cert.der().to_vec())],
                PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
                    certificate.key_pair.serialize_der(),
                )),
            )
            .unwrap();
        let mut server_config = server_config;
        server_config.alpn_protocols = vec![b"h2".to_vec()];
        let acceptor = TlsAcceptor::from(std::sync::Arc::new(server_config));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let stream = acceptor.accept(stream).await.unwrap();
            let mut connection = h2::server::handshake(stream).await.unwrap();
            if let Some(Ok((_request, mut respond))) = connection.accept().await {
                let response = http::Response::new(());
                let mut send = respond.send_response(response, false).unwrap();
                send.send_data(Bytes::from_static(b"h2-ok"), true).unwrap();
            }
            connection.graceful_shutdown();
            while connection.accept().await.is_some() {}
        });

        let dialer = ChaosDialer::new(42, "h2-test");
        let client = eggfetch_core::Client::builder()
            .dialer(dialer)
            .http_version_policy(HttpVersionPolicy::Http2Only)
            .tls_config(TlsConfig::default().danger_accept_invalid_certs(true))
            .build();
        let mut response = client
            .get(&format!("https://localhost:{}/", address.port()))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.bytes().await.unwrap().as_ref(), b"h2-ok");
        task.await.unwrap();
    }
}
