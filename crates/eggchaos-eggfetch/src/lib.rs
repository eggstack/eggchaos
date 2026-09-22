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
    run_seed: u64,
    proxy_name: Arc<str>,
    connection_ordinal: Arc<AtomicU64>,
    connect_timeout: Duration,
}

impl ChaosDialer {
    /// Create a dialer with empty policies.
    pub fn new(run_seed: u64, proxy_name: impl Into<Arc<str>>) -> Self {
        Self::with_policies(run_seed, proxy_name, FaultPlan::empty(), FaultPlan::empty())
    }
    /// Create a dialer with validated initial directional plans.
    pub fn with_policies(
        run_seed: u64,
        proxy_name: impl Into<Arc<str>>,
        upstream: FaultPlan,
        downstream: FaultPlan,
    ) -> Self {
        Self {
            upstream: LivePolicy::new(upstream),
            downstream: LivePolicy::new(downstream),
            run_seed,
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
    /// Publish a new upstream generation.
    pub fn publish_upstream(&self, plan: FaultPlan) -> Result<u64, eggchaos_core::ValidationError> {
        self.upstream.publish(plan)
    }
    /// Publish a new downstream generation.
    pub fn publish_downstream(
        &self,
        plan: FaultPlan,
    ) -> Result<u64, eggchaos_core::ValidationError> {
        self.downstream.publish(plan)
    }
}

impl Dialer for ChaosDialer {
    fn dial(&self, target: DialTarget) -> DialFuture<'_> {
        let timeout_duration = self.connect_timeout;
        let run_seed = self.run_seed;
        let proxy_name = self.proxy_name.clone();
        let upstream = self.upstream.clone();
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
                        let downstream = upstream.clone();
                        let stream = BidirectionalChaosStream::new_live(
                            stream,
                            upstream,
                            downstream,
                            run_seed,
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
}
