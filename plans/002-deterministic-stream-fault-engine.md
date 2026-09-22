# M002 — Deterministic Stream Fault Engine

Status: active  
Depends on: M001  
Successor: M003

## Objective

Implement the protocol-neutral directional fault engine in `eggchaos-core` with bounded memory, deterministic randomness, correct Tokio `AsyncWrite` behavior, and enough evidence that the engine can safely be embedded in a standalone relay or caller-owned transport.

This milestone is the technical core of eggchaos.

## User-visible outcome

Rust callers can wrap a Tokio-compatible stream and apply an ordered, validated `FaultPlan` to one write direction without running a proxy daemon.

The release-baseline native faults are functional:

- latency with jitter;
- bandwidth throttling;
- blackhole/timeout;
- byte limit;
- slow close;
- slicing;
- graceful disconnect;
- an abstract reset request/capability result where the generic stream cannot itself guarantee TCP RST;
- deterministic per-connection probability selection.

## Preconditions

M001 must be closed with:

- stable crate topology;
- canonical typed fault definitions;
- empty-plan `ChaosStream`;
- documented dependency graph;
- green baseline CI.

If M001 changed ADR 001 or ADR 002, reconcile this plan before implementation.

## Scope

All substantive work should remain in `eggchaos-core` except test fixtures/benchmarks.

Expected module shape, illustrative rather than mandatory:

```text
eggchaos-core/src/
  lib.rs
  plan.rs
  validate.rs
  stream.rs
  engine/
    mod.rs
    latency.rs
    bandwidth.rs
    blackhole.rs
    limit_data.rs
    slow_close.rs
    slice.rs
    disconnect.rs
  rng.rs
  evidence.rs
  capability.rs
```

Avoid one spawned Tokio task per fault or per write. Prefer a poll-driven state machine with timers/futures stored in the engine.

## Non-goals

Do not implement:

- TCP listener/runtime;
- admin API;
- Toxiproxy JSON routes;
- Prometheus;
- direct Eggfetch adapter;
- concrete OS-specific TCP RST;
- UDP or packet-level loss/reordering;
- arbitrary byte corruption unless specifically needed by a release-baseline fault.

## DirectionEngine contract

Compile an immutable validated `FaultPlan` into per-connection runtime state.

Each connection/direction gets its own state:

- active/inactive probabilistic fault decision;
- fault-local RNG state;
- counters;
- queued bytes/timers;
- shutdown/termination state;
- evidence hooks/summary values.

Do not share mutable per-connection state across connections.

The engine must preserve ordered fault semantics. Document whether a write traverses faults sequentially as logical stages or whether an optimized combined machine is equivalent. Optimization may not reorder observable effects.

## Deterministic RNG v1

Implement and freeze the RNG contract from ADR 002.

Preferred:

- SplitMix64-v1 or another explicitly documented algorithm;
- exact seed-mixing byte/int encoding;
- exact conversion for bounded integer ranges;
- exact conversion for probability decisions;
- no floating-point ambiguity if an integer threshold can be used.

Commit golden vectors for:

- top-level seed derivation;
- proxy/connection/direction/fault sub-seeds;
- first N raw values;
- Bernoulli results at representative probabilities;
- jitter values;
- slice-size variation.

Include `RngVersion::V1` in evidence.

If an implementation crate is used instead of a local algorithm, amend ADR 002 and prove stable replay across the supported dependency version.

## Probability semantics

At connection initialization:

- probability 0 -> fault inactive without consuming unnecessary random state;
- probability 1 -> active deterministically;
- intermediate probability -> one fault-local Bernoulli decision.

That activation decision is stable for the life/generation of the fault instance unless a live mutation later explicitly creates a new generation.

Per-segment random values for active faults use the same fault-local substream after the activation decision.

Tests must show that adding unrelated concurrent connections/faults does not change another fault's values.

## Latency

### Required semantics

For each accepted segment:

- calculate a release time from acceptance time + configured delay + deterministic jitter;
- preserve order unless a future explicitly documented fault allows reordering;
- allow multiple segments to be queued simultaneously;
- release eligible segments as inner writer capacity permits.

### Buffering

Use bounded accounting by bytes and, if useful, segment count.

Configuration must have an explicit maximum. Choose a conservative default in M002 and document why; M004 can expose it in file/API syntax.

When full:

- stop accepting new caller bytes;
- return `Pending`;
- arrange wakeup when queue capacity becomes available.

Do not silently discard or allocate unboundedly.

### Jitter

Define the exact range and clipping behavior. For example, symmetric jitter in `[-jitter,+jitter]` with total delay floored at zero. Whatever semantics are chosen must be documented and golden-tested.

## Bandwidth

Implement a monotonic-time rate limiter.

Required explicit concepts:

- rate in bytes/sec internally;
- burst capacity;
- token/refill precision;
- partial-write behavior;
- behavior for very low rates;
- overflow-safe duration arithmetic.

The limiter should accept/buffer only what it can own under the global engine bound.

Do not use blocking sleeps.

Rate=0 should either be invalid or map deliberately to blackhole; prefer invalid in native API to keep semantics distinct. Toxiproxy compatibility can translate edge behavior as observed from the oracle.

## Blackhole / timeout

Native model should distinguish:

- indefinite blackhole;
- blackhole with a close/termination deadline.

Bytes accepted while blackholed are intentionally discarded according to the fault's contract and must be counted as such.

If the chosen implementation wants backpressure rather than discard, that is a different fault and must not be mislabeled. Initial Toxiproxy-compatible intent is discard while blocking passage.

Indefinite blackhole must still respond to engine/runtime cancellation.

## Limit data

Forward exactly up to the configured byte limit, then request termination.

Handle a caller `poll_write` buffer that straddles the limit:

- report only the number of bytes actually accepted under the contract;
- never claim the post-limit suffix was forwarded;
- define whether termination occurs immediately after the boundary or on the next poll;
- test the exact behavior.

Per-fault state is per connection/direction.

## Slow close

Only shutdown is delayed.

Ordinary writes/flushes before shutdown must not gain the slow-close delay solely because the fault exists.

Store an interruptible timer after the first shutdown request, preserve repeated-poll correctness, then delegate inner shutdown at the defined point.

## Slice

Split writes into smaller logical segments.

Define:

- average size;
- size variation;
- deterministic range;
- lower bound >= 1;
- optional inter-slice delay;
- whether caller write completion means all bytes were buffered by the bounded engine or physically written.

Byte preservation is mandatory.

Slicing is user-space stream segmentation, not IP MTU behavior.

## Disconnect/reset request

Core should support a termination outcome such as:

- graceful shutdown requested;
- hard reset requested.

The generic engine cannot guarantee TCP RST for arbitrary `AsyncWrite`.

Return/emit a typed termination request to the embedding layer rather than using unsafe/downcasting tricks inside core.

M003 maps hard-reset requests to concrete TCP capabilities.

## AsyncWrite state-machine requirements

The implementation must handle:

- partial inner writes;
- `Pending` after partial progress;
- caller re-poll with the same buffer contract;
- flush while queues/timers exist;
- shutdown while queues/timers exist;
- inner errors after some bytes were forwarded;
- waker replacement;
- timer completion;
- cancellation;
- vectored writes if advertised.

Avoid spawning detached tasks just to wake the writer after delays. Tokio `Sleep` or equivalent state held in the future/object is preferable.

## Evidence and counters

Core should expose a low-cardinality per-connection/direction summary suitable for server aggregation:

- bytes accepted from caller;
- bytes forwarded to inner stream;
- bytes intentionally discarded;
- number of slices/segments;
- cumulative injected delay if meaningful;
- fault activation decisions;
- termination request/outcome;
- RNG version and connection/fault identities needed for replay.

Do not put wall-clock absolute timestamps into deterministic equality tests.

## Tests

Use `tokio::time::pause` / `advance` for deterministic time-based tests where possible.

The detailed required matrix is in `plans/reference/verification-matrix.md`.

At minimum:

- all fault-specific cases listed there;
- byte-conservation property tests for every preserving fault and representative ordered combinations;
- arbitrary fragmentation/partial-write test stream;
- bounded-buffer exhaustion and wakeup;
- flush/shutdown with pending queues;
- unrelated scheduling-noise determinism test;
- RNG golden vectors;
- fault order tests;
- panic-free invalid configuration tests.

Use property testing (e.g. proptest) for arbitrary payload/fragment combinations if dependency cost is acceptable as dev-only.

## Benchmarks

Add a small Criterion or equivalent benchmark target if useful, but do not optimize prematurely.

Capture at least:

- raw in-memory duplex/inner writer baseline;
- empty `ChaosStream`;
- fixed latency with paused/synthetic clock where benchmark meaning is valid;
- slice overhead;
- bandwidth state-machine overhead excluding deliberate waiting if possible.

This baseline feeds M008; no numerical release budget is imposed yet.

## Documentation

Update:

- native fault semantics;
- ordering;
- buffering/backpressure;
- deterministic seed/replay contract;
- difference between stream slicing/drop semantics and actual packet/network loss;
- limitations of generic reset behavior.

Do not claim Toxiproxy compatibility yet.

## Ordered work packages

Execute in this order:

1. **WP1 — Runtime compiler/state model:** compile validated ordered `FaultPlan` values into per-connection/per-direction state with explicit accepted/forwarded/discarded accounting.
2. **WP2 — RNG v1:** implement seed derivation and fault-local deterministic streams, freeze golden vectors, then wire probability activation before adding randomized fault details.
3. **WP3 — Preserving timing/shape faults:** implement latency, bandwidth, and slicing with bounded queues, partial-I/O correctness, and paused-time tests.
4. **WP4 — Destructive/termination faults:** implement blackhole/timeout, limit-data, slow-close, graceful disconnect, and abstract hard-reset requests with exact byte-boundary semantics.
5. **WP5 — Composition correctness:** exercise ordered fault combinations, flush/shutdown, cancellation, inner errors, and arbitrary fragmentation; add byte-conservation property tests.
6. **WP6 — Evidence surface:** expose deterministic connection/direction summaries without payload capture.
7. **WP7 — Benchmarks/docs:** record empty/core fault baselines and reconcile native semantics/reproducibility documentation.
8. **WP8 — Closure pass:** run the complete core/property/golden-vector suite on the candidate commit and activate M003 only after evidence closes M002.

## Acceptance criteria

M002 closes only when:

- every release-baseline fault has a typed validated config and implementation;
- preserving faults satisfy byte-conservation tests under arbitrary partial I/O;
- time-based behavior is covered by paused-time tests;
- buffering is explicitly bounded and backpressure works;
- RNG v1 is frozen with golden vectors;
- per-connection probability is deterministic and scheduler-independent;
- flush/shutdown behavior is tested while fault state is pending;
- core still has no HTTP/listener/CLI dependencies;
- routine CI plus core-specific property tests pass;
- benchmark baseline is recorded;
- closure evidence is committed.

## Stop/rejection conditions

Stop and reopen design if:

- correct buffering requires unbounded queues;
- `ChaosStream` cannot honor `AsyncWrite` without reporting bytes it may later accidentally lose;
- implementation starts spawning an unbounded number of tasks/timers per chunk;
- fault ordering cannot be explained deterministically;
- deterministic output changes when unrelated concurrent tasks are introduced;
- a core fault requires TCP-specific socket access;
- latency implementation serializes each read/write such that delay inherently becomes the dominant throughput cap.

## Closure evidence

Create `plans/closure/M002-deterministic-stream-fault-engine-closure.md` with:

- candidate SHA;
- exact test/property/benchmark commands;
- RNG golden fixture identifiers;
- buffer-bound evidence;
- fault-by-fault verdict;
- known limitations;
- benchmark baseline;
- deviations/ADR changes.

Then update registry:

- M002 -> `closed`;
- M003 -> `ready`.
