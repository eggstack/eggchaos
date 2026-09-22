# M009 — Core Fault Semantics Corrective

Status: ready  
Depends on: M002 historical implementation  
Parallel with: M010  
Successor gate: M011

## Objective

Correct the release-baseline fault engine so every public native fault has the semantics already promised by the roadmap and historical M002 closure.

This is a correctness milestone, not a feature expansion. The current type model is broadly usable, but several variants are only partially executed or have semantics that diverge materially from the documented contract.

## User-visible outcome

After M009, callers of `eggchaos-core` can rely on the following statements:

- latency adds bounded delay without inherently serializing every caller write until physical delivery;
- bandwidth is a true bounded rate limiter with an explicit burst policy;
- blackhole with no deadline discards indefinitely, while a finite timeout terminates after the deadline rather than silently recovering;
- `limit_data` forwards exactly the configured byte count and then terminates the stream instead of accepting/discarding the remainder invisibly;
- slicing uses deterministic size variation and the configured inter-slice delay;
- disconnect faults actually cause a transport termination request;
- hard-reset requests are surfaced to the embedding runtime rather than remaining dead state;
- live policy transitions do not lose already accepted preserving bytes.

## Baseline findings

The 2026-09-22 implementation audit found the following concrete gaps in the current engine:

1. `FaultKind::Disconnect` is represented in the model but has no effective execution path.
2. `TerminationRequest::HardReset` is defined but not consumed by the data-plane runtime.
3. `LimitData` can return the caller's entire input length even when only the prefix up to the configured limit is forwarded.
4. A finite `BlackholeConfig::close_after` stops discarding after the deadline instead of requesting termination at the deadline.
5. `SliceConfig::variation` is not used.
6. `SliceConfig::delay` is not used.
7. `BandwidthConfig::burst_bytes` is not used and the current release-time calculation is not a token-bucket-equivalent sustained-rate implementation.
8. The current `ChaosStream::poll_write` waits for queued bytes to drain before reporting a write accepted, which defeats the intended multi-segment latency queue and can turn latency into a throughput limiter.
9. Termination evidence is internal to the direction engine and is not exposed through a durable embedding mechanism.

These findings supersede the assumption that historical M002 closure alone is sufficient release evidence. Do not rewrite or delete the historical closure record; M009 is the corrective successor.

## Scope

Primary affected surfaces:

```text
crates/eggchaos-core/src/plan.rs
crates/eggchaos-core/src/engine.rs
crates/eggchaos-core/src/stream.rs
crates/eggchaos-core/src/policy.rs
crates/eggchaos-core/src/rng.rs
crates/eggchaos-core/Cargo.toml
benchmarks/
fuzz/
docs/
plans/reference/verification-matrix.md   # update only if semantics need clarification
```

M009 may make pre-1.0 public API corrections where the current type cannot express the promised semantics, but every such change must be documented.

## Non-goals

Do not implement:

- listener lifecycle;
- native HTTP/CLI CRUD;
- Toxiproxy route compatibility;
- OS-specific TCP reset application;
- UDP/datagram faults;
- packet-level loss/reorder;
- arbitrary byte corruption;
- new fault categories.

M009 produces correct generic stream behavior and a truthful termination signal. M010 consumes it at the TCP runtime edge.

## Behavioral invariants

### AsyncWrite acceptance

If `poll_write` returns `Ready(Ok(n))`, eggchaos owns exactly the first `n` bytes. Those bytes may reside in an internal bounded queue and do not need to have reached the inner transport yet.

Once ownership is reported:

- preserving faults must eventually forward the bytes exactly once unless the inner transport fails or the connection is explicitly terminated;
- destructive faults may discard only according to their documented contract and must count discarded bytes;
- a policy update cannot silently forget owned bytes.

The engine must not claim ownership of bytes it could not fit within its configured bound.

### Bounded queue

The aggregate direction-local owned-byte bound must be explicit and enforceable.

When full, `poll_write` returns `Pending` after registering a wakeup path. It must not report zero-length success for nonempty input and must not allocate beyond the configured limit.

### Fault order

Faults remain ordered. Optimization may combine state machines only if externally observable order is preserved.

## Required semantic corrections

### 1. Buffered write path

Refactor `ChaosStream::poll_write` so a preserving write can return success after bounded engine ownership, rather than waiting for release/physical delivery.

A suitable flow is:

1. drive any currently releasable queued bytes;
2. determine bounded acceptance capacity;
3. accept a nonzero prefix into the engine;
4. arrange timers/wakers for future release;
5. return `Ready(Ok(n))`;
6. use subsequent `poll_write`, `poll_flush`, or `poll_shutdown` calls to continue draining.

Do not spawn one task per segment.

`poll_flush` remains the barrier that guarantees all preserving accepted bytes have reached the inner writer.

### 2. Latency

For each accepted segment compute an independent release deadline:

`accept_time + base_delay + deterministic_jitter`

Preserve byte order unless another future fault explicitly allows reordering.

Multiple segments accepted close together should be able to share similar release deadlines and drain in a burst after the configured latency. Do not serially add the base latency once per write.

Jitter remains deterministic and symmetric around the base delay with total delay floored at zero.

### 3. Bandwidth token bucket

Replace the current per-chunk delay approximation with a specified monotonic token bucket or mathematically equivalent limiter.

Required behavior:

- sustained rate = `bytes_per_second`;
- capacity = `burst_bytes`;
- initial token state is documented and tested;
- refill uses monotonic elapsed time;
- fractional refill arithmetic does not drift catastrophically;
- a write may consume available tokens immediately up to burst capacity;
- excess bytes remain queued/backpressured and are released as tokens become available;
- changing rate in a future live generation must not create an unbounded burst.

Use integer/fixed-point arithmetic where practical to keep deterministic boundary behavior.

### 4. Blackhole / timeout

Semantics:

- `close_after = None`: accepted bytes are intentionally discarded indefinitely until policy transition/runtime cancellation;
- `close_after = Some(d)`: accepted bytes are discarded while the timer is active, then a graceful termination request becomes due at the deadline.

The timeout must fire even if no additional application write arrives after the deadline. Poll state must retain a timer capable of waking the stream/runtime.

Do not resume ordinary forwarding when the deadline expires.

### 5. Limit data

For remaining limit `R` and caller buffer length `L`:

- accept/forward at most `min(R, L)`;
- return only that accepted prefix length;
- when the remaining limit reaches zero, mark connection termination due after the accepted prefix is resolved;
- the caller's suffix is not reported as accepted and is never silently counted as discarded;
- subsequent write/flush behavior must lead the embedding relay to terminate deterministically.

Test exact boundaries 0-after-prior-progress, 1, N, N+1, and a caller buffer that straddles N.

The public config may continue forbidding an initial zero-byte limit.

### 6. Slicer

Use:

- `average_size`;
- `variation`;
- `delay`.

For each slice, choose a deterministic size in the documented symmetric range while maintaining a lower bound of one and respecting remaining input length.

Apply the configured delay between logical slices, not once per original caller write.

Golden-test the slice-size sequence for a fixed seed.

### 7. Disconnect

A disconnect fault must actively produce a termination request.

The current `DisconnectConfig` cannot represent Toxiproxy's delayed `reset_peer` behavior. Because eggchaos is pre-1.0, add a duration field if necessary, for example:

```rust
pub struct DisconnectConfig {
    pub after: Duration,
    pub hard_reset: bool,
}
```

or an equivalent typed representation.

Native zero delay means terminate at the first defined contract boundary. A positive delay uses monotonic timer state.

Update TOML/native serialization later through M010; M009 should stabilize the core type first.

### 8. Termination signal

Provide a durable embedding contract for termination. Acceptable designs include:

- a shared `TerminationHandle`/state exposed by `ChaosStream`;
- a typed engine outcome observable by the runtime wrapper;
- a lightweight cancellation/termination signal composed with the relay.

Requirements:

- distinguish graceful from hard-reset request;
- indicate which direction/fault requested it when evidence is available;
- remain observable even if a notification would otherwise happen before the runtime begins waiting;
- do not require downcasting generic streams;
- do not apply TCP-specific behavior inside `eggchaos-core`.

M010 maps this signal to connection shutdown/reset.

## Live policy interaction

Current generation changes drain preserving queued bytes before replacing the engine. Preserve this property.

Strengthen it so a generation transition cannot:

- drop buffered latency/slice/bandwidth bytes;
- erase a due termination request;
- reset byte-limit state ambiguously;
- reuse unrelated RNG draws.

Document whether a destructive old-generation blackhole discards already accepted bytes before transition. The result must be deterministic and evidenced.

## Evidence/counters

Expand `DirectionSummary` / engine evidence as needed to expose:

- bytes accepted;
- bytes forwarded;
- bytes deliberately discarded;
- buffered bytes/current high-water if useful;
- segment/slice count;
- termination requested and mode;
- injected delay/throttle accounting;
- RNG version.

Do not expose payloads.

## Ordered work packages

1. **WP1 — Contract tests first:** add failing tests for every audit finding, especially limit-data write counts, finite-blackhole termination, disconnect signal, slice variation/delay, burst behavior, and multi-write latency queueing.
2. **WP2 — Buffered write ownership:** refactor the AsyncWrite path so bounded internal acceptance can complete before physical drain while preserving flush/shutdown correctness.
3. **WP3 — Timing/rate state machines:** repair latency and implement token-bucket bandwidth plus deterministic slicer variation/inter-slice delay.
4. **WP4 — Destructive/termination state machines:** repair blackhole timeout, exact limit-data boundary, delayed disconnect, and durable termination signaling.
5. **WP5 — Live-transition integration:** prove old-generation preserving bytes and pending termination state survive/resolve correctly across policy publication.
6. **WP6 — Property/fuzz coverage:** extend arbitrary fragmentation/partial-write and ordered-combination tests; fuzz transition sequences and duration/rate arithmetic.
7. **WP7 — Benchmark/doc reconciliation:** compare bare/empty/latency/bandwidth/slice paths and update fault semantics docs without claiming runtime reset application yet.
8. **WP8 — Closure:** run exact-commit verification and write `plans/closure/M009-core-fault-semantics-corrective-closure.md`.

## Required tests

At minimum:

- `poll_write` can accept several latency-buffered writes up to the bound before their deadlines;
- latency delays do not multiply simply because writes are fragmented;
- latency queue backpressures exactly at the configured capacity and wakes later;
- token bucket initial burst and sustained rate under paused Tokio time;
- token bucket does not exceed burst after long idle;
- slicer deterministic variation vector;
- slicer inter-slice delay;
- finite blackhole wakes and requests termination at its deadline with no further write;
- indefinite blackhole remains discard-only until transition/cancellation;
- limit_data returns only the accepted prefix and then terminates;
- graceful disconnect signal;
- hard-reset request signal;
- delayed disconnect signal;
- shutdown while each timer/queue is pending;
- live policy transition with buffered data;
- live policy transition with pending termination;
- scheduler-noise RNG invariance;
- preserving-fault byte-conservation property tests.

## Verification commands

At minimum:

```sh
cargo fmt --all -- --check
cargo clippy -p eggchaos-core --all-targets --all-features -- -D warnings
cargo test -p eggchaos-core --all-features
cargo test --workspace --all-features
cargo check --manifest-path fuzz/Cargo.toml
cargo test --manifest-path benchmarks/Cargo.toml
```

Run the relevant fuzz target for a bounded nontrivial iteration count and record it.

## Acceptance criteria

M009 closes only when:

- all seven release-baseline native fault categories have actual tested execution semantics;
- `burst_bytes`, slice variation, and slice delay are not dead configuration;
- finite blackhole terminates rather than recovers;
- limit-data no longer reports unforwarded suffix bytes as accepted;
- disconnect produces a durable runtime-consumable signal;
- latency buffering can hold multiple accepted segments without unbounded memory;
- preserving faults pass byte-conservation properties under fragmentation;
- live transitions preserve accepted bytes and termination state;
- docs match implementation;
- exact-commit closure evidence exists.

## Stop/rejection conditions

Stop and revise before proceeding if:

- correct behavior requires unbounded buffering;
- AsyncWrite ownership cannot be stated precisely;
- the fix relies on detached per-segment tasks;
- token-bucket behavior depends on wall-clock/system time;
- disconnect/reset is implemented by pretending generic shutdown is a TCP RST;
- live mutation can still drop owned preserving bytes;
- a public API change is made without updating docs/config follow-on requirements.

## Follow-on activation

M009 may run in parallel with M010.

M011 remains blocked until both M009 and M010 are closed. M012 also remains blocked because its toxic behavior depends on corrected core semantics.
