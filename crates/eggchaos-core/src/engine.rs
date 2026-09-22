use std::{
    collections::VecDeque,
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
    time::{Duration, Instant},
};

use bytes::Bytes;
use tokio::{io::AsyncWrite, time::Sleep};

use crate::{derive_seed, DeterministicRng, Direction, FaultKind, FaultPlan, ValidationError};

/// A termination request emitted by an engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TerminationRequest {
    /// Close with the transport's normal shutdown operation.
    Graceful,
    /// Ask the embedding transport to attempt a hard reset.
    HardReset,
}

/// Direction-local counters and evidence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EngineEvidence {
    /// Bytes accepted from the caller.
    pub bytes_accepted: u64,
    /// Bytes written to the inner stream.
    pub bytes_forwarded: u64,
    /// Bytes intentionally discarded by a configured fault.
    pub bytes_discarded: u64,
    /// Number of logical segments accepted.
    pub segments: u64,
    /// Cumulative configured delay in milliseconds.
    pub injected_delay_ms: u64,
    /// The latest termination request, if any.
    pub termination: Option<TerminationRequest>,
}

#[derive(Debug)]
struct Queued {
    bytes: Bytes,
    release: Instant,
}

/// Poll-driven ordered fault state machine.
pub struct DirectionEngine {
    plan: FaultPlan,
    queue: VecDeque<Queued>,
    buffered: usize,
    max_buffer: usize,
    rngs: Vec<(bool, DeterministicRng)>,
    evidence: EngineEvidence,
    termination: Option<TerminationRequest>,
    sleep: Option<Pin<Box<Sleep>>>,
    shutdown_deadline: Option<Instant>,
    slow_close: Option<Duration>,
    remaining_limit: Option<u64>,
    blackhole_until: Option<Instant>,
    next_release: Instant,
}

impl DirectionEngine {
    /// Compile a plan for one connection/direction.
    pub fn new(
        plan: FaultPlan,
        run_seed: u64,
        proxy: &str,
        connection_key: u64,
        direction: Direction,
    ) -> Result<Self, ValidationError> {
        let mut max_buffer = 64 * 1024usize;
        let mut remaining_limit = None;
        let mut blackhole_until = None;
        let shutdown_deadline = None;
        let mut slow_close = None;
        let mut rngs = Vec::with_capacity(plan.faults().len());
        for fault in plan.faults() {
            let seed = derive_seed(run_seed, proxy, connection_key, direction, &fault.id);
            let mut rng = DeterministicRng::new(seed);
            let active = rng.bernoulli(fault.probability.get());
            match (active, fault.kind) {
                (true, FaultKind::Latency(c)) => {
                    max_buffer = max_buffer.min(c.max_buffer_bytes.get() as usize)
                }
                (true, FaultKind::LimitData(c)) => remaining_limit = Some(c.bytes.get()),
                (true, FaultKind::Blackhole(c)) => {
                    blackhole_until = c.close_after.map(|d| Instant::now() + d)
                }
                (true, FaultKind::SlowClose(c)) => slow_close = Some(c.delay),
                _ => {}
            }
            rngs.push((active, rng));
        }
        Ok(Self {
            plan,
            queue: VecDeque::new(),
            buffered: 0,
            max_buffer: max_buffer.max(1),
            rngs,
            evidence: EngineEvidence::default(),
            termination: None,
            sleep: None,
            shutdown_deadline,
            slow_close,
            remaining_limit,
            blackhole_until,
            next_release: Instant::now(),
        })
    }

    /// Construct the cheap empty path.
    pub fn empty() -> Self {
        Self {
            plan: FaultPlan::empty(),
            queue: VecDeque::new(),
            buffered: 0,
            max_buffer: 1,
            rngs: Vec::new(),
            evidence: EngineEvidence::default(),
            termination: None,
            sleep: None,
            shutdown_deadline: None,
            slow_close: None,
            remaining_limit: None,
            blackhole_until: None,
            next_release: Instant::now(),
        }
    }
    /// True when no stage is configured.
    pub fn is_empty(&self) -> bool {
        self.plan.is_empty()
    }
    /// Return a copy of the current evidence.
    pub fn evidence(&self) -> EngineEvidence {
        self.evidence
    }
    /// Return the current termination request.
    pub fn termination_request(&self) -> Option<TerminationRequest> {
        self.termination
    }
    /// Add a caller-visible accepted chunk to the bounded pipeline.
    pub fn accept(&mut self, input: &[u8]) -> usize {
        if input.is_empty() {
            return 0;
        }
        let mut accepted = input.len();
        if let Some(limit) = &mut self.remaining_limit {
            accepted = accepted.min(*limit as usize);
            *limit -= accepted as u64;
        }
        let mut discard = false;
        let now = Instant::now();
        // Match blackhole by kind without relying on a specific duration value.
        discard |= self
            .plan
            .faults()
            .iter()
            .zip(&self.rngs)
            .any(|(f, (active, _))| {
                *active
                    && matches!(f.kind, FaultKind::Blackhole(_))
                    && self.blackhole_until.is_none_or(|d| now < d)
            });
        if discard {
            self.evidence.bytes_accepted += accepted as u64;
            self.evidence.bytes_discarded += accepted as u64;
            if accepted < input.len() {
                self.termination = Some(TerminationRequest::Graceful);
            }
            return input.len();
        }
        if accepted == 0 {
            self.termination = Some(TerminationRequest::Graceful);
            return input.len();
        }
        let mut offset = 0usize;
        let slice_size = self
            .plan
            .faults()
            .iter()
            .zip(&self.rngs)
            .find_map(|(f, (active, _rng))| {
                if *active {
                    if let FaultKind::Slice(c) = f.kind {
                        Some(c.average_size.get().min(usize::MAX as u64) as usize)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .unwrap_or(accepted)
            .max(1);
        while offset < accepted {
            let end = (offset + slice_size).min(accepted);
            let mut release = self.next_release.max(now);
            for (fault, (active, rng)) in self.plan.faults().iter().zip(&mut self.rngs) {
                if !*active {
                    continue;
                }
                match fault.kind {
                    FaultKind::Latency(c) => {
                        let jitter_range = c.jitter.as_millis().min(u64::MAX as u128) as u64;
                        let jitter = if jitter_range == 0 {
                            0
                        } else {
                            rng.below(jitter_range.saturating_mul(2).saturating_add(1)) as i128
                                - jitter_range as i128
                        };
                        let millis = (c.delay.as_millis() as i128 + jitter).max(0) as u64;
                        release = release.max(now + Duration::from_millis(millis));
                        self.evidence.injected_delay_ms =
                            self.evidence.injected_delay_ms.saturating_add(millis);
                    }
                    FaultKind::Bandwidth(c) => {
                        let millis = ((end - offset) as u128 * 1000
                            / c.bytes_per_second.get() as u128)
                            .min(u64::MAX as u128) as u64;
                        release = release.max(now + Duration::from_millis(millis));
                    }
                    _ => {}
                }
            }
            if self.buffered + end - offset > self.max_buffer {
                break;
            }
            self.queue.push_back(Queued {
                bytes: Bytes::copy_from_slice(&input[offset..end]),
                release,
            });
            self.buffered += end - offset;
            self.evidence.segments += 1;
            offset = end;
            self.next_release = release;
        }
        self.evidence.bytes_accepted += offset as u64;
        if offset < accepted {
            return 0;
        }
        if accepted < input.len() {
            self.termination = Some(TerminationRequest::Graceful);
        }
        input.len()
    }

    fn poll_queue<T: AsyncWrite + Unpin>(
        &mut self,
        cx: &mut Context<'_>,
        inner: &mut T,
    ) -> Poll<io::Result<()>> {
        loop {
            let Some(front) = self.queue.front_mut() else {
                return Poll::Ready(Ok(()));
            };
            let now = Instant::now();
            if front.release > now {
                let sleep = self.sleep.get_or_insert_with(|| {
                    Box::pin(tokio::time::sleep(
                        front.release.saturating_duration_since(now),
                    ))
                });
                if sleep.as_mut().poll(cx).is_pending() {
                    return Poll::Pending;
                }
                self.sleep = None;
            }
            match Pin::new(&mut *inner).poll_write(cx, &front.bytes) {
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "inner stream accepted zero bytes",
                    )))
                }
                Poll::Ready(Ok(n)) => {
                    self.evidence.bytes_forwarded += n as u64;
                    self.buffered -= n;
                    if n == front.bytes.len() {
                        self.queue.pop_front();
                    } else {
                        front.bytes = front.bytes.slice(n..);
                    }
                }
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }

    /// Drive accepted bytes into the inner stream.
    pub fn poll_flush<T: AsyncWrite + Unpin>(
        &mut self,
        cx: &mut Context<'_>,
        inner: &mut T,
    ) -> Poll<io::Result<()>> {
        match self.poll_queue(cx, inner) {
            Poll::Ready(Ok(())) => Pin::new(inner).poll_flush(cx),
            other => other,
        }
    }

    /// Drive shutdown including a slow-close delay.
    pub fn poll_shutdown<T: AsyncWrite + Unpin>(
        &mut self,
        cx: &mut Context<'_>,
        inner: &mut T,
    ) -> Poll<io::Result<()>> {
        if self.poll_queue(cx, inner).is_pending() {
            return Poll::Pending;
        }
        if self.shutdown_deadline.is_none() {
            if let Some(delay) = self.slow_close {
                self.shutdown_deadline = Some(Instant::now() + delay);
            }
        }
        if let Some(deadline) = self.shutdown_deadline {
            if Instant::now() < deadline {
                let sleep = self.sleep.get_or_insert_with(|| {
                    Box::pin(tokio::time::sleep(
                        deadline.saturating_duration_since(Instant::now()),
                    ))
                });
                if sleep.as_mut().poll(cx).is_pending() {
                    return Poll::Pending;
                }
                self.sleep = None;
            }
        }
        Pin::new(inner).poll_shutdown(cx)
    }
}
