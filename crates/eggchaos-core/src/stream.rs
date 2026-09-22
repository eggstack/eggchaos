use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::{Direction, DirectionEngine, FaultPlan};

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
        })
    }
    /// Wrap a stream with an empty plan.
    pub fn passthrough(inner: T, direction: Direction) -> Self {
        Self {
            inner,
            engine: DirectionEngine::empty(),
            direction,
        }
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
        if !this.engine.queue_is_empty() && this.engine.poll_flush(cx, &mut this.inner).is_pending()
        {
            return Poll::Pending;
        }
        let accepted = this.engine.accept(bytes);
        if accepted == 0 {
            return Poll::Pending;
        }
        Poll::Ready(Ok(accepted))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        this.engine.poll_flush(cx, &mut this.inner)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
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

impl DirectionEngine {
    pub(crate) fn queue_is_empty(&self) -> bool {
        self.is_empty()
            || self.evidence().bytes_accepted
                == self.evidence().bytes_forwarded + self.evidence().bytes_discarded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
}
