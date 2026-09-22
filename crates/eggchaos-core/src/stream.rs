use std::{
    collections::VecDeque,
    io,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::{Direction, DirectionEngine, FaultPlan, LivePolicy};

/// Serializable counters for one wrapped stream direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DirectionSummary {
    /// Bytes accepted from the caller.
    pub bytes_accepted: u64,
    /// Bytes forwarded to the wrapped stream.
    pub bytes_forwarded: u64,
    /// Bytes intentionally discarded.
    pub bytes_discarded: u64,
    /// Accepted logical segments.
    pub segments: u64,
    /// Cumulative configured delay.
    pub injected_delay_ms: u64,
}

/// Error returned when a stream engine cannot be constructed.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The fault plan violates a native invariant.
    #[error(transparent)]
    Validation(#[from] crate::ValidationError),
}

/// A Tokio stream whose writes are passed through one directional fault plan.
pub struct ChaosStream<T> {
    inner: T,
    engine: DirectionEngine,
    direction: Direction,
    live_policy: Option<LivePolicy>,
    run_seed: u64,
    proxy: String,
    connection_key: u64,
    observed_generation: u64,
    pending_accept: Option<usize>,
}

impl<T> ChaosStream<T> {
    /// Wrap a stream using an explicit deterministic connection identity.
    pub fn new(
        inner: T,
        plan: FaultPlan,
        run_seed: u64,
        proxy: impl AsRef<str>,
        connection_key: u64,
        direction: Direction,
    ) -> Result<Self, EngineError> {
        Ok(Self {
            inner,
            engine: DirectionEngine::new(
                plan,
                run_seed,
                proxy.as_ref(),
                connection_key,
                direction,
            )?,
            direction,
            live_policy: None,
            run_seed,
            proxy: proxy.as_ref().to_owned(),
            connection_key,
            observed_generation: 0,
            pending_accept: None,
        })
    }
    /// Wrap a stream with an empty plan.
    pub fn passthrough(inner: T, direction: Direction) -> Self {
        Self {
            inner,
            engine: DirectionEngine::empty(),
            direction,
            live_policy: None,
            run_seed: 0,
            proxy: String::new(),
            connection_key: 0,
            observed_generation: 0,
            pending_accept: None,
        }
    }
    /// Wrap a stream with a generation-published live policy.
    pub fn new_live(
        inner: T,
        policy: LivePolicy,
        run_seed: u64,
        proxy: impl AsRef<str>,
        connection_key: u64,
        direction: Direction,
    ) -> Result<Self, EngineError> {
        let proxy = proxy.as_ref().to_owned();
        let generation = policy.generation();
        Ok(Self {
            inner,
            engine: DirectionEngine::new(
                policy.snapshot(),
                run_seed,
                &proxy,
                connection_key,
                direction,
            )?,
            direction,
            live_policy: Some(policy),
            run_seed,
            proxy,
            connection_key,
            observed_generation: generation,
            pending_accept: None,
        })
    }
    /// Return the configured direction.
    pub const fn direction(&self) -> Direction {
        self.direction
    }
    /// Access the current direction summary.
    pub fn summary(&self) -> DirectionSummary {
        let e = self.engine.evidence();
        DirectionSummary {
            bytes_accepted: e.bytes_accepted,
            bytes_forwarded: e.bytes_forwarded,
            bytes_discarded: e.bytes_discarded,
            segments: e.segments,
            injected_delay_ms: e.injected_delay_ms,
        }
    }
    /// Return the wrapped transport.
    pub fn into_inner(self) -> T {
        self.inner
    }
    /// Borrow the wrapped transport.
    pub fn get_ref(&self) -> &T {
        &self.inner
    }
    /// Mutably borrow the wrapped transport.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for ChaosStream<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buffer)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for ChaosStream<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.as_mut().get_mut();
        if this.engine.is_empty() {
            return Pin::new(&mut this.inner).poll_write(cx, bytes);
        }
        if let Some(accepted) = this.pending_accept {
            match this.engine.poll_flush(cx, &mut this.inner) {
                Poll::Ready(Ok(())) => {
                    this.pending_accept = None;
                    return Poll::Ready(Ok(accepted));
                }
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
        }
        if this.update_live(cx).is_pending() {
            return Poll::Pending;
        }
        if !this.engine.queue_is_empty() && this.engine.poll_flush(cx, &mut this.inner).is_pending()
        {
            return Poll::Pending;
        }
        let accepted = this.engine.accept(bytes);
        if accepted == 0 {
            return Poll::Pending;
        }
        match this.engine.poll_flush(cx, &mut this.inner) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(accepted)),
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => {
                this.pending_accept = Some(accepted);
                Poll::Pending
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.update_live(cx).is_pending() {
            return Poll::Pending;
        }
        this.engine.poll_flush(cx, &mut this.inner)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.update_live(cx).is_pending() {
            return Poll::Pending;
        }
        this.engine.poll_shutdown(cx, &mut this.inner)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        if self.engine.is_empty() {
            return Pin::new(&mut self.inner).poll_write_vectored(cx, bufs);
        }
        let mut joined = Vec::new();
        for buf in bufs {
            joined.extend_from_slice(buf);
        }
        self.poll_write(cx, &joined)
    }
}

impl<T: AsyncWrite + Unpin> ChaosStream<T> {
    fn update_live(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let Some(policy) = self.live_policy.clone() else {
            return Poll::Ready(());
        };
        if policy.generation() == self.observed_generation {
            return Poll::Ready(());
        }
        if !self.engine.queue_is_empty() && self.engine.poll_flush(cx, &mut self.inner).is_pending()
        {
            return Poll::Pending;
        }
        self.engine = DirectionEngine::new(
            policy.snapshot(),
            self.run_seed,
            &self.proxy,
            self.connection_key,
            self.direction,
        )
        .expect("published policies are validated");
        self.observed_generation = policy.generation();
        Poll::Ready(())
    }
}

impl DirectionEngine {
    pub(crate) fn queue_is_empty(&self) -> bool {
        self.is_empty()
            || self.evidence().bytes_accepted
                == self.evidence().bytes_forwarded + self.evidence().bytes_discarded
    }
}

/// A physical stream with independent write/upstream and read/downstream
/// policies. It is intended for in-process HTTP clients whose dialer owns one
/// full-duplex connection.
pub struct BidirectionalChaosStream<T> {
    inner: T,
    upstream: DirectionEngine,
    downstream: DirectionEngine,
    pending_accept: Option<usize>,
    output: VecDeque<u8>,
    upstream_policy: Option<LivePolicy>,
    downstream_policy: Option<LivePolicy>,
    observed_upstream: u64,
    observed_downstream: u64,
    run_seed: u64,
    proxy: String,
    connection_key: u64,
}

impl<T: AsyncRead + AsyncWrite + Unpin> BidirectionalChaosStream<T> {
    /// Construct a physical stream carrying two generation-published policies.
    pub fn new_live(
        inner: T,
        upstream: LivePolicy,
        downstream: LivePolicy,
        run_seed: u64,
        proxy: impl AsRef<str>,
        connection_key: u64,
    ) -> Result<Self, EngineError> {
        let proxy = proxy.as_ref().to_owned();
        let observed_upstream = upstream.generation();
        let observed_downstream = downstream.generation();
        Ok(Self {
            inner,
            upstream: DirectionEngine::new(
                upstream.snapshot(),
                run_seed,
                &proxy,
                connection_key,
                Direction::Upstream,
            )?,
            downstream: DirectionEngine::new(
                downstream.snapshot(),
                run_seed,
                &proxy,
                connection_key,
                Direction::Downstream,
            )?,
            pending_accept: None,
            output: VecDeque::new(),
            upstream_policy: Some(upstream),
            downstream_policy: Some(downstream),
            observed_upstream,
            observed_downstream,
            run_seed,
            proxy,
            connection_key,
        })
    }

    fn update_policies(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Some(policy) = self.upstream_policy.clone() {
            if policy.generation() != self.observed_upstream {
                if self.upstream.poll_flush(cx, &mut self.inner).is_pending() {
                    return Poll::Pending;
                }
                self.upstream = DirectionEngine::new(
                    policy.snapshot(),
                    self.run_seed,
                    &self.proxy,
                    self.connection_key,
                    Direction::Upstream,
                )
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
                self.observed_upstream = policy.generation();
            }
        }
        if let Some(policy) = self.downstream_policy.clone() {
            if policy.generation() != self.observed_downstream {
                if !self.output.is_empty() {
                    return Poll::Ready(Ok(()));
                }
                if !self.downstream.queue_is_empty()
                    && self
                        .downstream
                        .poll_flush(cx, &mut CaptureWriter(&mut self.output))
                        .is_pending()
                {
                    return Poll::Pending;
                }
                self.downstream = DirectionEngine::new(
                    policy.snapshot(),
                    self.run_seed,
                    &self.proxy,
                    self.connection_key,
                    Direction::Downstream,
                )
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
                self.observed_downstream = policy.generation();
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncRead for BidirectionalChaosStream<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        match this.update_policies(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Pending => return Poll::Pending,
        }
        if this.downstream.is_empty() {
            return Pin::new(&mut this.inner).poll_read(cx, buffer);
        }
        loop {
            if !this.output.is_empty() {
                let take = buffer.remaining().min(this.output.len());
                for _ in 0..take {
                    buffer.put_slice(&[this.output.pop_front().expect("length checked")]);
                }
                return Poll::Ready(Ok(()));
            }
            if !this.downstream.queue_is_empty()
                && this
                    .downstream
                    .poll_flush(cx, &mut CaptureWriter(&mut this.output))
                    .is_pending()
            {
                return Poll::Pending;
            }
            let mut temp = [0u8; 8192];
            let mut read = ReadBuf::new(&mut temp);
            match Pin::new(&mut this.inner).poll_read(cx, &mut read) {
                Poll::Ready(Ok(())) if read.filled().is_empty() => return Poll::Ready(Ok(())),
                Poll::Ready(Ok(())) => {
                    let accepted = this.downstream.accept(read.filled());
                    if accepted == 0 {
                        return Poll::Pending;
                    }
                }
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> AsyncWrite for BidirectionalChaosStream<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.as_mut().get_mut();
        match this.update_policies(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Pending => return Poll::Pending,
        }
        if this.upstream.is_empty() {
            return Pin::new(&mut this.inner).poll_write(cx, bytes);
        }
        if let Some(accepted) = this.pending_accept {
            if this.upstream.poll_flush(cx, &mut this.inner).is_pending() {
                return Poll::Pending;
            }
            this.pending_accept = None;
            return Poll::Ready(Ok(accepted));
        }
        if !this.upstream.queue_is_empty()
            && this.upstream.poll_flush(cx, &mut this.inner).is_pending()
        {
            return Poll::Pending;
        }
        let accepted = this.upstream.accept(bytes);
        if accepted == 0 {
            return Poll::Pending;
        }
        match this.upstream.poll_flush(cx, &mut this.inner) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(accepted)),
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => {
                this.pending_accept = Some(accepted);
                Poll::Pending
            }
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        match this.update_policies(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Pending => return Poll::Pending,
        };
        this.upstream.poll_flush(cx, &mut this.inner)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        match this.update_policies(cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
            Poll::Pending => return Poll::Pending,
        };
        this.upstream.poll_shutdown(cx, &mut this.inner)
    }
    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let mut joined = Vec::new();
        for buf in bufs {
            joined.extend_from_slice(buf);
        }
        self.as_mut().poll_write(cx, &joined)
    }
}

struct CaptureWriter<'a>(&'a mut VecDeque<u8>);
impl AsyncWrite for CaptureWriter<'_> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.0.extend(bytes);
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{num::NonZeroU64, time::Duration};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn fault(id: &str, kind: crate::FaultKind) -> crate::FaultSpec {
        crate::FaultSpec {
            id: crate::FaultId::new(id).unwrap(),
            probability: crate::Probability::new(1.0).unwrap(),
            kind,
        }
    }

    #[tokio::test]
    async fn empty_plan_is_transparent() {
        let (mut left, right) = tokio::io::duplex(16);
        let mut wrapped = ChaosStream::passthrough(right, Direction::Upstream);
        left.write_all(b"hello").await.unwrap();
        let mut out = [0; 5];
        wrapped.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"hello");
    }

    #[tokio::test]
    async fn empty_plan_delegates_shutdown() {
        let (left, right) = tokio::io::duplex(16);
        let mut wrapped = ChaosStream::passthrough(right, Direction::Upstream);
        drop(left);
        let mut out = [0; 1];
        assert_eq!(wrapped.read(&mut out).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn limit_data_stops_at_exact_boundary() {
        let plan = crate::FaultPlan::new(vec![fault(
            "limit",
            crate::FaultKind::LimitData(crate::LimitDataConfig {
                bytes: NonZeroU64::new(3).unwrap(),
            }),
        )])
        .unwrap();
        let (mut peer, right) = tokio::io::duplex(32);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        assert_eq!(wrapped.write(b"abcdef").await.unwrap(), 6);
        wrapped.flush().await.unwrap();
        let mut out = [0; 3];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"abc");
        assert_eq!(wrapped.summary().bytes_forwarded, 3);
    }

    #[tokio::test(start_paused = true)]
    async fn latency_is_bounded_and_flushable() {
        let plan = crate::FaultPlan::new(vec![fault(
            "latency",
            crate::FaultKind::Latency(crate::LatencyConfig {
                delay: Duration::from_millis(100),
                jitter: Duration::ZERO,
                max_buffer_bytes: NonZeroU64::new(32).unwrap(),
            }),
        )])
        .unwrap();
        let (mut peer, right) = tokio::io::duplex(32);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        wrapped.write_all(b"abc").await.unwrap();
        let flush = tokio::spawn(async move {
            wrapped.flush().await.unwrap();
            wrapped
        });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(100)).await;
        let wrapped = flush.await.unwrap();
        let mut out = [0; 3];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"abc");
        assert!(wrapped.summary().injected_delay_ms >= 100);
    }

    #[tokio::test]
    async fn blackhole_counts_discarded_bytes() {
        let plan = crate::FaultPlan::new(vec![fault(
            "hole",
            crate::FaultKind::Blackhole(crate::BlackholeConfig { close_after: None }),
        )])
        .unwrap();
        let (_peer, right) = tokio::io::duplex(32);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        assert_eq!(wrapped.write(b"abcdef").await.unwrap(), 6);
        assert_eq!(wrapped.summary().bytes_discarded, 6);
    }

    #[tokio::test]
    async fn live_policy_publishes_after_existing_queue_drains() {
        let initial = crate::FaultPlan::new(vec![fault(
            "latency",
            crate::FaultKind::Latency(crate::LatencyConfig {
                delay: Duration::from_millis(10),
                jitter: Duration::ZERO,
                max_buffer_bytes: NonZeroU64::new(32).unwrap(),
            }),
        )])
        .unwrap();
        let policy = crate::LivePolicy::new(initial);
        let (mut peer, right) = tokio::io::duplex(32);
        let mut wrapped =
            ChaosStream::new_live(right, policy.clone(), 7, "p", 1, Direction::Upstream).unwrap();
        wrapped.write_all(b"a").await.unwrap();
        policy.publish(crate::FaultPlan::empty()).unwrap();
        wrapped.flush().await.unwrap();
        wrapped.write_all(b"b").await.unwrap();
        let mut out = [0; 2];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"ab");
        assert_eq!(policy.generation(), 2);
    }

    #[tokio::test]
    async fn bidirectional_downstream_policy_updates_from_empty() {
        let upstream = crate::LivePolicy::new(crate::FaultPlan::empty());
        let downstream = crate::LivePolicy::new(crate::FaultPlan::empty());
        let (mut peer, right) = tokio::io::duplex(32);
        let wrapped =
            BidirectionalChaosStream::new_live(right, upstream, downstream.clone(), 7, "p", 1)
                .unwrap();
        let read_task = tokio::spawn(async move {
            let mut wrapped = wrapped;
            let mut out = [0; 1];
            let _ = wrapped.read(&mut out).await;
        });
        tokio::task::yield_now().await;
        downstream
            .publish(
                crate::FaultPlan::new(vec![fault(
                    "hole",
                    crate::FaultKind::Blackhole(crate::BlackholeConfig { close_after: None }),
                )])
                .unwrap(),
            )
            .unwrap();
        peer.write_all(b"x").await.unwrap();
        tokio::task::yield_now().await;
        assert!(!read_task.is_finished());
        read_task.abort();
    }

    #[test]
    fn slice_and_probability_validation_are_explicit() {
        assert!(crate::Probability::new(0.0).is_ok());
        assert!(crate::Probability::new(1.0).is_ok());
        assert!(crate::Probability::new(-0.1).is_err());
        assert!(crate::Probability::new(1.1).is_err());
        let bad = fault(
            "slice",
            crate::FaultKind::Slice(crate::SliceConfig {
                average_size: NonZeroU64::new(4).unwrap(),
                variation: 4,
                delay: Duration::ZERO,
            }),
        );
        assert!(crate::FaultPlan::new(vec![bad]).is_err());
    }
}
