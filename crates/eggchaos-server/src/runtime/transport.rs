use serde::{Deserialize, Serialize};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::Duration,
};
use tokio::net::TcpStream;

/// How a hard-reset request resolved at the concrete transport edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResetResult {
    /// An abortive close was initiated through a supported socket API.
    /// Wire-level RST observation remains platform-dependent (M013).
    Applied,
    /// The reset was attempted but the socket operation failed.
    Failed(String),
    /// No resettable transport existed when the request resolved (for
    /// example the relay had already failed and the sockets were gone).
    Unsupported(String),
}

/// Shared abortive-close control for one TCP socket.
///
/// The relay owns the wrapped streams, so the connection task cannot reclaim
/// them to call socket options directly. Instead the wrapper carries shared
/// flags: on a hard-reset request the task sets the flag and drops the relay
/// future, and the wrapper's `Drop` applies `SO_LINGER=0` before close,
/// which produces an abortive close (RST) on supported platforms. The
/// outcome is recorded into the shared slot so evidence stays truthful.
#[derive(Debug, Default)]
pub struct TcpResetHandle {
    reset_on_close: AtomicBool,
    outcome: StdMutex<Option<ResetResult>>,
}

impl TcpResetHandle {
    /// Request an abortive close when the socket is released.
    pub fn request_reset(&self) {
        self.reset_on_close.store(true, Ordering::Release);
    }
    /// Return the recorded close outcome, if the socket was released.
    pub fn outcome(&self) -> Option<ResetResult> {
        self.outcome.lock().ok()?.clone()
    }
}

/// A `TcpStream` wrapper that can apply an abortive close on drop when its
/// shared handle requests it. Ordinary drops close gracefully (FIN).
pub struct ResettableTcpStream {
    inner: Option<TcpStream>,
    handle: Arc<TcpResetHandle>,
}

impl ResettableTcpStream {
    /// Wrap a socket, returning the stream and its shared reset handle.
    pub fn new(stream: TcpStream) -> (Self, Arc<TcpResetHandle>) {
        let handle = Arc::new(TcpResetHandle::default());
        (
            Self {
                inner: Some(stream),
                handle: handle.clone(),
            },
            handle,
        )
    }
}

impl Drop for ResettableTcpStream {
    fn drop(&mut self) {
        let Some(stream) = self.inner.take() else {
            return;
        };
        if !self.reset_on_close_requested() {
            return;
        }
        // Abortive close through stable APIs only: converting to a
        // socket2 handle lets us set zero linger before close, which
        // discards buffers and emits RST on platforms that honor it.
        // `std::net::TcpStream::set_linger` is still unstable, so socket2
        // is used instead of unstable standard APIs or unsafe code.
        let result = stream
            .into_std()
            .map_err(|error| error.to_string())
            .and_then(|std_stream| {
                socket2::Socket::from(std_stream)
                    .set_linger(Some(Duration::ZERO))
                    .map_err(|error| error.to_string())
                // Dropping the socket here closes abortively (RST) on
                // platforms that honor zero linger.
            });
        let outcome = match result {
            Ok(()) => ResetResult::Applied,
            Err(reason) => ResetResult::Failed(reason),
        };
        if let Ok(mut slot) = self.handle.outcome.lock() {
            *slot = Some(outcome);
        }
    }
}

impl ResettableTcpStream {
    fn reset_on_close_requested(&self) -> bool {
        self.handle.reset_on_close.load(Ordering::Acquire)
    }
}

impl tokio::io::AsyncRead for ResettableTcpStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let stream = self
            .inner
            .as_mut()
            .expect("ResettableTcpStream is live while polled");
        std::pin::Pin::new(stream).poll_read(cx, buffer)
    }
}

impl tokio::io::AsyncWrite for ResettableTcpStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        let stream = self
            .inner
            .as_mut()
            .expect("ResettableTcpStream is live while polled");
        std::pin::Pin::new(stream).poll_write(cx, bytes)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let stream = self
            .inner
            .as_mut()
            .expect("ResettableTcpStream is live while polled");
        std::pin::Pin::new(stream).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let stream = self
            .inner
            .as_mut()
            .expect("ResettableTcpStream is live while polled");
        std::pin::Pin::new(stream).poll_shutdown(cx)
    }
    fn is_write_vectored(&self) -> bool {
        self.inner
            .as_ref()
            .map(tokio::io::AsyncWrite::is_write_vectored)
            .unwrap_or(false)
    }
}
