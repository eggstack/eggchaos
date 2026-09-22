use std::{
    collections::VecDeque,
    future::Future,
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use tokio::{
    io::AsyncWrite,
    sync::Notify,
    time::{Instant, Sleep},
};

use crate::{
    derive_seed, DeterministicRng, Direction, FaultId, FaultKind, FaultPlan, RngVersion,
    ValidationError,
};

/// A termination request emitted by an engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TerminationRequest {
    /// Close with the transport's normal shutdown operation.
    Graceful,
    /// Ask the embedding transport to attempt a hard reset.
    HardReset,
}

/// Durable termination evidence: which direction and fault requested it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TerminationInfo {
    /// Requested termination mode.
    pub request: TerminationRequest,
    /// Direction whose engine requested termination.
    pub direction: Direction,
    /// Stable fault identity when a configured fault requested it.
    pub fault_id: Option<String>,
}

#[derive(Debug, Default)]
struct TermSlot {
    inner: Mutex<Option<TerminationInfo>>,
    notify: Notify,
}

/// Level-triggered termination signal shared between an engine and its
/// embedding runtime.
///
/// Unlike a bare `Notify`, publication is durable: a runtime that begins
/// waiting after the request was published still observes it. The first
/// published request wins so a live-policy transition can never erase a
/// termination that is already due.
#[derive(Debug, Clone)]
pub struct TerminationHandle {
    slot: Arc<TermSlot>,
}

impl Default for TerminationHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminationHandle {
    /// Create an unset handle.
    pub fn new() -> Self {
        Self {
            slot: Arc::new(TermSlot {
                inner: Mutex::new(None),
                notify: Notify::new(),
            }),
        }
    }

    /// Return the published request, if any.
    pub fn get(&self) -> Option<TerminationInfo> {
        self.slot.inner.lock().ok()?.clone()
    }

    /// Publish a request. Returns true when this call set the stored value;
    /// a previously stored request is preserved (first wins).
    pub fn publish(&self, info: TerminationInfo) -> bool {
        if let Ok(mut guard) = self.slot.inner.lock() {
            if guard.is_none() {
                *guard = Some(info);
                self.slot.notify.notify_waiters();
                return true;
            }
        }
        false
    }

    /// Resolve once a request is published; returns immediately if one
    /// already is.
    pub async fn terminated(&self) -> TerminationInfo {
        loop {
            let notified = self.slot.notify.notified();
            if let Some(info) = self.get() {
                return info;
            }
            notified.await;
        }
    }

    fn poll_terminated(&self, cx: &mut Context<'_>) -> Poll<TerminationInfo> {
        // Register the waker before checking so a racing publish wakes us.
        let notified = self.slot.notify.notified();
        tokio::pin!(notified);
        if let Some(info) = self.get() {
            return Poll::Ready(info);
        }
        match notified.poll(cx) {
            Poll::Ready(()) => Poll::Ready(self.get().unwrap_or(TerminationInfo {
                request: TerminationRequest::Graceful,
                direction: Direction::Upstream,
                fault_id: None,
            })),
            Poll::Pending => Poll::Pending,
        }
    }
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
    /// Number of slicer-produced slices accepted.
    pub slices: u64,
    /// Bytes currently owned in the bounded queue.
    pub buffered_bytes: u64,
    /// High-water mark of owned queued bytes.
    pub high_water_bytes: u64,
    /// Cumulative configured latency delay in milliseconds.
    pub injected_delay_ms: u64,
    /// Cumulative bandwidth-throttle delay in milliseconds.
    pub throttled_delay_ms: u64,
    /// The latest termination request, if any.
    pub termination: Option<TerminationRequest>,
    /// Deterministic RNG contract version.
    pub rng_version: RngVersion,
}

#[derive(Debug)]
struct Queued {
    bytes: Bytes,
    release: Instant,
}

/// A deadline-keyed async timer.
///
/// The previous implementation cached a single `Sleep` future and reused it
/// across queue heads via `get_or_insert_with`. That is unsound: when a
/// head's release has already passed, the sleep branch is skipped and a
/// stale (already fired) timer stays cached; the next head then reuses the
/// fired timer and is released early without waiting. Keying the cached
/// timer by its deadline makes reuse exact: a changed deadline always
/// re-arms, and a consumed timer is cleared.
#[derive(Debug, Default)]
struct ArmedTimer {
    sleep: Option<Pin<Box<Sleep>>>,
    deadline: Option<Instant>,
}

impl ArmedTimer {
    /// Resolve once `deadline` is reached. `deadline` must be greater than
    /// `now`; a zero remaining duration resolves immediately.
    fn poll(&mut self, cx: &mut Context<'_>, now: Instant, deadline: Instant) -> Poll<()> {
        if self.deadline != Some(deadline) {
            self.sleep = Some(Box::pin(tokio::time::sleep(
                deadline.saturating_duration_since(now),
            )));
            self.deadline = Some(deadline);
        }
        let sleep = self.sleep.as_mut().expect("timer is armed above");
        match sleep.as_mut().poll(cx) {
            Poll::Ready(()) => {
                self.sleep = None;
                self.deadline = None;
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Monotonic token bucket with integer fixed-point token accounting.
///
/// Tokens are stored in microtokens (1 token = 1 byte, scaled by 1e6) so
/// fractional refill never drifts. Sustained rate is `rate` bytes/second and
/// capacity is `burst` bytes. The bucket starts full; this initial state is
/// documented and covered by tests. A bucket refills only from monotonic
/// elapsed time and never exceeds capacity, so a long idle period grants at
/// most one burst.
///
/// All timestamps use the Tokio clock so throttling stays deterministic under
/// paused Tokio time as well as on wall-clock runtimes.
#[derive(Debug)]
struct TokenBucket {
    rate: u64,
    capacity_micro: u128,
    tokens_micro: u128,
    last: Instant,
    cursor: Instant,
}

impl TokenBucket {
    fn new(rate: u64, burst: u64, now: Instant) -> Self {
        Self {
            rate,
            capacity_micro: u128::from(burst) * 1_000_000,
            tokens_micro: u128::from(burst) * 1_000_000,
            last: now,
            cursor: now,
        }
    }

    fn refill(&mut self, now: Instant) {
        let elapsed_nanos = now.saturating_duration_since(self.last).as_nanos();
        if elapsed_nanos > 0 {
            // Microtokens earned: elapsed_s * rate * 1e6
            // = elapsed_nanos * rate / 1000.
            let earned = elapsed_nanos.saturating_mul(u128::from(self.rate)) / 1_000;
            self.tokens_micro = (self.tokens_micro.saturating_add(earned)).min(self.capacity_micro);
            self.last = now;
        }
    }

    /// Schedule `len` bytes. Returns the earliest release time and the
    /// throttle delay in milliseconds imposed by this call.
    fn consume(&mut self, len: usize, now: Instant, earliest: Instant) -> (Instant, u64) {
        self.refill(now);
        let need = (len as u128).saturating_mul(1_000_000);
        let base = earliest.max(now);
        if self.tokens_micro >= need {
            self.tokens_micro -= need;
            self.cursor = self.cursor.max(base);
            (base, 0)
        } else {
            let deficit = need - self.tokens_micro;
            // wait_nanos = ceil(deficit_micro * 1000 / rate).
            let wait_nanos = deficit
                .saturating_mul(1_000)
                .saturating_add(u128::from(self.rate) - 1)
                / u128::from(self.rate);
            let wait_nanos = wait_nanos.min(u128::from(u64::MAX));
            let wait = Duration::from_nanos(wait_nanos as u64);
            let release = base.max(now.checked_add(wait).unwrap_or(now));
            self.tokens_micro = 0;
            self.cursor = self.cursor.max(release);
            // Throttle accounting: ceiling milliseconds imposed beyond the
            // latency-derived earliest time, so any imposed wait counts.
            let throttled_ms = release
                .saturating_duration_since(earliest.max(now))
                .as_nanos()
                .saturating_add(999_999)
                / 1_000_000;
            let throttled_ms = throttled_ms.min(u128::from(u64::MAX)) as u64;
            (release, throttled_ms)
        }
    }
}

#[derive(Debug, Clone)]
struct BlackholeState {
    deadline: Option<Instant>,
    fault_id: FaultId,
}

#[derive(Debug, Clone)]
struct DisconnectState {
    deadline: Instant,
    hard_reset: bool,
    fault_id: FaultId,
}

/// Poll-driven ordered fault state machine.
pub struct DirectionEngine {
    plan: FaultPlan,
    direction: Direction,
    queue: VecDeque<Queued>,
    buffered: usize,
    high_water: usize,
    max_buffer: usize,
    rngs: Vec<(bool, DeterministicRng)>,
    evidence: EngineEvidence,
    termination: Option<TerminationInfo>,
    term_handle: TerminationHandle,
    queue_timer: ArmedTimer,
    term_timer: ArmedTimer,
    shutdown_timer: ArmedTimer,
    shutdown_deadline: Option<Instant>,
    slow_close: Option<Duration>,
    remaining_limit: Option<(u64, FaultId)>,
    blackhole: Option<BlackholeState>,
    disconnect: Option<DisconnectState>,
    bandwidth: Option<TokenBucket>,
    slicer_active: bool,
    slice_cursor: Instant,
}

impl std::fmt::Debug for DirectionEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectionEngine")
            .field("direction", &self.direction)
            .field("buffered", &self.buffered)
            .field("max_buffer", &self.max_buffer)
            .field("evidence", &self.evidence)
            .field("termination", &self.termination)
            .finish()
    }
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
        Self::new_with_termination(
            plan,
            run_seed,
            proxy,
            connection_key,
            direction,
            TerminationHandle::new(),
        )
    }

    /// Compile a plan while sharing a durable termination handle, so a
    /// live-policy transition can never erase an already-due request.
    pub fn new_with_termination(
        plan: FaultPlan,
        run_seed: u64,
        proxy: &str,
        connection_key: u64,
        direction: Direction,
        term_handle: TerminationHandle,
    ) -> Result<Self, ValidationError> {
        let now = Instant::now();
        let mut max_buffer = 64 * 1024usize;
        let mut remaining_limit = None;
        let mut blackhole: Option<BlackholeState> = None;
        let mut disconnect: Option<DisconnectState> = None;
        let mut bandwidth_cfg = None;
        let mut slicer_active = false;
        let mut rngs = Vec::with_capacity(plan.faults().len());
        for fault in plan.faults() {
            let seed = derive_seed(run_seed, proxy, connection_key, direction, &fault.id);
            let mut rng = DeterministicRng::new(seed);
            let active = rng.bernoulli(fault.probability.get());
            if !active {
                rngs.push((false, rng));
                continue;
            }
            match fault.kind {
                FaultKind::Latency(c) => {
                    max_buffer = max_buffer.min(c.max_buffer_bytes.get() as usize);
                }
                FaultKind::LimitData(c) => {
                    if remaining_limit.is_none() {
                        remaining_limit = Some((c.bytes.get(), fault.id.clone()));
                    }
                }
                FaultKind::Blackhole(c) => {
                    let deadline = c.close_after.map(|d| now + d);
                    match &blackhole {
                        // An indefinite blackhole dominates finite ones.
                        Some(existing) if existing.deadline.is_none() => {}
                        _ if deadline.is_none() => {
                            blackhole = Some(BlackholeState {
                                deadline: None,
                                fault_id: fault.id.clone(),
                            });
                        }
                        Some(existing)
                            if existing
                                .deadline
                                .is_some_and(|d| deadline.is_some_and(|n| d <= n)) => {}
                        _ => {
                            blackhole = Some(BlackholeState {
                                deadline,
                                fault_id: fault.id.clone(),
                            });
                        }
                    }
                }
                FaultKind::SlowClose(_) => {}
                FaultKind::Bandwidth(c) => {
                    if bandwidth_cfg.is_none() {
                        bandwidth_cfg = Some((c.bytes_per_second.get(), c.burst_bytes.get()));
                    }
                }
                FaultKind::Slice(_) => {
                    slicer_active = true;
                }
                FaultKind::Disconnect(c) => {
                    let deadline = now + c.after;
                    let replace = match &disconnect {
                        None => true,
                        Some(existing) => {
                            deadline < existing.deadline
                                || (deadline == existing.deadline
                                    && c.hard_reset
                                    && !existing.hard_reset)
                        }
                    };
                    if replace {
                        disconnect = Some(DisconnectState {
                            deadline,
                            hard_reset: c.hard_reset,
                            fault_id: fault.id.clone(),
                        });
                    }
                }
            }
            rngs.push((true, rng));
        }
        // Slow-close delay snapshot (first active wins; plans rarely combine).
        let slow_close = plan
            .faults()
            .iter()
            .zip(&rngs)
            .find_map(|(fault, (active, _))| {
                if *active {
                    if let FaultKind::SlowClose(c) = fault.kind {
                        return Some(c.delay);
                    }
                }
                None
            });
        let termination = term_handle.get();
        let evidence = EngineEvidence {
            rng_version: RngVersion::V1,
            termination: termination.as_ref().map(|info| info.request),
            ..Default::default()
        };
        Ok(Self {
            plan,
            direction,
            queue: VecDeque::new(),
            buffered: 0,
            high_water: 0,
            max_buffer: max_buffer.max(1),
            rngs,
            evidence,
            termination,
            term_handle,
            queue_timer: ArmedTimer::default(),
            term_timer: ArmedTimer::default(),
            shutdown_timer: ArmedTimer::default(),
            shutdown_deadline: None,
            slow_close,
            remaining_limit,
            blackhole,
            disconnect,
            bandwidth: bandwidth_cfg.map(|(rate, burst)| TokenBucket::new(rate, burst, now)),
            slicer_active,
            slice_cursor: now,
        })
    }

    /// Construct the cheap empty path.
    pub fn empty() -> Self {
        let evidence = EngineEvidence {
            rng_version: RngVersion::V1,
            ..Default::default()
        };
        Self {
            plan: FaultPlan::empty(),
            direction: Direction::Upstream,
            queue: VecDeque::new(),
            buffered: 0,
            high_water: 0,
            max_buffer: 1,
            rngs: Vec::new(),
            evidence,
            termination: None,
            term_handle: TerminationHandle::new(),
            queue_timer: ArmedTimer::default(),
            term_timer: ArmedTimer::default(),
            shutdown_timer: ArmedTimer::default(),
            shutdown_deadline: None,
            slow_close: None,
            remaining_limit: None,
            blackhole: None,
            disconnect: None,
            bandwidth: None,
            slicer_active: false,
            slice_cursor: Instant::now(),
        }
    }

    /// True when no stage is configured.
    pub fn is_empty(&self) -> bool {
        self.plan.is_empty()
    }

    /// Borrow the compiled plan.
    pub fn plan(&self) -> &FaultPlan {
        &self.plan
    }

    /// Return the configured direction.
    pub const fn direction(&self) -> Direction {
        self.direction
    }

    /// Return a copy of the current evidence.
    pub fn evidence(&self) -> EngineEvidence {
        let mut evidence = self.evidence;
        evidence.buffered_bytes = self.buffered as u64;
        evidence.high_water_bytes = self.high_water as u64;
        evidence
    }

    /// Return the current termination request.
    pub fn termination_request(&self) -> Option<TerminationRequest> {
        self.termination.as_ref().map(|info| info.request)
    }

    /// Return full termination evidence, if any.
    pub fn termination_info(&self) -> Option<TerminationInfo> {
        self.termination.clone()
    }

    /// Durable termination handle shared with the embedding runtime.
    pub fn termination_handle(&self) -> TerminationHandle {
        self.term_handle.clone()
    }

    /// Bytes currently owned in the bounded queue.
    pub fn buffered_bytes(&self) -> usize {
        self.buffered
    }

    /// Total bounded ownership capacity for this direction.
    pub fn capacity_bytes(&self) -> usize {
        self.max_buffer
    }

    /// True when the bounded queue holds no owned bytes.
    pub fn queue_is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    fn publish_termination(&mut self, info: TerminationInfo) {
        self.term_handle.publish(info.clone());
        // First published request wins end to end.
        if self.termination.is_none() {
            self.termination = Some(info.clone());
        }
        self.evidence.termination = self.termination.as_ref().map(|i| i.request);
    }

    fn refresh_sync_terminations(&mut self) {
        if self.termination.is_some() {
            return;
        }
        let now = Instant::now();
        // A due disconnect publishes even without further writes; zero-delay
        // disconnects are therefore due at the first contract boundary.
        if let Some(state) = &self.disconnect {
            if now >= state.deadline {
                let info = TerminationInfo {
                    request: if state.hard_reset {
                        TerminationRequest::HardReset
                    } else {
                        TerminationRequest::Graceful
                    },
                    direction: self.direction,
                    fault_id: Some(state.fault_id.to_string()),
                };
                self.publish_termination(info);
                return;
            }
        }
        // A finite blackhole stops discarding at its deadline and instead
        // requests graceful termination. Indefinite blackholes never
        // terminate on their own.
        if let Some(state) = &self.blackhole {
            if let Some(deadline) = state.deadline {
                if now >= deadline {
                    let info = TerminationInfo {
                        request: TerminationRequest::Graceful,
                        direction: self.direction,
                        fault_id: Some(state.fault_id.to_string()),
                    };
                    self.publish_termination(info);
                }
            }
        }
    }

    fn blackhole_discarding(&self) -> bool {
        let Some(state) = &self.blackhole else {
            return false;
        };
        match state.deadline {
            None => true,
            Some(deadline) => Instant::now() < deadline,
        }
    }

    fn next_slice_size(&mut self, remaining: usize) -> usize {
        // The plan order is stable, so drawing from the first active
        // slicer's fault-local RNG yields a deterministic size sequence
        // for a fixed seed. Size is symmetric in
        // [average - variation, average + variation], lower-bounded at one.
        let mut size = remaining.max(1) as u64;
        for (fault, (active, rng)) in self.plan.faults().iter().zip(&mut self.rngs) {
            if !*active {
                continue;
            }
            if let FaultKind::Slice(c) = fault.kind {
                let span = c.variation.saturating_mul(2).saturating_add(1);
                let offset = rng.below(span);
                size = c
                    .average_size
                    .get()
                    .saturating_sub(c.variation)
                    .saturating_add(offset)
                    .max(1);
                break;
            }
        }
        size.min(remaining as u64).max(1) as usize
    }

    /// Accept a caller-visible prefix into the bounded pipeline.
    ///
    /// Returns the number of bytes now owned by the engine. Ownership means:
    /// preserving faults will eventually forward the bytes exactly once
    /// (unless the inner transport fails or a durable termination resolves
    /// them), while destructive faults count them as discarded per their
    /// contract. A return of zero means either the bounded queue is full
    /// (retry after the release timer wakes the task) or a termination is
    /// already due.
    pub fn accept(&mut self, input: &[u8]) -> usize {
        if input.is_empty() {
            return 0;
        }
        self.refresh_sync_terminations();
        let now = Instant::now();
        let mut limit_available = match &self.remaining_limit {
            Some((remaining, _)) => *remaining,
            None => u64::MAX,
        };
        if limit_available == 0 {
            if self.termination.is_none() {
                if let Some((_, id)) = &self.remaining_limit {
                    let id = id.clone();
                    self.publish_termination(TerminationInfo {
                        request: TerminationRequest::Graceful,
                        direction: self.direction,
                        fault_id: Some(id.to_string()),
                    });
                }
            }
            return 0;
        }
        let capacity = self.max_buffer.saturating_sub(self.buffered);
        let mut accepted_cap = (input.len() as u64)
            .min(limit_available)
            .min(capacity as u64) as usize;

        // Destructive blackhole path: discard without buffering. The bound
        // does not apply because no bytes are retained.
        if self.blackhole_discarding() {
            accepted_cap = (input.len() as u64).min(limit_available) as usize;
            if accepted_cap == 0 {
                return 0;
            }
            self.evidence.bytes_accepted += accepted_cap as u64;
            self.evidence.bytes_discarded += accepted_cap as u64;
            self.evidence.segments += 1;
            if self.remaining_limit.is_some() {
                limit_available -= accepted_cap as u64;
                if let Some((remaining, _)) = &mut self.remaining_limit {
                    *remaining = limit_available;
                }
                if limit_available == 0 {
                    if let Some((_, id)) = &self.remaining_limit {
                        let id = id.clone();
                        self.publish_termination(TerminationInfo {
                            request: TerminationRequest::Graceful,
                            direction: self.direction,
                            fault_id: Some(id.to_string()),
                        });
                    }
                }
            }
            // Report only the accepted prefix; a limit-induced suffix is
            // never claimed as owned.
            return accepted_cap;
        }

        if accepted_cap == 0 {
            return 0;
        }
        let mut offset = 0usize;
        while offset < accepted_cap {
            let remaining = accepted_cap - offset;
            let chunk = if self.slicer_active {
                self.next_slice_size(remaining)
            } else {
                remaining
            }
            .max(1)
            .min(remaining);
            if self.buffered + chunk > self.max_buffer {
                break;
            }
            let mut release = now;
            let mut slice_delay = Duration::ZERO;
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
                        // Independent per-segment deadline: segments accepted
                        // together share similar deadlines and drain as a
                        // burst instead of serializing one delay per write.
                        release = release.max(now + Duration::from_millis(millis));
                        self.evidence.injected_delay_ms =
                            self.evidence.injected_delay_ms.saturating_add(millis);
                    }
                    FaultKind::Slice(c) => {
                        slice_delay = slice_delay.max(c.delay);
                    }
                    _ => {}
                }
            }
            if let Some(bucket) = &mut self.bandwidth {
                let (bw_release, throttled_ms) = bucket.consume(chunk, now, release);
                release = bw_release;
                self.evidence.throttled_delay_ms = self
                    .evidence
                    .throttled_delay_ms
                    .saturating_add(throttled_ms);
            }
            if self.slicer_active && !slice_delay.is_zero() {
                // Inter-slice delay staggers logical slices rather than
                // applying once per original caller write.
                release = release.max(self.slice_cursor);
                self.slice_cursor = release + slice_delay;
            }
            self.queue.push_back(Queued {
                bytes: Bytes::copy_from_slice(&input[offset..offset + chunk]),
                release,
            });
            self.buffered += chunk;
            self.high_water = self.high_water.max(self.buffered);
            self.evidence.segments += 1;
            if self.slicer_active {
                self.evidence.slices += 1;
            }
            offset += chunk;
        }
        if offset == 0 {
            return 0;
        }
        self.evidence.bytes_accepted += offset as u64;
        if self.remaining_limit.is_some() {
            if let Some((remaining, _)) = &mut self.remaining_limit {
                *remaining = remaining.saturating_sub(offset as u64);
                if *remaining == 0 {
                    let id = self
                        .remaining_limit
                        .as_ref()
                        .map(|(_, id)| id.clone())
                        .expect("limit checked");
                    self.publish_termination(TerminationInfo {
                        request: TerminationRequest::Graceful,
                        direction: self.direction,
                        fault_id: Some(id.to_string()),
                    });
                }
            }
        }
        // Report only the owned prefix. The caller retains any suffix,
        // whether it was excluded by the byte limit or by bounded capacity.
        offset
    }

    /// Observe termination, arming deadline timers so a finite blackhole or
    /// delayed disconnect fires even when no further application write
    /// arrives. Never resolves to `Ready(None)`: with no deadline pending it
    /// waits on the durable handle so a later synchronous publication wakes
    /// the waiter.
    pub fn poll_due_termination(&mut self, cx: &mut Context<'_>) -> Poll<TerminationInfo> {
        self.refresh_sync_terminations();
        if let Some(info) = self.termination.clone() {
            return Poll::Ready(info);
        }
        // Earliest pending deadline across disconnect and finite blackhole.
        let mut deadline: Option<Instant> = None;
        if let Some(state) = &self.disconnect {
            deadline = Some(match deadline {
                Some(current) => current.min(state.deadline),
                None => state.deadline,
            });
        }
        if let Some(state) = &self.blackhole {
            if let Some(end) = state.deadline {
                deadline = Some(match deadline {
                    Some(current) => current.min(end),
                    None => end,
                });
            }
        }
        if let Some(end) = deadline {
            let now = Instant::now();
            if now < end {
                if self.term_timer.poll(cx, now, end).is_pending() {
                    return Poll::Pending;
                }
                self.refresh_sync_terminations();
                if let Some(info) = self.termination.clone() {
                    return Poll::Ready(info);
                }
            } else {
                self.refresh_sync_terminations();
                if let Some(info) = self.termination.clone() {
                    return Poll::Ready(info);
                }
            }
        }
        // No deadline pending: park on the durable handle so a future
        // synchronous publication (for example limit exhaustion from another
        // write path sharing this engine's handle) wakes this task.
        self.term_handle.poll_terminated(cx)
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
                // Deadline-keyed wait: a stale timer for a previous head can
                // never release this head early.
                if self.queue_timer.poll(cx, now, front.release).is_pending() {
                    return Poll::Pending;
                }
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
            let now = Instant::now();
            if now < deadline && self.shutdown_timer.poll(cx, now, deadline).is_pending() {
                return Poll::Pending;
            }
        }
        Pin::new(inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlackholeConfig, LimitDataConfig};
    use proptest::prelude::*;
    use std::num::NonZeroU64;

    fn fault(id: &str, kind: FaultKind) -> crate::FaultSpec {
        crate::FaultSpec {
            id: crate::FaultId::new(id).unwrap(),
            probability: crate::Probability::new(1.0).unwrap(),
            kind,
        }
    }

    fn plan(faults: Vec<crate::FaultSpec>) -> FaultPlan {
        FaultPlan::new(faults).unwrap()
    }

    #[test]
    fn token_bucket_grants_initial_burst_then_limits() {
        let start = Instant::now();
        let mut bucket = TokenBucket::new(100, 200, start);
        // Full burst is immediately available.
        let (release, throttled) = bucket.consume(200, start, start);
        assert_eq!(release, start);
        assert_eq!(throttled, 0);
        // The next byte needs 10ms of refill at 100 B/s.
        let (release, throttled) = bucket.consume(1, start, start);
        assert_eq!(release, start + Duration::from_millis(10));
        assert_eq!(throttled, 10);
    }

    #[test]
    fn token_bucket_capped_after_long_idle() {
        let start = Instant::now();
        let mut bucket = TokenBucket::new(10, 50, start);
        let idle = start + Duration::from_secs(3600);
        bucket.refill(idle);
        // Even after an hour idle, at most one burst is available.
        let (release, _) = bucket.consume(50, idle, idle);
        assert_eq!(release, idle);
        let (release, _) = bucket.consume(1, idle, idle);
        assert_eq!(release, idle + Duration::from_millis(100));
    }

    #[test]
    fn token_bucket_sustained_rate_is_cumulative() {
        let start = Instant::now();
        let mut bucket = TokenBucket::new(100, 100, start);
        let (first, _) = bucket.consume(100, start, start);
        assert_eq!(first, start);
        // A second burst-sized chunk scheduled at the same instant waits the
        // full deficit instead of reusing the burst.
        let (second, throttled) = bucket.consume(100, start, start);
        assert_eq!(second, start + Duration::from_secs(1));
        assert_eq!(throttled, 1000);
    }

    #[test]
    fn zero_delay_disconnect_is_due_at_first_boundary() {
        let engine = DirectionEngine::new(
            plan(vec![fault(
                "bye",
                FaultKind::Disconnect(crate::DisconnectConfig {
                    after: Duration::ZERO,
                    hard_reset: false,
                }),
            )]),
            7,
            "p",
            1,
            Direction::Upstream,
        )
        .unwrap();
        assert!(engine.termination_request().is_none());
        let mut engine = engine;
        // The first contract boundary (accept) publishes the request.
        assert_eq!(engine.accept(b"hello"), 5);
        assert_eq!(
            engine.termination_request(),
            Some(TerminationRequest::Graceful)
        );
        let info = engine.termination_info().unwrap();
        assert_eq!(info.fault_id.as_deref(), Some("bye"));
        assert_eq!(info.direction, Direction::Upstream);
    }

    #[test]
    fn finite_blackhole_reports_prefix_only_and_terminates() {
        let mut engine = DirectionEngine::new(
            plan(vec![
                fault(
                    "hole",
                    FaultKind::Blackhole(BlackholeConfig {
                        close_after: Some(Duration::from_millis(10)),
                    }),
                ),
                fault(
                    "limit",
                    FaultKind::LimitData(LimitDataConfig {
                        bytes: NonZeroU64::new(4).unwrap(),
                    }),
                ),
            ]),
            7,
            "p",
            1,
            Direction::Upstream,
        )
        .unwrap();
        // Limit caps the discard path: only the accepted prefix is reported.
        assert_eq!(engine.accept(b"abcdef"), 4);
        assert_eq!(engine.evidence().bytes_discarded, 4);
        assert_eq!(
            engine.termination_request(),
            Some(TerminationRequest::Graceful)
        );
    }

    #[test]
    fn identical_inputs_give_identical_deterministic_state() {
        let build = || {
            DirectionEngine::new(
                plan(vec![
                    fault(
                        "latency",
                        FaultKind::Latency(crate::LatencyConfig {
                            delay: Duration::from_millis(25),
                            jitter: Duration::from_millis(10),
                            max_buffer_bytes: NonZeroU64::new(1024).unwrap(),
                        }),
                    ),
                    fault(
                        "slice",
                        FaultKind::Slice(crate::SliceConfig {
                            average_size: NonZeroU64::new(8).unwrap(),
                            variation: 3,
                            delay: Duration::ZERO,
                        }),
                    ),
                ]),
                99,
                "proxy",
                41,
                Direction::Downstream,
            )
            .unwrap()
        };
        let mut first = build();
        let mut second = build();
        assert_eq!(
            first.accept(b"0123456789abcdef"),
            second.accept(b"0123456789abcdef")
        );
        assert_eq!(first.evidence(), second.evidence());
    }

    #[test]
    fn probability_zero_never_activates_and_one_always_does() {
        let inactive = DirectionEngine::new(
            plan(vec![crate::FaultSpec {
                id: crate::FaultId::new("maybe").unwrap(),
                probability: crate::Probability::new(0.0).unwrap(),
                kind: FaultKind::Latency(crate::LatencyConfig {
                    delay: Duration::from_millis(1_000),
                    jitter: Duration::ZERO,
                    max_buffer_bytes: NonZeroU64::new(1024).unwrap(),
                }),
            }]),
            7,
            "p",
            1,
            Direction::Upstream,
        )
        .unwrap();
        // Probability 0 keeps the default 64 KiB bound instead of the
        // fault's 1 KiB... here max_buffer only shrinks for active faults.
        assert_eq!(inactive.capacity_bytes(), 64 * 1024);
        let active = DirectionEngine::new(
            plan(vec![fault(
                "sure",
                FaultKind::Latency(crate::LatencyConfig {
                    delay: Duration::from_millis(1_000),
                    jitter: Duration::ZERO,
                    max_buffer_bytes: NonZeroU64::new(7).unwrap(),
                }),
            )]),
            7,
            "p",
            1,
            Direction::Upstream,
        )
        .unwrap();
        assert_eq!(active.capacity_bytes(), 7);
    }

    fn preserving_plan() -> FaultPlan {
        plan(vec![
            fault(
                "latency",
                FaultKind::Latency(crate::LatencyConfig {
                    delay: Duration::from_millis(10),
                    jitter: Duration::from_millis(3),
                    max_buffer_bytes: NonZeroU64::new(512).unwrap(),
                }),
            ),
            fault(
                "throttle",
                FaultKind::Bandwidth(crate::BandwidthConfig {
                    bytes_per_second: NonZeroU64::new(4096).unwrap(),
                    burst_bytes: NonZeroU64::new(512).unwrap(),
                }),
            ),
            fault(
                "slice",
                FaultKind::Slice(crate::SliceConfig {
                    average_size: NonZeroU64::new(8).unwrap(),
                    variation: 3,
                    delay: Duration::from_millis(1),
                }),
            ),
        ])
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn preserving_accept_conserves_bytes(
            fragments in prop::collection::vec(
                prop::collection::vec(any::<u8>(), 1..32usize), 1..8usize
            )
        ) {
            let mut engine =
                DirectionEngine::new(preserving_plan(), 7, "p", 1, Direction::Upstream).unwrap();
            let mut total = 0usize;
            for fragment in &fragments {
                // The 512-byte bound always fits these fragments.
                let accepted = engine.accept(fragment);
                prop_assert!(accepted <= fragment.len());
                total += accepted;
            }
            prop_assert_eq!(total, fragments.iter().map(Vec::len).sum::<usize>());
            let evidence = engine.evidence();
            prop_assert_eq!(evidence.bytes_accepted, total as u64);
            prop_assert_eq!(evidence.bytes_forwarded, 0);
            prop_assert_eq!(evidence.bytes_discarded, 0);
            prop_assert_eq!(engine.buffered_bytes(), total);
        }

        #[test]
        fn limit_data_accepts_exact_prefix(
            fragments in prop::collection::vec(
                prop::collection::vec(any::<u8>(), 1..24usize), 1..6usize
            )
        ) {
            let mut engine = DirectionEngine::new(
                plan(vec![fault(
                    "limit",
                    FaultKind::LimitData(LimitDataConfig {
                        bytes: NonZeroU64::new(40).unwrap(),
                    }),
                )]),
                7,
                "p",
                1,
                Direction::Upstream,
            )
            .unwrap();
            let offered: usize = fragments.iter().map(Vec::len).sum();
            let mut total = 0usize;
            for fragment in &fragments {
                total += engine.accept(fragment);
            }
            prop_assert_eq!(total, offered.min(40));
            prop_assert_eq!(engine.evidence().bytes_forwarded, 0);
            if offered >= 40 {
                prop_assert_eq!(
                    engine.termination_request(),
                    Some(TerminationRequest::Graceful)
                );
            }
        }
    }
}
