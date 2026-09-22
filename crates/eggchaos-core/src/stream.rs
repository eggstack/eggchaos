use std::{
    collections::VecDeque,
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::{
    Direction, DirectionEngine, FaultPlan, LivePolicy, PublishedPolicy, RngVersion,
    TerminationHandle, TerminationInfo, TerminationRequest, FAULT_TYPE_NAMES,
};

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
    /// Slicer-produced slices accepted.
    pub slices: u64,
    /// Bytes currently owned in the bounded queue.
    pub buffered_bytes: u64,
    /// High-water mark of owned queued bytes.
    pub high_water_bytes: u64,
    /// Cumulative configured latency delay.
    pub injected_delay_ms: u64,
    /// Cumulative bandwidth-throttle delay.
    pub throttled_delay_ms: u64,
    /// Per-fault-type activation counts, indexed by
    /// `FaultKind::type_index`.
    pub activations: [u64; 7],
    /// Latest termination request, if any.
    pub termination: Option<TerminationRequest>,
    /// Deterministic RNG contract version.
    pub rng_version: RngVersion,
}

/// Maximum fault identities retained in live evidence per direction.
pub const MAX_EVIDENCE_FAULTS: usize = 128;

/// One connection-active fault identity for evidence (no payloads).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ActiveFault {
    /// Stable fault identity.
    pub id: String,
    /// Low-cardinality fault type (`FAULT_TYPE_NAMES` spelling).
    pub fault_type: String,
}

/// Lock-shared live evidence for one stream direction, updated by
/// `ChaosStream` as bytes flow and generations transition. The embedding
/// runtime reads it without locking the stream itself.
#[derive(Debug, Default)]
pub struct StreamEvidence {
    observed_generation: AtomicU64,
    pending_generation: AtomicU64,
    seed_namespace: AtomicU64,
    bytes_accepted: AtomicU64,
    bytes_forwarded: AtomicU64,
    bytes_discarded: AtomicU64,
    high_water_bytes: AtomicU64,
    transitions: AtomicU64,
    activations: [AtomicU64; 7],
    active_faults: Mutex<(Vec<ActiveFault>, bool)>,
    /// Bytes moved through the empty-engine direct path, which bypasses
    /// engine counters. Mirroring adds these to accepted/forwarded so
    /// evidence covers no-fault traffic too.
    direct_bytes: AtomicU64,
}

impl StreamEvidence {
    /// Currently compiled policy generation (0 when no live policy).
    pub fn observed_generation(&self) -> u64 {
        self.observed_generation.load(Ordering::Relaxed)
    }
    /// Transition target draining the old generation, if any (0 = none).
    pub fn pending_generation(&self) -> u64 {
        self.pending_generation.load(Ordering::Relaxed)
    }
    /// Seed namespace the current engine compiled from.
    pub fn seed_namespace(&self) -> u64 {
        self.seed_namespace.load(Ordering::Relaxed)
    }
    /// Completed live-policy transitions.
    pub fn transitions(&self) -> u64 {
        self.transitions.load(Ordering::Relaxed)
    }
    /// Engine byte counters mirrored from the last drive.
    pub fn byte_counts(&self) -> (u64, u64, u64) {
        (
            self.bytes_accepted.load(Ordering::Relaxed),
            self.bytes_forwarded.load(Ordering::Relaxed),
            self.bytes_discarded.load(Ordering::Relaxed),
        )
    }
    /// High-water mark of owned queued bytes.
    pub fn high_water_bytes(&self) -> u64 {
        self.high_water_bytes.load(Ordering::Relaxed)
    }
    /// Per-fault-type activation counts.
    pub fn activations(&self) -> [u64; 7] {
        self.activations
            .each_ref()
            .map(|c| c.load(Ordering::Relaxed))
    }
    /// Connection-active fault identities and whether the list truncated.
    pub fn active_faults(&self) -> (Vec<ActiveFault>, bool) {
        self.active_faults.lock().expect("evidence lock").clone()
    }
    fn refresh_policy(&self, snapshot: &PublishedPolicy) {
        self.observed_generation
            .store(snapshot.generation, Ordering::Relaxed);
        self.seed_namespace
            .store(snapshot.seed_namespace, Ordering::Relaxed);
        let mut faults = Vec::new();
        let mut truncated = false;
        for fault in snapshot.plan.faults() {
            if faults.len() >= MAX_EVIDENCE_FAULTS {
                truncated = true;
                break;
            }
            faults.push(ActiveFault {
                id: fault.id.as_str().to_owned(),
                fault_type: FAULT_TYPE_NAMES[fault.kind.type_index()].to_owned(),
            });
        }
        *self.active_faults.lock().expect("evidence lock") = (faults, truncated);
    }
    fn mirror_engine(&self, engine: &DirectionEngine) {
        let evidence = engine.evidence();
        let direct = self.direct_bytes.load(Ordering::Relaxed);
        self.bytes_accepted.store(
            evidence.bytes_accepted.saturating_add(direct),
            Ordering::Relaxed,
        );
        self.bytes_forwarded.store(
            evidence.bytes_forwarded.saturating_add(direct),
            Ordering::Relaxed,
        );
        self.bytes_discarded
            .store(evidence.bytes_discarded, Ordering::Relaxed);
        self.high_water_bytes
            .store(evidence.high_water_bytes, Ordering::Relaxed);
        for (slot, value) in self.activations.iter().zip(evidence.activations) {
            slot.store(value, Ordering::Relaxed);
        }
    }
    fn note_direct(&self, bytes: u64) {
        if bytes > 0 {
            self.direct_bytes.fetch_add(bytes, Ordering::Relaxed);
            // The direct path accepts and forwards atomically: mirror
            // immediately so evidence never lags a completed write.
            self.bytes_accepted.fetch_add(bytes, Ordering::Relaxed);
            self.bytes_forwarded.fetch_add(bytes, Ordering::Relaxed);
        }
    }
}

/// Error returned when a stream engine cannot be constructed.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The fault plan violates a native invariant.
    #[error(transparent)]
    Validation(#[from] crate::ValidationError),
}

/// A Tokio stream whose writes are passed through one directional fault plan.
///
/// Bounded ownership contract: `poll_write` may report a write accepted as
/// soon as the engine owns the bytes inside its bounded queue, before they
/// reach the inner transport. `poll_flush` is the barrier guaranteeing every
/// preserving accepted byte has reached the inner writer.
pub struct ChaosStream<T> {
    inner: T,
    engine: DirectionEngine,
    direction: Direction,
    live_policy: Option<LivePolicy>,
    proxy: String,
    connection_key: u64,
    observed_generation: u64,
    evidence: Arc<StreamEvidence>,
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
        let engine = DirectionEngine::new(
            plan.clone(),
            run_seed,
            proxy.as_ref(),
            connection_key,
            direction,
        )?;
        let evidence = Arc::new(StreamEvidence::default());
        evidence.seed_namespace.store(run_seed, Ordering::Relaxed);
        evidence.refresh_policy(&PublishedPolicy {
            generation: 0,
            plan: Arc::new(plan),
            seed_namespace: run_seed,
        });
        Ok(Self {
            inner,
            engine,
            direction,
            live_policy: None,
            proxy: proxy.as_ref().to_owned(),
            connection_key,
            observed_generation: 0,
            evidence,
        })
    }
    /// Wrap a stream with an empty plan.
    pub fn passthrough(inner: T, direction: Direction) -> Self {
        Self {
            inner,
            engine: DirectionEngine::empty(),
            direction,
            live_policy: None,
            proxy: String::new(),
            connection_key: 0,
            observed_generation: 0,
            evidence: Arc::new(StreamEvidence::default()),
        }
    }
    /// Wrap a stream with a generation-published live policy. The engine
    /// compiles from one atomic snapshot, so plan, generation, and seed
    /// namespace always agree.
    pub fn new_live(
        inner: T,
        policy: LivePolicy,
        proxy: impl AsRef<str>,
        connection_key: u64,
        direction: Direction,
    ) -> Result<Self, EngineError> {
        let proxy = proxy.as_ref().to_owned();
        let snapshot = policy.snapshot();
        let engine = DirectionEngine::new(
            (*snapshot.plan).clone(),
            snapshot.seed_namespace,
            &proxy,
            connection_key,
            direction,
        )?;
        let evidence = Arc::new(StreamEvidence::default());
        evidence.refresh_policy(&snapshot);
        Ok(Self {
            inner,
            engine,
            direction,
            live_policy: Some(policy),
            proxy,
            connection_key,
            observed_generation: snapshot.generation,
            evidence,
        })
    }
    /// Return the configured direction.
    pub const fn direction(&self) -> Direction {
        self.direction
    }
    /// Generation this stream compiled its engine from.
    pub const fn observed_generation(&self) -> u64 {
        self.observed_generation
    }
    /// Live generation the stream has not yet transitioned to, if any.
    pub fn pending_generation(&self) -> Option<u64> {
        self.live_policy.as_ref().and_then(|policy| {
            let generation = policy.snapshot().generation;
            (generation != self.observed_generation).then_some(generation)
        })
    }
    /// Lock-shared live evidence for this direction.
    pub fn stream_evidence(&self) -> Arc<StreamEvidence> {
        self.evidence.clone()
    }
    /// Durable termination handle shared with the embedding runtime.
    pub fn termination_handle(&self) -> TerminationHandle {
        self.engine.termination_handle()
    }
    /// Latest termination evidence, if any.
    pub fn termination_info(&self) -> Option<TerminationInfo> {
        self.engine.termination_info()
    }
    /// Observe termination, arming deadline timers so a finite blackhole or
    /// delayed disconnect fires even with no further application write.
    pub fn poll_termination(&mut self, cx: &mut Context<'_>) -> Poll<Option<TerminationInfo>> {
        if self.engine.is_empty() {
            return Poll::Pending;
        }
        match self.engine.poll_due_termination(cx) {
            Poll::Ready(info) => Poll::Ready(Some(info)),
            Poll::Pending => Poll::Pending,
        }
    }
    /// Access the current direction summary, including transparent
    /// direct-path bytes.
    pub fn summary(&self) -> DirectionSummary {
        let e = self.engine.evidence();
        let direct = self.evidence.direct_bytes.load(Ordering::Relaxed);
        DirectionSummary {
            bytes_accepted: e.bytes_accepted.saturating_add(direct),
            bytes_forwarded: e.bytes_forwarded.saturating_add(direct),
            bytes_discarded: e.bytes_discarded,
            segments: e.segments,
            slices: e.slices,
            buffered_bytes: e.buffered_bytes,
            high_water_bytes: e.high_water_bytes,
            injected_delay_ms: e.injected_delay_ms,
            throttled_delay_ms: e.throttled_delay_ms,
            activations: e.activations,
            termination: e.termination,
            rng_version: e.rng_version,
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

/// Read-side pumping requires a bidirectional transport: only a stream
/// that can also be written can own queued writes worth driving.
impl<T: AsyncRead + AsyncWrite + Unpin> AsyncRead for ChaosStream<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        if !this.engine.is_empty() {
            // Read-side pump: the embedding relay never flushes mid-stream,
            // so a delay fault's queued bytes would sit until connection
            // close. Polling the write queue on every read drives releasable
            // bytes toward the inner transport without changing byte
            // semantics. Pump errors are ignored here; the write path owns
            // error reporting.
            let _ = this.engine.poll_flush(cx, &mut this.inner);
            this.evidence.mirror_engine(&this.engine);
            // A resolved graceful termination surfaces as EOF once the
            // accepted prefix has drained: the embedding relay observes the
            // FIN analog and ends, which lets the runtime record drain
            // evidence. Live inner bytes are never discarded: available data
            // is delivered first and only a would-be-idle read becomes EOF.
            // Hard-reset termination is owned by the runtime's abortive
            // close, so reads keep inner behavior for it.
            if this.engine.queue_is_empty()
                && matches!(
                    this.engine.termination_request(),
                    Some(TerminationRequest::Graceful)
                )
            {
                let filled_before = buffer.filled().len();
                match Pin::new(&mut this.inner).poll_read(cx, buffer) {
                    Poll::Ready(Ok(())) if buffer.filled().len() == filled_before => {
                        // Inner EOF passes through as EOF.
                        return Poll::Ready(Ok(()));
                    }
                    Poll::Ready(result) => return Poll::Ready(result),
                    Poll::Pending => return Poll::Ready(Ok(())),
                }
            }
        }
        Pin::new(&mut this.inner).poll_read(cx, buffer)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for ChaosStream<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.as_mut().get_mut();
        // Live generation must be observed before the empty-engine fast
        // path: a connection established without faults takes the direct
        // path, and a later publication still has to transition it to the
        // faulted engine without reconnecting.
        if this.update_live(cx).is_pending() {
            return Poll::Pending;
        }
        if this.engine.is_empty() {
            // Direct path: count accepted/forwarded bytes for evidence;
            // the engine never sees them.
            let outcome = Pin::new(&mut this.inner).poll_write(cx, bytes);
            if let Poll::Ready(Ok(written)) = outcome {
                this.evidence.note_direct(written as u64);
                return Poll::Ready(Ok(written));
            }
            return outcome;
        }
        // Opportunistically drive releasable bytes to free bounded capacity.
        // This arms release timers but never blocks acceptance: ownership
        // can complete before physical delivery.
        if let Poll::Ready(Err(error)) = this.engine.poll_flush(cx, &mut this.inner) {
            return Poll::Ready(Err(error));
        }
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if this.engine.termination_request().is_some() && this.engine.queue_is_empty() {
            return Poll::Ready(Err(terminated_error(this.engine.termination_info())));
        }
        let accepted = this.engine.accept(bytes);
        if accepted > 0 {
            // Drive already-due bytes immediately: the embedding relay never
            // flushes mid-stream, so due bytes would otherwise sit queued
            // until an unrelated read pump or flush. Not-due bytes stay
            // queued behind their release timers. Errors are owned by the
            // write path and surface on the next pre-accept drive.
            let _ = this.engine.poll_flush(cx, &mut this.inner);
            this.evidence.mirror_engine(&this.engine);
            return Poll::Ready(Ok(accepted));
        }
        // Nothing accepted: either bounded capacity is full (the drive above
        // armed the release timer, so retry after wakeup) or a termination
        // is due while the accepted prefix still drains.
        if this.engine.termination_request().is_some() && this.engine.queue_is_empty() {
            return Poll::Ready(Err(terminated_error(this.engine.termination_info())));
        }
        Poll::Pending
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.update_live(cx).is_pending() {
            return Poll::Pending;
        }
        let outcome = this.engine.poll_flush(cx, &mut this.inner);
        this.evidence.mirror_engine(&this.engine);
        outcome
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
        // Observe the live generation before the empty-engine fast path,
        // mirroring `poll_write`.
        if self.update_live(cx).is_pending() {
            return Poll::Pending;
        }
        if self.engine.is_empty() {
            match Pin::new(&mut self.inner).poll_write_vectored(cx, bufs) {
                Poll::Ready(Ok(written)) => {
                    self.evidence.note_direct(written as u64);
                    return Poll::Ready(Ok(written));
                }
                other => return other,
            }
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
        // One atomic load: the generation compared here always agrees with
        // the plan and seed namespace the transition compiles from.
        let snapshot = policy.snapshot();
        if snapshot.generation == self.observed_generation {
            return Poll::Ready(());
        }
        // Barrier transition: already accepted preserving bytes drain before
        // the engine is replaced, so a policy update never silently forgets
        // owned bytes. Already discarded blackhole bytes stay discarded.
        // The pending marker is visible to evidence readers while draining.
        self.evidence
            .pending_generation
            .store(snapshot.generation, Ordering::Relaxed);
        if !self.engine.queue_is_empty() && self.engine.poll_flush(cx, &mut self.inner).is_pending()
        {
            return Poll::Pending;
        }
        // The termination handle is shared across the swap, so a due
        // termination request survives the transition (first wins).
        let handle = self.engine.termination_handle();
        self.engine = DirectionEngine::new_with_termination(
            (*snapshot.plan).clone(),
            snapshot.seed_namespace,
            &self.proxy,
            self.connection_key,
            self.direction,
            handle,
        )
        .expect("published policies are validated");
        self.observed_generation = snapshot.generation;
        self.evidence.refresh_policy(&snapshot);
        self.evidence.pending_generation.store(0, Ordering::Relaxed);
        self.evidence.transitions.fetch_add(1, Ordering::Relaxed);
        self.evidence.mirror_engine(&self.engine);
        Poll::Ready(())
    }
}

/// Write error surfacing a durable engine termination request.
fn terminated_error(info: Option<TerminationInfo>) -> io::Error {
    let detail = match info {
        Some(TerminationInfo {
            fault_id: Some(id),
            direction,
            ..
        }) => format!("terminated by fault {id} ({})", direction.as_str()),
        Some(TerminationInfo { direction, .. }) => {
            format!("terminated ({})", direction.as_str())
        }
        None => "terminated".to_owned(),
    };
    io::Error::new(
        io::ErrorKind::ConnectionAborted,
        format!("eggchaos stream {detail}"),
    )
}

/// A physical stream with independent write/upstream and read/downstream
/// policies. It is intended for in-process HTTP clients whose dialer owns one
/// full-duplex connection.
pub struct BidirectionalChaosStream<T> {
    inner: T,
    upstream: DirectionEngine,
    downstream: DirectionEngine,
    output: VecDeque<u8>,
    upstream_policy: Option<LivePolicy>,
    downstream_policy: Option<LivePolicy>,
    observed_upstream: u64,
    observed_downstream: u64,
    proxy: String,
    connection_key: u64,
}

impl<T: AsyncRead + AsyncWrite + Unpin> BidirectionalChaosStream<T> {
    /// Construct a physical stream carrying two generation-published policies.
    /// Both engines compile from atomic snapshots, so plan, generation,
    /// and seed namespace always agree.
    pub fn new_live(
        inner: T,
        upstream: LivePolicy,
        downstream: LivePolicy,
        proxy: impl AsRef<str>,
        connection_key: u64,
    ) -> Result<Self, EngineError> {
        let proxy = proxy.as_ref().to_owned();
        let upstream_snapshot = upstream.snapshot();
        let downstream_snapshot = downstream.snapshot();
        let observed_upstream = upstream_snapshot.generation;
        let observed_downstream = downstream_snapshot.generation;
        Ok(Self {
            inner,
            upstream: DirectionEngine::new(
                (*upstream_snapshot.plan).clone(),
                upstream_snapshot.seed_namespace,
                &proxy,
                connection_key,
                Direction::Upstream,
            )?,
            downstream: DirectionEngine::new(
                (*downstream_snapshot.plan).clone(),
                downstream_snapshot.seed_namespace,
                &proxy,
                connection_key,
                Direction::Downstream,
            )?,
            output: VecDeque::new(),
            upstream_policy: Some(upstream),
            downstream_policy: Some(downstream),
            observed_upstream,
            observed_downstream,
            proxy,
            connection_key,
        })
    }

    /// Latest upstream termination evidence, if any.
    pub fn upstream_termination(&self) -> Option<TerminationInfo> {
        self.upstream.termination_info()
    }

    /// Latest downstream termination evidence, if any.
    pub fn downstream_termination(&self) -> Option<TerminationInfo> {
        self.downstream.termination_info()
    }

    fn update_policies(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Some(policy) = self.upstream_policy.clone() {
            let snapshot = policy.snapshot();
            if snapshot.generation != self.observed_upstream {
                if !self.upstream.queue_is_empty()
                    && self.upstream.poll_flush(cx, &mut self.inner).is_pending()
                {
                    return Poll::Pending;
                }
                let handle = self.upstream.termination_handle();
                self.upstream = DirectionEngine::new_with_termination(
                    (*snapshot.plan).clone(),
                    snapshot.seed_namespace,
                    &self.proxy,
                    self.connection_key,
                    Direction::Upstream,
                    handle,
                )
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
                self.observed_upstream = snapshot.generation;
            }
        }
        if let Some(policy) = self.downstream_policy.clone() {
            let snapshot = policy.snapshot();
            if snapshot.generation != self.observed_downstream {
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
                let handle = self.downstream.termination_handle();
                self.downstream = DirectionEngine::new_with_termination(
                    (*snapshot.plan).clone(),
                    snapshot.seed_namespace,
                    &self.proxy,
                    self.connection_key,
                    Direction::Downstream,
                    handle,
                )
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
                self.observed_downstream = snapshot.generation;
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
        if !this.upstream.is_empty() {
            // Read-side pump for the write direction, mirroring
            // `ChaosStream`: pooled physical connections only make progress
            // on delay faults when reads drive releasable queued writes.
            let _ = this.upstream.poll_flush(cx, &mut this.inner);
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
            if !this.downstream.queue_is_empty() {
                // Drive queued bytes into the output buffer, then loop back
                // to serve them before reading more from inner: inner may
                // already be at EOF (peer closed after writing), and an
                // inner read would strand the flushed bytes behind a
                // premature EOF. Flush errors propagate; the write path
                // owns no exclusive error reporting here.
                match this
                    .downstream
                    .poll_flush(cx, &mut CaptureWriter(&mut this.output))
                {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                    Poll::Ready(Ok(())) => {}
                }
                if !this.output.is_empty() {
                    continue;
                }
            }
            // Drained queue with a resolved termination ends the read side
            // here instead of waiting for inner EOF: unlike the relay
            // embedding, no runtime polls this stream's termination handle,
            // so graceful requests surface as EOF and hard resets as errors.
            // Live inner bytes were already delivered above; only a
            // would-be-idle read becomes EOF/error.
            match this.downstream.termination_request() {
                Some(TerminationRequest::Graceful) => return Poll::Ready(Ok(())),
                Some(TerminationRequest::HardReset) => {
                    let detail = this
                        .downstream
                        .termination_info()
                        .and_then(|info| info.fault_id)
                        .map(|id| format!("connection reset by fault {id}"))
                        .unwrap_or_else(|| "connection reset".into());
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::ConnectionReset,
                        detail,
                    )));
                }
                None => {}
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
        if let Poll::Ready(Err(error)) = this.upstream.poll_flush(cx, &mut this.inner) {
            return Poll::Ready(Err(error));
        }
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if this.upstream.termination_request().is_some() && this.upstream.queue_is_empty() {
            return Poll::Ready(Err(terminated_error(this.upstream.termination_info())));
        }
        let accepted = this.upstream.accept(bytes);
        if accepted > 0 {
            // Drive already-due bytes immediately: the embedding relay never
            // flushes mid-stream. See `ChaosStream::poll_write`.
            let _ = this.upstream.poll_flush(cx, &mut this.inner);
            return Poll::Ready(Ok(accepted));
        }
        if this.upstream.termination_request().is_some() && this.upstream.queue_is_empty() {
            return Poll::Ready(Err(terminated_error(this.upstream.termination_info())));
        }
        Poll::Pending
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
    use std::{future::poll_fn, num::NonZeroU64, time::Duration};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn fault(id: &str, kind: crate::FaultKind) -> crate::FaultSpec {
        crate::FaultSpec {
            id: crate::FaultId::new(id).unwrap(),
            probability: crate::Probability::new(1.0).unwrap(),
            kind,
        }
    }

    fn latency_plan(delay_ms: u64, buffer: u64) -> crate::FaultPlan {
        crate::FaultPlan::new(vec![fault(
            "latency",
            crate::FaultKind::Latency(crate::LatencyConfig {
                delay: Duration::from_millis(delay_ms),
                jitter: Duration::ZERO,
                max_buffer_bytes: NonZeroU64::new(buffer).unwrap(),
            }),
        )])
        .unwrap()
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
    async fn direct_path_bytes_count_toward_evidence() {
        // No-fault traffic bypasses the engine but must still reconcile
        // in evidence and summaries.
        let policy = crate::LivePolicy::new(crate::FaultPlan::empty(), 0);
        let (mut peer, right) = tokio::io::duplex(32);
        let mut wrapped =
            ChaosStream::new_live(right, policy, "p", 1, Direction::Upstream).unwrap();
        wrapped.write_all(b"abc").await.unwrap();
        let mut out = [0; 3];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"abc");
        let evidence = wrapped.stream_evidence();
        assert_eq!(evidence.byte_counts(), (3, 3, 0));
        assert_eq!(wrapped.summary().bytes_accepted, 3);
        assert_eq!(wrapped.summary().bytes_forwarded, 3);
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
    async fn limit_data_reports_only_accepted_prefix_then_terminates() {
        let plan = crate::FaultPlan::new(vec![fault(
            "limit",
            crate::FaultKind::LimitData(crate::LimitDataConfig {
                bytes: NonZeroU64::new(3).unwrap(),
            }),
        )])
        .unwrap();
        let (mut peer, right) = tokio::io::duplex(32);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        // The suffix beyond the limit is never reported as accepted.
        assert_eq!(wrapped.write(b"abcdef").await.unwrap(), 3);
        wrapped.flush().await.unwrap();
        let mut out = [0; 3];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"abc");
        assert_eq!(wrapped.summary().bytes_forwarded, 3);
        assert_eq!(
            wrapped.summary().termination,
            Some(crate::TerminationRequest::Graceful)
        );
        // Once the accepted prefix drains, further writes fail
        // deterministically instead of vanishing.
        let error = wrapped.write(b"d").await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
    }

    #[tokio::test]
    async fn limit_data_boundaries_are_exact() {
        for (limit, writes, expected) in [
            (1_u64, vec![b"ab".as_slice()], vec![1_usize]),
            (
                4_u64,
                vec![b"ab".as_slice(), b"cdef".as_slice()],
                vec![2, 2],
            ),
            (5_u64, vec![b"abcde".as_slice()], vec![5]),
        ] {
            let plan = crate::FaultPlan::new(vec![fault(
                "limit",
                crate::FaultKind::LimitData(crate::LimitDataConfig {
                    bytes: NonZeroU64::new(limit).unwrap(),
                }),
            )])
            .unwrap();
            let (_peer, right) = tokio::io::duplex(64);
            let mut wrapped =
                ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
            let mut total = 0;
            for (write, expect) in writes.iter().zip(expected.iter()) {
                let accepted = wrapped.write(write).await.unwrap();
                assert_eq!(accepted, *expect, "limit {limit}");
                total += accepted;
            }
            assert_eq!(total as u64, limit.min(6));
            wrapped.flush().await.unwrap();
            assert_eq!(wrapped.summary().bytes_forwarded, total as u64);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn latency_is_bounded_and_flushable() {
        let plan = latency_plan(100, 32);
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

    #[tokio::test(start_paused = true)]
    async fn latency_does_not_multiply_across_fragmented_writes() {
        let plan = latency_plan(100, 64);
        let (mut peer, right) = tokio::io::duplex(64);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        // Both segments are accepted before either deadline expires.
        wrapped.write_all(b"abcd").await.unwrap();
        wrapped.write_all(b"efgh").await.unwrap();
        assert_eq!(wrapped.summary().buffered_bytes, 8);
        let start = tokio::time::Instant::now();
        let flush = tokio::spawn(async move {
            wrapped.flush().await.unwrap();
            wrapped
        });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(100)).await;
        let wrapped = flush.await.unwrap();
        // One base delay releases the burst, not one delay per write.
        assert!(start.elapsed() < Duration::from_millis(200));
        let mut out = [0; 8];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"abcdefgh");
        assert_eq!(wrapped.summary().bytes_forwarded, 8);
    }

    #[tokio::test(start_paused = true)]
    async fn latency_queue_backpressures_at_capacity_and_wakes() {
        let plan = latency_plan(100, 4);
        let (mut peer, right) = tokio::io::duplex(64);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        wrapped.write_all(b"abcd").await.unwrap();
        // The bound is full and the head release is 100ms out: the next
        // write cannot complete within 10ms instead of allocating beyond it.
        let timeout = tokio::time::timeout(Duration::from_millis(10), wrapped.write(b"e")).await;
        assert!(timeout.is_err());
        tokio::time::advance(Duration::from_millis(100)).await;
        wrapped.flush().await.unwrap();
        // Capacity freed by the drain; the retry succeeds.
        assert_eq!(wrapped.write(b"e").await.unwrap(), 1);
        wrapped.flush().await.unwrap();
        let mut out = [0; 5];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"abcde");
        assert_eq!(wrapped.summary().high_water_bytes, 4);
    }

    #[tokio::test(start_paused = true)]
    async fn bandwidth_burst_then_sustained_rate() {
        let plan = crate::FaultPlan::new(vec![fault(
            "throttle",
            crate::FaultKind::Bandwidth(crate::BandwidthConfig {
                bytes_per_second: NonZeroU64::new(100).unwrap(),
                burst_bytes: NonZeroU64::new(100).unwrap(),
            }),
        )])
        .unwrap();
        let (mut peer, right) = tokio::io::duplex(512);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        // Initial burst passes without waiting.
        wrapped.write_all(&[1; 100]).await.unwrap();
        wrapped.flush().await.unwrap();
        let mut out = [0; 100];
        peer.read_exact(&mut out).await.unwrap();
        // The next 100 bytes need a full second at 100 B/s.
        wrapped.write_all(&[2; 100]).await.unwrap();
        let start = tokio::time::Instant::now();
        let flush = tokio::spawn(async move {
            wrapped.flush().await.unwrap();
            wrapped
        });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(1)).await;
        let wrapped = flush.await.unwrap();
        assert!(start.elapsed() >= Duration::from_secs(1));
        let mut out = [0; 100];
        peer.read_exact(&mut out).await.unwrap();
        assert!(out.iter().all(|b| *b == 2));
        assert!(wrapped.summary().throttled_delay_ms >= 1000);
    }

    #[tokio::test]
    async fn slicer_preserves_bytes_with_deterministic_variation() {
        // Golden sequence for seed 7 / proxy "p" / key 1 / upstream / "slice"
        // with average 8 and variation 3, from the SplitMix64-v1 contract.
        let plan = crate::FaultPlan::new(vec![fault(
            "slice",
            crate::FaultKind::Slice(crate::SliceConfig {
                average_size: NonZeroU64::new(8).unwrap(),
                variation: 3,
                delay: Duration::ZERO,
            }),
        )])
        .unwrap();
        let (mut peer, right) = tokio::io::duplex(128);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        let payload: Vec<u8> = (0..40).collect();
        wrapped.write_all(&payload).await.unwrap();
        // Every slice stays inside [average - variation, average + variation]
        // and the segmentation is deterministic for the fixed seed.
        let first = wrapped.summary();
        assert_eq!(first.bytes_accepted, 40);
        assert!(first.slices >= 4 && first.slices <= 8);
        wrapped.flush().await.unwrap();
        let mut out = [0; 40];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(out.to_vec(), payload);
        // Same identity reproduces the same segmentation under scheduling noise.
        let plan2 = crate::FaultPlan::new(vec![fault(
            "slice",
            crate::FaultKind::Slice(crate::SliceConfig {
                average_size: NonZeroU64::new(8).unwrap(),
                variation: 3,
                delay: Duration::ZERO,
            }),
        )])
        .unwrap();
        let (_peer2, right2) = tokio::io::duplex(128);
        let mut noisy = ChaosStream::new(right2, plan2, 7, "p", 1, Direction::Upstream).unwrap();
        tokio::task::yield_now().await;
        tokio::spawn(async {}).await.unwrap();
        noisy.write_all(&payload).await.unwrap();
        assert_eq!(noisy.summary().slices, first.slices);
    }

    #[tokio::test(start_paused = true)]
    async fn slicer_inter_slice_delay_staggers_delivery() {
        let plan = crate::FaultPlan::new(vec![fault(
            "slice",
            crate::FaultKind::Slice(crate::SliceConfig {
                average_size: NonZeroU64::new(4).unwrap(),
                variation: 0,
                delay: Duration::from_millis(50),
            }),
        )])
        .unwrap();
        let (mut peer, right) = tokio::io::duplex(64);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        wrapped.write_all(b"abcdefghijkl").await.unwrap();
        assert_eq!(wrapped.summary().slices, 3);
        // Three slices with a 50ms inter-slice delay drain at +0/+50/+100:
        // a once-per-write delay would finish at +50 instead of +100.
        let start = tokio::time::Instant::now();
        wrapped.flush().await.unwrap();
        assert_eq!(start.elapsed(), Duration::from_millis(100));
        let mut out = [0; 12];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"abcdefghijkl");
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
        // Indefinite blackhole never terminates on its own.
        assert_eq!(wrapped.summary().termination, None);
        assert_eq!(wrapped.write(b"gh").await.unwrap(), 2);
    }

    #[tokio::test]
    async fn finite_blackhole_terminates_at_deadline_without_further_writes() {
        tokio::time::pause();
        let plan = crate::FaultPlan::new(vec![fault(
            "hole",
            crate::FaultKind::Blackhole(crate::BlackholeConfig {
                close_after: Some(Duration::from_millis(100)),
            }),
        )])
        .unwrap();
        let (_peer, right) = tokio::io::duplex(32);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        assert_eq!(wrapped.write(b"hello").await.unwrap(), 5);
        assert_eq!(wrapped.summary().bytes_discarded, 5);
        assert_eq!(wrapped.summary().termination, None);
        // No further write arrives; a driver polling the termination future
        // arms the deadline timer and the deadline still fires termination.
        let handle = wrapped.termination_handle();
        let driver = tokio::spawn(async move {
            poll_fn(|cx| wrapped.poll_termination(cx)).await;
        });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(100)).await;
        tokio::time::timeout(Duration::from_secs(5), driver)
            .await
            .unwrap()
            .unwrap();
        let info = handle.terminated().await;
        assert_eq!(info.request, crate::TerminationRequest::Graceful);
        assert_eq!(info.fault_id.as_deref(), Some("hole"));
    }

    #[tokio::test]
    async fn disconnect_signals_are_durable_and_typed() {
        // Graceful, zero delay.
        let plan = crate::FaultPlan::new(vec![fault(
            "bye",
            crate::FaultKind::Disconnect(crate::DisconnectConfig {
                after: Duration::ZERO,
                hard_reset: false,
            }),
        )])
        .unwrap();
        let (mut peer, right) = tokio::io::duplex(32);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        wrapped.write_all(b"hi").await.unwrap();
        assert_eq!(
            wrapped.summary().termination,
            Some(crate::TerminationRequest::Graceful)
        );
        // Accepted bytes still drain before termination resolves.
        wrapped.flush().await.unwrap();
        let mut out = [0; 2];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"hi");
        // A late waiter still observes the request (level-triggered).
        let info = wrapped.termination_handle().terminated().await;
        assert_eq!(info.request, crate::TerminationRequest::Graceful);

        // Hard reset request.
        let plan = crate::FaultPlan::new(vec![fault(
            "rst",
            crate::FaultKind::Disconnect(crate::DisconnectConfig {
                after: Duration::ZERO,
                hard_reset: true,
            }),
        )])
        .unwrap();
        let (_peer, right) = tokio::io::duplex(32);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        wrapped.write_all(b"x").await.unwrap();
        assert_eq!(
            wrapped.summary().termination,
            Some(crate::TerminationRequest::HardReset)
        );
    }

    #[tokio::test]
    async fn delayed_disconnect_fires_without_further_writes() {
        tokio::time::pause();
        let plan = crate::FaultPlan::new(vec![fault(
            "later",
            crate::FaultKind::Disconnect(crate::DisconnectConfig {
                after: Duration::from_millis(75),
                hard_reset: true,
            }),
        )])
        .unwrap();
        let (_peer, right) = tokio::io::duplex(32);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        assert_eq!(wrapped.summary().termination, None);
        let handle = wrapped.termination_handle();
        let driver = tokio::spawn(async move {
            poll_fn(|cx| wrapped.poll_termination(cx)).await;
        });
        // The deadline timer is armed by the first termination poll, then
        // fires past the deadline with no application writes.
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(75)).await;
        tokio::time::timeout(Duration::from_secs(5), driver)
            .await
            .unwrap()
            .unwrap();
        let info = handle.terminated().await;
        assert_eq!(info.request, crate::TerminationRequest::HardReset);
        assert_eq!(info.fault_id.as_deref(), Some("later"));
    }

    #[tokio::test(start_paused = true)]
    async fn read_pumps_releasable_queued_writes() {
        // The embedding relay never flushes mid-stream, so reads drive
        // releasable queued writes toward the inner transport.
        let plan = latency_plan(50, 32);
        let (mut peer, right) = tokio::io::duplex(64);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        wrapped.write_all(b"abc").await.unwrap();
        peer.write_all(b"zy").await.unwrap();
        let mut inbound = [0; 2];
        wrapped.read_exact(&mut inbound).await.unwrap();
        assert_eq!(&inbound, b"zy");
        // The queued write is still held before its deadline.
        assert_eq!(wrapped.summary().bytes_forwarded, 0);
        tokio::time::advance(Duration::from_millis(50)).await;
        peer.write_all(b"!").await.unwrap();
        // This read pumps the now-releasable queued write even though the
        // caller never flushed.
        let mut mark = [0; 1];
        wrapped.read_exact(&mut mark).await.unwrap();
        assert_eq!(&mark, b"!");
        assert_eq!(wrapped.summary().bytes_forwarded, 3);
        let mut out = [0; 3];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"abc");
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_delivers_pending_latency_queue() {
        let plan = latency_plan(100, 32);
        let (mut peer, right) = tokio::io::duplex(32);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        wrapped.write_all(b"abc").await.unwrap();
        let shutdown = tokio::spawn(async move {
            wrapped.shutdown().await.unwrap();
        });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(100)).await;
        shutdown.await.unwrap();
        let mut out = [0; 3];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"abc");
    }

    #[tokio::test]
    async fn live_policy_publishes_after_existing_queue_drains() {
        let initial = latency_plan(10, 32);
        let policy = crate::LivePolicy::new(initial, 0);
        let (mut peer, right) = tokio::io::duplex(32);
        let mut wrapped =
            ChaosStream::new_live(right, policy.clone(), "p", 1, Direction::Upstream).unwrap();
        wrapped.write_all(b"a").await.unwrap();
        policy.publish(crate::FaultPlan::empty(), 0).unwrap();
        wrapped.flush().await.unwrap();
        wrapped.write_all(b"b").await.unwrap();
        let mut out = [0; 2];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"ab");
        assert_eq!(policy.generation(), 2);
    }

    #[tokio::test]
    async fn live_transition_preserves_due_termination() {
        let limited = crate::FaultPlan::new(vec![fault(
            "limit",
            crate::FaultKind::LimitData(crate::LimitDataConfig {
                bytes: NonZeroU64::new(2).unwrap(),
            }),
        )])
        .unwrap();
        let policy = crate::LivePolicy::new(limited, 0);
        let (mut peer, right) = tokio::io::duplex(32);
        let mut wrapped =
            ChaosStream::new_live(right, policy.clone(), "p", 1, Direction::Upstream).unwrap();
        assert_eq!(wrapped.write(b"ab").await.unwrap(), 2);
        assert!(wrapped.pending_generation().is_none());
        // Publish an empty plan while termination from the old generation is
        // already due: the swap must not erase it.
        policy.publish(crate::FaultPlan::empty(), 0).unwrap();
        assert_eq!(wrapped.pending_generation(), Some(2));
        wrapped.flush().await.unwrap();
        let mut out = [0; 2];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"ab");
        assert_eq!(wrapped.observed_generation(), 2);
        assert_eq!(
            wrapped.termination_info().map(|info| info.request),
            Some(crate::TerminationRequest::Graceful)
        );
    }

    #[tokio::test]
    async fn preserving_combination_conserves_bytes_under_fragmentation() {
        let plan = crate::FaultPlan::new(vec![
            fault(
                "latency",
                crate::FaultKind::Latency(crate::LatencyConfig {
                    delay: Duration::from_millis(5),
                    jitter: Duration::from_millis(2),
                    max_buffer_bytes: NonZeroU64::new(256).unwrap(),
                }),
            ),
            fault(
                "throttle",
                crate::FaultKind::Bandwidth(crate::BandwidthConfig {
                    bytes_per_second: NonZeroU64::new(4096).unwrap(),
                    burst_bytes: NonZeroU64::new(128).unwrap(),
                }),
            ),
            fault(
                "slice",
                crate::FaultKind::Slice(crate::SliceConfig {
                    average_size: NonZeroU64::new(7).unwrap(),
                    variation: 2,
                    delay: Duration::ZERO,
                }),
            ),
        ])
        .unwrap();
        let (mut peer, right) = tokio::io::duplex(512);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        let payload: Vec<u8> = (0..200).map(|i| (i % 251) as u8).collect();
        // Arbitrary fragmentation: 1..=9 byte writes.
        let mut offset = 0;
        let mut size = 1;
        while offset < payload.len() {
            let end = (offset + size).min(payload.len());
            wrapped.write_all(&payload[offset..end]).await.unwrap();
            offset = end;
            size = size % 9 + 1;
        }
        wrapped.flush().await.unwrap();
        let summary = wrapped.summary();
        assert_eq!(summary.bytes_accepted, 200);
        assert_eq!(summary.bytes_forwarded, 200);
        assert_eq!(summary.bytes_discarded, 0);
        let mut out = vec![0; 200];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(out, payload);
    }

    #[tokio::test]
    async fn bidirectional_downstream_policy_updates_from_empty() {
        let upstream = crate::LivePolicy::new(crate::FaultPlan::empty(), 0);
        let downstream = crate::LivePolicy::new(crate::FaultPlan::empty(), 0);
        let (mut peer, right) = tokio::io::duplex(32);
        let wrapped =
            BidirectionalChaosStream::new_live(right, upstream, downstream.clone(), "p", 1)
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
                0,
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

    #[test]
    fn evidence_serializes_without_payloads() {
        let summary = DirectionSummary {
            bytes_accepted: 3,
            bytes_forwarded: 3,
            bytes_discarded: 0,
            segments: 1,
            slices: 0,
            buffered_bytes: 0,
            high_water_bytes: 3,
            injected_delay_ms: 10,
            throttled_delay_ms: 0,
            activations: [0; 7],
            termination: Some(crate::TerminationRequest::Graceful),
            rng_version: crate::RngVersion::V1,
        };
        let value = serde_json::to_value(summary).unwrap();
        assert_eq!(value["bytes_forwarded"], 3);
        assert!(value.get("payload").is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn live_publish_from_empty_direct_path_engages_fault() {
        // A stream created without faults takes the direct write path, but
        // a later publication must still transition it without reconnecting.
        let policy = crate::LivePolicy::new(crate::FaultPlan::empty(), 0);
        let (mut peer, right) = tokio::io::duplex(32);
        let mut wrapped =
            ChaosStream::new_live(right, policy.clone(), "p", 1, Direction::Upstream).unwrap();
        wrapped.write_all(b"a").await.unwrap();
        let mut out = [0; 1];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"a");
        policy.publish(latency_plan(50, 32), 0).unwrap();
        wrapped.write_all(b"b").await.unwrap();
        // Still held before the deadline: the fault engaged on the live
        // connection (direct-path writes never buffer, but they do
        // count, so "a" already forwarded).
        assert_eq!(wrapped.summary().buffered_bytes, 1);
        assert_eq!(wrapped.summary().bytes_forwarded, 1);
        tokio::time::advance(Duration::from_millis(50)).await;
        wrapped.flush().await.unwrap();
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"b");
    }

    #[tokio::test]
    async fn due_bytes_forward_without_explicit_flush() {
        // The embedding relay never flushes mid-stream, so already-due
        // bytes must reach the inner transport on the write path.
        let plan = crate::FaultPlan::new(vec![fault(
            "close",
            crate::FaultKind::SlowClose(crate::SlowCloseConfig {
                delay: Duration::ZERO,
            }),
        )])
        .unwrap();
        let (mut peer, right) = tokio::io::duplex(32);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        wrapped.write_all(b"abc").await.unwrap();
        let mut out = [0; 3];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"abc");
    }

    #[tokio::test]
    async fn slow_close_shutdown_counts_activation() {
        let plan = crate::FaultPlan::new(vec![fault(
            "close",
            crate::FaultKind::SlowClose(crate::SlowCloseConfig {
                delay: Duration::from_millis(50),
            }),
        )])
        .unwrap();
        let (_peer, right) = tokio::io::duplex(32);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        wrapped.shutdown().await.unwrap();
        assert_eq!(wrapped.summary().activations[4], 1);
    }

    #[tokio::test]
    async fn graceful_termination_reads_eof_after_drain() {
        // Once a graceful termination resolves and the accepted prefix has
        // drained, reads see EOF so an embedding relay ends instead of
        // idling past its grace period.
        let plan = crate::FaultPlan::new(vec![fault(
            "bye",
            crate::FaultKind::Disconnect(crate::DisconnectConfig {
                after: Duration::ZERO,
                hard_reset: false,
            }),
        )])
        .unwrap();
        let (mut peer, right) = tokio::io::duplex(32);
        let mut wrapped = ChaosStream::new(right, plan, 7, "p", 1, Direction::Upstream).unwrap();
        wrapped.write_all(b"abc").await.unwrap();
        let mut out = [0; 3];
        peer.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"abc");
        assert_eq!(
            wrapped.termination_info().map(|info| info.request),
            Some(crate::TerminationRequest::Graceful)
        );
        let mut mark = [0; 1];
        assert_eq!(wrapped.read(&mut mark).await.unwrap(), 0);
    }
}

#[cfg(test)]
mod bidirectional_read_tests {
    use super::*;
    use std::{num::NonZeroU64, time::Duration};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Drive one downstream fault through a bidirectional stream whose peer
    /// writes then closes, and read everything back.
    async fn drive_closed_peer(kind: crate::FaultKind) -> Vec<u8> {
        let plan = crate::FaultPlan::new(vec![crate::FaultSpec {
            id: crate::FaultId::new("f").unwrap(),
            probability: crate::Probability::new(1.0).unwrap(),
            kind,
        }])
        .unwrap();
        let (mut peer, right) = tokio::io::duplex(256 * 1024);
        let mut wrapped = BidirectionalChaosStream::new_live(
            right,
            crate::LivePolicy::new(crate::FaultPlan::empty(), 0),
            crate::LivePolicy::new(plan, 0),
            "p",
            1,
        )
        .unwrap();
        tokio::spawn(async move {
            peer.write_all(&vec![7u8; 4096]).await.unwrap();
            // Close after writing: the read path must deliver flushed bytes
            // before observing inner EOF, not strand them behind it.
        });
        let mut out = vec![0u8; 4096];
        tokio::time::timeout(Duration::from_secs(10), wrapped.read_exact(&mut out))
            .await
            .expect("read completes")
            .unwrap();
        out
    }

    /// M013 narrow fix: flushed output bytes must be served before the next
    /// inner read, so a peer that closes after writing cannot strand them
    /// behind a premature EOF.
    #[tokio::test]
    async fn downstream_reads_survive_peer_close_after_write() {
        let latency = drive_closed_peer(crate::FaultKind::Latency(crate::LatencyConfig {
            delay: Duration::ZERO,
            jitter: Duration::ZERO,
            max_buffer_bytes: NonZeroU64::new(64 * 1024).unwrap(),
        }))
        .await;
        assert!(latency.iter().all(|b| *b == 7));
        let shaped = drive_closed_peer(crate::FaultKind::Bandwidth(crate::BandwidthConfig {
            bytes_per_second: NonZeroU64::new(1_000_000).unwrap(),
            burst_bytes: NonZeroU64::new(64 * 1024).unwrap(),
        }))
        .await;
        assert!(shaped.iter().all(|b| *b == 7));
    }
}
