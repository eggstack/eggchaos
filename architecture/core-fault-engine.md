# Core fault engine (`eggchaos-core`)

Part of [Eggchaos architecture overview](overview.md).

Evidence-first review handoff for the protocol-neutral deterministic
byte-stream fault substrate. All behavior below is read from the sources
cited; no inference about missing code.

Sources:

- `crates/eggchaos-core/src/lib.rs`
- `crates/eggchaos-core/src/plan.rs`
- `crates/eggchaos-core/src/engine.rs`
- `crates/eggchaos-core/src/stream.rs`
- `crates/eggchaos-core/src/policy.rs`
- `crates/eggchaos-core/src/rng.rs`
- `crates/eggchaos-core/src/datagram.rs`
- `crates/eggchaos-core/Cargo.toml`
- `docs/architecture.md`
- `plans/adrs/001-stream-fault-engine-boundary.md`
- `plans/adrs/002-determinism-and-live-mutation.md`

## 1. Purpose and boundary

`eggchaos-core` is protocol-neutral. It owns typed fault plans, validation,
deterministic identity-scoped randomness, and the write-side impairment state
machine over caller-owned Tokio-compatible full-duplex streams. From
`docs/architecture.md`:

- dependency direction `eggchaos-cli -> eggchaos-server -> eggchaos-core`;
  adapters (`eggchaos-toxiproxy`, `eggchaos-eggfetch`) sit over server/core
  authority and never hold alternate state;
- core does not know about HTTP, listeners, CLIs, Toxiproxy, or Eggfetch;
- empty plan delegates directly to the wrapped Tokio stream without queue or
  timer (`crates/eggchaos-core/src/stream.rs`: `ChaosStream::poll_write` /
  `poll_write_vectored` empty-engine direct path; `engine.rs`:
  `DirectionEngine::empty` / `is_empty`);
- `#![deny(unsafe_code)]` in `crates/eggchaos-core/src/lib.rs:2`;
- minimal direct deps in `crates/eggchaos-core/Cargo.toml`:
  `arc-swap`, `bytes`, `serde`, `thiserror`, `tokio`; dev-deps `proptest`,
  `serde_json`. Description: “Deterministic bounded byte-stream fault
  injection for Tokio”.

Boundary from `plans/adrs/001-stream-fault-engine-boundary.md`:

- canonical primitive is a wrapper over a caller-owned Tokio stream.
  `ChaosStream<T>` delegates `AsyncRead` to `inner` and implements
  `AsyncWrite` through a directional engine. Read-side pass-through in v1
  plus a read-side pump (see §5).
- for a standalone connection the client-side wrapper’s write engine is the
  downstream plan and the upstream-side wrapper’s write engine is the upstream
  plan; both are passed to `eggress_relay::relay_with_options`. Eggress
  remains relay / half-close authority; eggchaos must not fork it.
- one heterogeneous `DirectionEngine` state machine is preferred over nested
  generic wrapper types per fault, because fault lists are runtime data,
  dynamically updatable, JSON/TOML-configurable, and need ordered execution
  plus inspectable evidence.
- connection-level effects (total duration, hard reset capability, operator
  kill, service shutdown) are coordinated by the server/dialer supervisor via
  cancellation/termination signals. Listener/runtime lifecycle is not smuggled
  into core. `eggchaos-core` never applies TCP-specific behavior; the runtime
  edge maps the abstract termination signal to shutdown or reset
  (`docs/architecture.md`, `engine.rs`, `stream.rs`).

## 2. Public API inventory

### Datagram sibling engine (`datagram.rs`)

The datagram engine is a separate whole-message API. `DatagramPlan` preserves
ordered `DatagramFaultSpec` stages and validates unique IDs and bounded
duplicate/delay parameters. Its six `DatagramFaultKind` stages are delay with
symmetric jitter, loss, bounded duplication, hold-based reorder, payload
corruption, and whole-datagram bandwidth. `DatagramQueueLimits` bounds both
queued candidate count and bytes and classifies candidates larger than the
configured datagram maximum.

`DatagramDirectionEngine::admit` receives one complete `Bytes` payload and
one `PublishedDatagramPolicy` snapshot and returns a `DatagramAdmission`:
`Consumed` (oversize, fully configured-loss, or fully overflowed),
`Immediate(vec)` (no release delay while the scheduler is empty — accounted
exactly as queue-then-drain, including queued/high-water counters, and sent
without entering the heap), or `Queued` (the scheduler owns candidates).
Immediate items are sorted by `(ingress ordinal, copy index)` so both paths
emit identical order; an empty plan never allocates a candidate vector.
Candidate RNG uses the separate
`derive_datagram_seed` domain plus ingress ordinal, copy index, fault ID,
direction, proxy/association identity, and seed namespace. `take_ready` pops
due candidates from a min-heap keyed by `(deadline, ingress ordinal, copy
index)` — O(1) deadline peek, O(log n) insert, O(k log n) drain of k ready
candidates — with the same total order the previous full-queue sort produced;
the 14-case golden corpus is byte-for-byte unchanged. Old generations
remain queued with their already-decided outcomes after policy publication.
Queue overflow drops the newest candidate and is counted separately from
configured loss. No payload is retained in evidence.

Unlike `ChaosStream`, this API has no `AsyncWrite`, byte-stream buffering,
flush/shutdown contract, transport socket, or connection-level probability.
It is an engine primitive; a later server milestone owns UDP listener and
association lifecycle.

### `lib.rs`

- `Direction` (`crates/eggchaos-core/src/lib.rs:27-43`):
  `Upstream` (client→target), `Downstream` (target→client). Serde
  `lowercase`. `as_str()` returns stable `"upstream"` / `"downstream"`.
- `StreamCapabilities` (`lib.rs:47-54`):
  `{ graceful_shutdown: bool, half_close: bool, hard_reset: bool }`.
  Transport capabilities an embedding runtime may expose. Serde
  serializable.

Re-exports:

- `engine.rs`: `DirectionEngine`, `EngineEvidence`, `TerminationHandle`,
  `TerminationInfo`, `TerminationRequest`.
- `plan.rs`: `BandwidthConfig`, `BlackholeConfig`, `DisconnectConfig`,
  `FaultId`, `FaultKind`, `FaultPlan`, `FaultSpec`, `LatencyConfig`,
  `LimitDataConfig`, `Probability`, `RngVersion`, `SliceConfig`,
  `SlowCloseConfig`, `StreamLossConfig`, `ValidationError`,
  `FAULT_TYPE_NAMES`, `STREAM_LOSS_GRAIN_BYTES`, `STREAM_LOSS_TYPE_NAME`.
- `policy.rs`: `LivePolicy`, `PolicyConflict`, `PublishError`,
  `PublishedPolicy`.
- `rng.rs`: `derive_policy_seed`, `derive_seed`,
  `derive_stream_loss_seed`, `DeterministicRng`,
  `RngEvidence`.
- `stream.rs`: `ActiveFault`, `BidirectionalChaosStream`, `ChaosStream`,
  `DirectionSummary`, `EngineError`, `StreamEvidence`,
  `MAX_EVIDENCE_FAULTS`.

### `plan.rs` — typed plans

- `RngVersion` (`plan.rs:8-12`): `V1` (default). “SplitMix64 with the
  eggchaos v1 domain-separation encoding.”
- `FaultId` (`plan.rs:16-38`): opaque validated `String`. `new` rejects
  empty or `len > 128` with `ValidationError::InvalidFaultId`. `as_str()`,
  `Display`.
- `Probability` (`plan.rs:42-63`): `f64` in closed `[0, 1]`. `new` rejects
  NaN/infinite/out-of-range with `ProbabilityOutOfRange`. `get()`.
  `Default` is `1.0`.
- Config structs, all `Clone, Copy, PartialEq, Eq, Serialize, Deserialize`:
  - `LatencyConfig` (`plan.rs:67-74`):
    `{ delay: Duration, jitter: Duration, max_buffer_bytes: NonZeroU64 }`.
    Jitter is symmetric, clipped at zero total delay.
  - `BandwidthConfig` (`plan.rs:78-83`):
    `{ bytes_per_second: NonZeroU64, burst_bytes: NonZeroU64 }`.
  - `BlackholeConfig` (`plan.rs:87-90`):
    `{ close_after: Option<Duration> }`. `None` = indefinite.
  - `LimitDataConfig` (`plan.rs:94-97`): `{ bytes: NonZeroU64 }`.
  - `SlowCloseConfig` (`plan.rs:101-104`): `{ delay: Duration }`.
    Shutdown only, never ordinary writes.
  - `SliceConfig` (`plan.rs:108-115`):
    `{ average_size: NonZeroU64, variation: u64, delay: Duration }`.
    Lower bound remains at least one.
   - `DisconnectConfig` (`plan.rs:126-132`):
     `{ after: Duration #[serde(default)], hard_reset: bool }`.
     `after == ZERO` terminates at the first defined contract boundary (first
     write/flush poll after activation). Positive `after` defers to that
     monotonic deadline while preserving bytes accepted before it. Comment
     records the M009 corrective note: `after` was added pre-1.0 to express
     Toxiproxy delayed `reset_peer`.
 - `StreamLossConfig` (`plan.rs`): `{ loss_rate: Probability, correlation:
   Probability }`. Deterministic userspace stream-chunk loss (ADR 007),
   not IP/TCP packet loss. `STREAM_LOSS_GRAIN_BYTES = 32768` is the frozen
   v1 logical grain; `STREAM_LOSS_TYPE_NAME = "stream-loss"` is the stable
   native spelling (only the Toxiproxy presentation may say `packet_loss`).
- `FaultKind` (`plan.rs`): 8 variants: the 7 legacy ones above plus
  `StreamLoss(StreamLossConfig)`. `FaultKind` is `PartialEq` (not `Eq`:
  loss probabilities are `f64`-backed).
- `FAULT_TYPE_NAMES` (`plan.rs`):
  `["latency","bandwidth","blackhole","limit-data","slow-close","slice",
  "disconnect"]` stays exactly seven entries in legacy order. Order is
  part of the evidence contract and must not change.
  `FaultKind::type_name()` matches directly (`stream-loss` included);
  `FaultKind::type_index()` returns `Option<usize>` — `Some(0..6)` for
  legacy faults, `None` for `StreamLoss`, which reports through additive
  named evidence instead of a legacy activation slot.
- `FaultSpec` (`plan.rs:187-194`):
  `{ id: FaultId, probability: Probability, kind: FaultKind }` plus
  `validate()`.
- `FaultPlan` (`plan.rs:198-266`): ordered validated `Vec<FaultSpec>`.
  `new(Vec<FaultSpec>)` rejects duplicates and invalid stages;
  `empty()`, `faults()`, `is_empty()`, `get(id)`, `with_fault`,
  `without_fault` (order-preserving retain), `replace_fault` (in-place
  preserve order, append if missing), `validate()`.
- `ValidationError` (`plan.rs:291-313`, `thiserror`):
  `ProbabilityOutOfRange`, `InvalidFaultId` (`1..=128 bytes`),
  `DuplicateFaultId(String)`, `ZeroCapacity`, `InvalidSlice`
  (“variation must be smaller than average size”), `InvalidRate`,
  `ZeroLimit`.

### `engine.rs` — directional state machine

- `TerminationRequest` (`engine.rs:25-30`): `Graceful`, `HardReset`.
- `TerminationInfo` (`engine.rs:34-41`):
  `{ request, direction: Direction, fault_id: Option<String> }`.
- `TerminationHandle` (`engine.rs:57-124`): durable level-triggered signal
  (`Arc<Mutex<Option<TerminationInfo>>>` + `Notify`). `new()`, `Default`,
  `get() -> Option<TerminationInfo>`, `publish(info) -> bool` (first wins;
  `true` only when this call set the value), `terminated().await`
  (resolves immediately if already published). Private `poll_terminated`
  registers the `Notify` waker before checking so a racing publish wakes the
  waiter.
- `EngineEvidence` (`engine.rs`): `Copy` counters:
  `bytes_accepted`, `bytes_forwarded`, `bytes_discarded`, `segments`,
  `slices`, `buffered_bytes`, `high_water_bytes`, `injected_delay_ms`
  (latency), `throttled_delay_ms` (bandwidth), `termination:
  Option<TerminationRequest>`, `activations: [u64; 7]` indexed by
  `FaultKind::type_index`, `rng_version: RngVersion`, plus additive
  stream-loss fields `stream_loss_chunks_evaluated`,
  `stream_loss_chunks_dropped`, `stream_loss_bytes_discarded` (unique
  chunks / counted-once bytes across all active stream-loss faults; the
  byte field is included in aggregate `bytes_discarded`). Activation rule
  is documented inline: preserving stages (latency, bandwidth, slice) and
  limit count once per `accept` they engage in; blackhole counts per
  discarding call; termination-requesting stages (disconnect, limit
  exhaustion, finite blackhole close) count once on first publish;
  slow-close counts once when a positive shutdown delay is enforced.
  Stream loss owns no legacy activation slot; in the loss path preserving
  stages engage once per `accept` call in which they queue at least one
  byte.
- `DirectionEngine` (`engine.rs:299-321`, methods `335-991`):
  - constructors: `new(plan, run_seed, proxy, connection_key, direction)`,
    `new_with_termination(..., term_handle)` (shares the durable handle so a
    live transition never erases a due request), `empty()` (cheap no-stage
    path, `max_buffer = 1`, direction `Upstream`).
  - observers: `is_empty()`, `plan()`, `direction()`, `evidence()` (fills
    `buffered_bytes`/`high_water_bytes` from live queue state),
    `termination_request()`, `termination_info()`, `termination_handle()`,
    `buffered_bytes()`, `capacity_bytes()`, `queue_is_empty()`.
  - pipeline: `accept(&mut self, input: &[u8]) -> usize`,
    `poll_due_termination(cx) -> Poll<TerminationInfo>` (never
    `Ready(None)`; arms deadline timers else parks on the handle),
    `poll_flush(cx, inner)`, `poll_shutdown(cx, inner)`.
  - internals: `Queued { bytes: Bytes, release: Instant }`,
    deadline-keyed `ArmedTimer` (comment at `engine.rs:171-184` documents the
    prior stale-`Sleep` reuse bug and the fix), `TokenBucket` (microtoken
    fixed point, see §3), `BlackholeState { deadline, fault_id }`,
    `DisconnectState { deadline, hard_reset, fault_id }`.
   - compile policy in `new_with_termination` (`engine.rs`):
     per-fault `derive_seed(...)` + one `bernoulli(probability)` draw in plan
     order; inactive faults push `(false, rng)` and contribute no state.
     Active-fault combination is first-wins except: `max_buffer` is the `min`
     across active latency bounds over a `64*1024` default (then `.max(1)`);
     first `LimitData` wins; indefinite blackhole dominates finite, else
     earliest finite deadline wins; first `Bandwidth` wins; any `Slice`
     sets `slicer_active`; earliest `Disconnect` deadline wins (ties prefer
     `hard_reset`); first active `SlowClose` sets the shutdown delay.
     Every active `StreamLoss` fault additionally gets a
     `StreamLossConnState` holding a dedicated chunk RNG seeded by
     `derive_stream_loss_seed` (domain-separated from activation and
     per-segment draws), the newest decided chunk/outcome, and the
     previous-drop bit. No engine-wide stream-loss combination exists:
     composition is applied per logical chunk at accept time.
   - `accept` with stream loss (`accept_with_stream_loss`): classifies the
     limit-bounded input into chunk-bounded runs keyed to the absolute
     `stream_loss_offset` (frozen while blackhole owns the prefix).
     `decide_stream_loss_chunk` evaluates each chunk across every active
     loss fault in plan order (drop if any drops; per-fault `prev_dropped`
     advances from the fault's own decision) and counts unique evaluated /
     dropped chunks once. Dropped runs resolve immediately with no retained
     payload; preserving runs are truncated to the remaining queue capacity
     (a longer run must never stall the write with no timer armed) and flow
     through `push_preserve_run` (slicer/latency/bandwidth, survivor bytes
     only). Acceptance stops at the first preserving byte the bound cannot
     own and never skips ahead to later drops. The limit counts discards;
     `bytes_discarded`/`stream_loss_bytes_discarded` reconcile with
     `accepted = forwarded + discarded + buffered`.

### `policy.rs` — live publication

- `PublishedPolicy` (`policy.rs:12-22`):
  `{ generation: u64, plan: Arc<FaultPlan>, seed_namespace: u64 }`.
  Immutable snapshot; plan/generation/namespace move together via one atomic
  load. Manual updates retain the current namespace; scenario runs publish
  derived namespaces (see `rng.rs`).
- `LivePolicy` (`policy.rs:26-137`, `Arc<ArcSwap<PublishedPolicy>>`):
  `new(plan, seed_namespace)` starts at generation `1`; `Default` is empty
  plan + namespace `0`. `snapshot() -> Arc<PublishedPolicy>`,
  `plan()`, `generation()`, `seed_namespace()`,
  `publish(plan, seed_namespace)` validates then stores generation+1,
  `publish_expected(plan, seed_namespace, expected)` validates, returns
  `Conflict` without changing state on stale base, else CAS-loops
  (`compare_and_swap` + `Arc::ptr_eq` proof).
- `PolicyConflict` (`policy.rs:46-51`): `{ expected, found }`.
- `PublishError` (`policy.rs:55-60`):
  `Invalid(ValidationError)` (nothing published),
  `Conflict(PolicyConflict)` (nothing published).

### `rng.rs` — determinism primitive

- `RngEvidence` (`rng.rs:9-14`): `{ version: RngVersion, seed: u64 }`.
- `derive_seed(run_seed, proxy, connection_key, direction, fault)`
  (`rng.rs:17-36`): stable sub-seed from explicit identity. Mixes
  `run_seed + 0x9e3779b97f4a7c15`, each byte of `proxy` + `fault.id`,
  `connection_key.rotate_left(17)`, direction constant (`0x5550` upstream /
  `0x444e` downstream), final `splitmix`.
- `derive_policy_seed(scenario_seed, run_id, event_index)` (`rng.rs:54-63`):
  pure function of the triple; never scheduling/wall/connection-order
  dependent.
- `DeterministicRng` (`rng.rs:65-98`): `new(seed) const`,
  `next_u64()` (`state += 0x9e3779b97f4a7c15` then `splitmix`),
  `below(upper)` (`0` for empty range else `next % upper`),
  `bernoulli(p)` (`false` if `<= 0`, `true` if `>= 1`, else
  `next <= (p * u64::MAX) as u64`).
- Golden vectors in `rng.rs:106-124` (see §6).

### `stream.rs` — Tokio adapters

- `DirectionSummary` (`stream.rs`): serializable per-direction
  counters mirroring `EngineEvidence` plus transparent direct-path bytes
  (see below) and the additive `stream_loss_*` fields. No payloads.
- `MAX_EVIDENCE_FAULTS` (`stream.rs:50`): `128`.
- `ActiveFault` (`stream.rs:54-59`): `{ id: String, fault_type: String }`
  (`FAULT_TYPE_NAMES` spelling, no payloads).
- `StreamEvidence` (`stream.rs:65-168`): lock-shared atomics
  (`observed_generation`, `pending_generation`, `seed_namespace`,
  accepted/forwarded/discarded, `high_water_bytes`, `transitions`,
  `activations`, `active_faults: Mutex<(Vec<ActiveFault>, bool)>`,
  `direct_bytes`). Readers: `observed_generation()`,
  `pending_generation()`, `seed_namespace()`, `transitions()`,
  `byte_counts()`, `high_water_bytes()`, `activations()`, `active_faults()
  -> (Vec<ActiveFault>, truncated: bool)`. Writers: `refresh_policy`
  (truncates at 128 with flag), `mirror_engine` (adds `direct_bytes` to
  accepted/forwarded), `note_direct` (direct path accepts+forwards
  atomically).
- `EngineError` (`stream.rs:172-176`): `Validation(#[from]
  ValidationError)`.
- `ChaosStream<T>` (`stream.rs:184-345`, `349-494`, `496-537`):
  `new(inner, plan, run_seed, proxy, connection_key, direction)`,
  `passthrough(inner, direction)`, `new_live(inner, policy, proxy,
  connection_key, direction)` (compiles from the atomic snapshot’s plan +
  `seed_namespace`), `direction()`, `observed_generation()`,
  `pending_generation()`, `stream_evidence()`, `termination_handle()`,
  `termination_info()`, `poll_termination()` (`Pending` for empty engines),
  `summary()` (engine evidence + direct bytes), `into_inner/get_ref/get_mut`.
  `AsyncRead` read-pumps releasable queued writes on every read and maps
  drained graceful termination to EOF (live bytes delivered first).
  `AsyncWrite` implements the §4 contract including vectored writes
  (joins `IoSlice`s then `poll_write`) and `update_live` barrier (see §5).
  Termination write error is `ConnectionAborted`
  (`terminated_error`, `stream.rs:540-556`).
- `BidirectionalChaosStream<T>` (`stream.rs:561-834`): physical stream with
  independent upstream/downstream `DirectionEngine`s plus `output:
  VecDeque<u8>` for downstream delivery. `new_live(inner, upstream,
  downstream, proxy, connection_key)`, `upstream_termination()`,
  `downstream_termination()`, `update_policies` barrier. Read path drives
  downstream queue into `output` via internal `CaptureWriter`, serves flushed
  output before the next inner read (peer-close safety), maps drained
  graceful to EOF and hard-reset to `ConnectionReset`. Write path mirrors
  `ChaosStream` for the upstream engine. Read-side also pumps the upstream
  queue for pooled connections.

## 3. Per-fault semantics (M009 baseline, `docs/architecture.md:31-68`)

Release-baseline execution, each grounded in `engine.rs` / `stream.rs`:

- latency (`LatencyConfig`): each accepted segment gets an independent
  `accept_time + base_delay + deterministic_jitter` deadline
  (`engine.rs:756-777`). Segments accepted together share similar deadlines
  and drain as a burst; the delay is not serialized once per write
  (test `latency_does_not_multiply_across_fragmented_writes`,
  `stream.rs:997-1020`). Jitter is symmetric in
  `±jitter_ms` via `rng.below(2*jitter+1) - jitter`, total floored at zero.
  Byte order preserved. `injected_delay_ms` accumulates per-segment millis.
  Bound is `max_buffer_bytes`; compile takes the minimum across active
  latencies over a 64 KiB default (`engine.rs:381-383`). Activation index 0
  once per engaging `accept`.
- bandwidth (`BandwidthConfig`): integer fixed-point token bucket
  (`engine.rs:208-283`). Sustained `bytes_per_second`, capacity
  `burst_bytes`, starts full (documented, tested in
  `token_bucket_grants_initial_burst_then_limits`). Microtokens
  (`bytes * 1e6`); refill only from monotonic Tokio-clock elapsed time;
  `consume(len, now, earliest)` returns `(release, throttled_ms)` where the
  throttle delay is ceiling millis beyond the latency-derived earliest time.
  Excess stays queued until tokens refill; long idle grants at most one burst
  (`token_bucket_capped_after_long_idle`). First active bandwidth config wins
  (`engine.rs:413-417`). Activation index 1 once per engaging `accept`;
  `throttled_delay_ms` accumulates.
- blackhole/timeout (`BlackholeConfig`): `close_after = None` discards
  indefinitely until policy transition or runtime cancellation and never
  self-terminates (`blackhole_counts_discarded_bytes`,
  `stream.rs:1146-1160`). `close_after = Some(d)` discards until the
  deadline, then publishes graceful termination that fires even with no
  further application write via `poll_due_termination` + `term_timer`
  (`finite_blackhole_terminates_at_deadline_without_further_writes`,
  `stream.rs:1162-1192`). Discard path bypasses the bound (no bytes
  retained) but honors the remaining limit prefix
  (`engine.rs:703-737`; `finite_blackhole_reports_prefix_only_and_terminates`).
  Counts `bytes_discarded`, `segments`, activation index 2 per discarding
  call plus one on finite close. Compile: indefinite dominates finite, else
  earliest finite wins (`engine.rs:389-410`).
- limit-data (`LimitDataConfig`): accepts/forwards at most the remaining
  count and returns only the accepted prefix length
  (`engine.rs:678-701,739-741,840-862`; proptest
  `limit_data_accepts_exact_prefix`). At zero, graceful termination is
  published after the accepted prefix resolves; the caller suffix is never
  reported as accepted. Later writes fail deterministically with
  `ConnectionAborted` once the prefix drains (`limit_data_reports_only_...
  `stream.rs:917-943`, exact-boundary table
  `limit_data_boundaries_are_exact`). First limit wins. Activation index 3
  once per engaging `accept` plus once on exhaustion.
- slicer (`SliceConfig`): deterministic symmetric sizes in
  `[average - variation, average + variation]`, lower-bounded at one, drawn
  from the first active slicer’s fault-local stream via
  `rng.below(2*variation+1)` (`engine.rs:638-661`). Configured `delay` is
  applied between logical slices via `slice_cursor` staggering, not once per
  caller write (`slicer_inter_slice_delay_staggers_delivery`,
  `stream.rs:1121-1144`; 3×4-byte slices with 50 ms inter-slice delay drain
  at +100 ms). Preserves bytes exactly (proptest
  `preserving_accept_conserves_bytes`, fragmentation test to 200 bytes).
  Counts `slices`, activation index 5.
- disconnect (`DisconnectConfig`): publishes at `now + after`;
  `after == ZERO` is due at the first contract boundary
  (`refresh_sync_terminations` on `accept` /
  `poll_due_termination`; test `zero_delay_disconnect_is_due_at_first_boundary`).
  `hard_reset = true` requests hard reset, else graceful. Bytes accepted
  before the deadline still drain (`disconnect_signals_are_durable_and_typed`).
  Delayed disconnect fires with no further writes via the deadline timer
  (`delayed_disconnect_fires_without_further_writes`). Compile keeps the
  earliest deadline, ties preferring hard reset (`engine.rs:421-439`).
  Activation index 6 once on first publish.
- slow-close (`SlowCloseConfig`): delays `poll_shutdown` only, never ordinary
  writes. `poll_shutdown` first drains the queue, then arms
  `shutdown_deadline = now + delay` once and waits via `shutdown_timer`
  (`engine.rs`; `shutdown_delivers_pending_latency_queue`). First
  active delay wins (`engine.rs`). Activation index 4 once when a
  positive shutdown delay is enforced (`slow_close_shutdown_counts_activation`).
- stream-loss (`StreamLossConfig`, ADR 007, M036): userspace logical-chunk
  loss with grain `STREAM_LOSS_GRAIN_BYTES = 32768`; byte offset `n`
  belongs to chunk `n / 32768`, decided once and reused across writes.
  Per active fault: `p(drop[0]) = loss_rate`,
  `p(drop[n]) = min(1, loss_rate + correlation)` after a dropped chunk,
  drawn from the fault-local chunk stream (`derive_stream_loss_seed`).
  `FaultSpec::probability` gates per-connection activation first; inactive
  faults decide nothing. Multiple active loss faults compose by union
  (drop if any drops; fault-local `prev_dropped`; bytes counted once).
  Blackhole dominates while discarding (loss state frozen); the limit
  counts the accepted prefix including discards; latency/bandwidth/slicer
  see survivors only and never move grain boundaries; disconnect/slow-close
  semantics are unchanged. `loss_rate = 0` preserves exactly;
  `loss_rate = 1` discards everything with zero high-water. Golden corpus
  plus fragmentation proptests live in
  `crates/eggchaos-core/tests/stream_loss.rs`.

Termination is never a TCP behavior in core: the runtime edge maps
`Graceful` / `HardReset` to shutdown vs reset using `StreamCapabilities`.
Ordinary `poll_shutdown` is never advertised as RST
(`docs/architecture.md:22-26`).

## 4. Write/flush contract

From `docs/architecture.md:33-38`, ADR 001 §Required AsyncWrite correctness,
`engine.rs:663-863,914-991`, `stream.rs:393-464,766-834`:

- `poll_write` may report acceptance as soon as `DirectionEngine::accept`
  owns the bytes in its bounded queue; physical delivery is not required.
  `poll_flush` is the barrier guaranteeing every preserving accepted byte
  has reached the inner writer (`poll_queue` drains in release order then
  `inner.poll_flush`).
- `accept` returns the owned prefix length only. Zero means: empty input,
  full bound, exhausted limit, or due termination. Blackhole discard path
  reports the accepted (discarded) prefix instead of buffering.
  Stream-loss discard runs likewise report the discarded prefix without
  consuming the bound; preserve runs are truncated to remaining capacity
  so a call either consumes bytes or leaves the pre/post-accept drive's
  wakeup armed (never a bare `Pending` with an empty queue).
- when the bound is full, `poll_write` returns `Pending` after arming the
  release timer (`ArmedTimer::poll` keyed by exact deadline). It never
  reports zero-length success for a non-empty write and never allocates
  beyond the limit: `accepted_cap = min(input, limit, capacity)`; the chunk
  loop breaks if `buffered + chunk > max_buffer`; `offset == 0` returns 0
  which the stream maps to `Pending` (or `ConnectionAborted` if terminated
  and drained). Empty caller input `b""` still returns `Ok(0)` per Tokio.
- `poll_write` opportunistically drives releasable bytes before `accept`
  (frees capacity, arms timers, never blocks acceptance) and drives
  already-due bytes immediately after `accept`, because the embedding relay
  never flushes mid-stream. Errors from the pre-accept drive fail the write;
  errors from the post-accept drive surface on the next pre-accept drive.
  Reads additionally pump releasable queued writes
  (`read_pumps_releasable_queued_writes`).
- `poll_queue` forwards via `inner.poll_write`; inner `Ok(0)` becomes
  `WriteZero` I/O error; partial inner writes advance the head slice;
  `bytes_forwarded` and `buffered` track exactly. `poll_shutdown` drains the
  queue first, then enforces slow-close, then `inner.poll_shutdown`.
- empty engines take the direct path: delegate to `inner.poll_write` /
  `poll_write_vectored`, count via `StreamEvidence::note_direct`, never touch
  engine counters. Evidence/summary add direct bytes to accepted+forwarded so
  no-fault traffic reconciles (`direct_path_bytes_count_toward_evidence`).

## 5. Termination and generation swaps

Level-triggered termination (`engine.rs:43-124,571-636,870-912`,
`stream.rs:304-312,372-387,426-446`):

- durable: `TerminationHandle` stores the first published `TerminationInfo`;
  later publishes (including across a generation swap sharing the handle)
  return `false` and are dropped. Late waiters via `terminated().await` or
  `poll_due_termination` still observe the first request.
- sync publishers: due disconnect and finite-blackhole close in
  `refresh_sync_terminations` (called from `accept` and
  `poll_due_termination`); limit exhaustion in `accept`. Async arming:
  `poll_due_termination` takes the earliest disconnect/finite-blackhole
  deadline, waits it with `term_timer`, republishes on expiry, else parks on
  the handle. It never resolves `Ready(None)`.
- stream mapping: writes after termination with an empty queue fail with
  `ConnectionAborted` (`terminated_error` includes fault id + direction);
  non-empty queue keeps returning `Pending` until the accepted prefix drains.
  `ChaosStream::poll_read` delivers live inner bytes first; only a
  would-be-idle read with drained queue + graceful request becomes EOF
  (lets the relay observe the FIN analog). Hard-reset reads keep inner
  behavior in `ChaosStream` (runtime owns abortive close); in
  `BidirectionalChaosStream` (no runtime polling the handle) drained graceful
  becomes EOF and hard-reset becomes `ConnectionReset`.
- evidence: `EngineEvidence.termination`, `DirectionSummary.termination`,
  `termination_request()` / `termination_info()`.

Barrier generation swap (ADR 002 barrier-transition,
`stream.rs:496-537,626-676`):

- streams observe the live generation on every write/flush/shutdown (and
  `ChaosStream::poll_write` observes before the empty-engine fast path so an
  empty direct-path connection still transitions without reconnecting;
  `live_publish_from_empty_direct_path_engages_fault`).
- transition: publish `pending_generation` to evidence, drain preserving
  bytes — `ChaosStream` requires `queue_is_empty` or a ready `poll_flush`;
  bidirectional upstream uses the same rule, downstream additionally requires
  `output.is_empty` and drains via `CaptureWriter`. Discarded blackhole bytes
  stay discarded.
- swap via `DirectionEngine::new_with_termination` sharing the old
  termination handle, then `observed_generation = snapshot.generation`,
  `refresh_policy` (active-fault list truncated at 128 with flag),
  `pending_generation = 0`, `transitions += 1`, `mirror_engine`.
  Byte limits and RNG state restart per generation; a due termination
  survives (`live_transition_preserves_due_termination`). Stream-loss
  chunk decisions, correlation bits, and the absolute offset cursor
  restart with the fresh engine (ADR 007: no cross-generation loss-state
  migration). All three of
  plan/generation/seed-namespace come from one `snapshot()` load so they
  always agree.

## 6. Determinism

From ADR 002 and `rng.rs` / `engine.rs`:

- identity components per fault instance: `run_seed`, `proxy_identity`,
  `connection_key` (standalone: proxy-local monotonic accept ordinal,
  included in evidence; embedded callers may supply a stable app key),
  `direction`, `fault_identity`, `rng_version` (`RngVersion::V1`).
  No draw is shared across fault instances or directions; no
  process-global or scheduler-order RNG.
- seed namespaces: manual control updates retain the policy’s current
  namespace; scenario runs publish `derive_policy_seed(scenario_seed, run_id,
  event_index)` namespaces so a scenario seed participates in the decisions
  it reproduces (`rng.rs:46-63`, `policy.rs:17-21`).
- algorithm: frozen SplitMix64-v1 (`rng.rs:38-44,69-77`). Golden vectors
  pinned in `rng.rs:106-124`:
  - `DeterministicRng::new(42).next_u64()` → `2949826092126892291`,
    then `5139283748462763858`;
  - `derive_seed(42, "proxy", 7, Upstream, "latency")` →
    `11882912530514077282`;
  - `derive_policy_seed(7,1,0)` → `4026889766568732747`;
    `(7,1,1)` → `11250473848183634583`;
    `(8,1,0)` → `3937417822122820953`
    (each identity component participates).
- helpers: `below(upper)` for ranges (slice sizes, jitter offsets),
  `bernoulli(p)` for connection activation. Probability semantics
  (ADR 002): `0` never activates, `1` always activates, intermediate values
  make one deterministic Bernoulli choice per connection from the
  fault-local substream at compile time (`engine.rs`; test
  `probability_zero_never_activates_and_one_always_does` — inactive faults
  keep the 64 KiB default bound). Stream-loss chunk draws use the separate
  `derive_stream_loss_seed` domain (fixed additive constant over
  `derive_seed`; existing vectors byte-identical), advanced exactly once
  per absolute logical chunk in visit order, so loss traces are
  fragmentation-independent even though per-segment draws (slice sizes,
  jitter) remain fragmentation-sensitive as before. Queue-capacity
  truncation of preserve runs only re-segments queueing, never chunk
  identity. Per-segment randomness (slice sizes, jitter
  offsets) uses subsequent draws from the same fault-local substream in plan
  order, so identical `(seed, proxy, key, direction, plan)` reproduces
  identical `accept` results and evidence
  (`identical_inputs_give_identical_deterministic_state`,
  `seed_namespace_changes_probabilistic_decisions_reproducibly`,
  slicer determinism under scheduling noise in `stream.rs:1077-1119`).
- evidence carries `rng_version`; scenario replay additionally needs
  config hash, run seed + version, proxy/fault ids, connection keys,
  generation transitions with monotonic scenario-relative timestamps,
  kills/resets, termination outcomes, discarded counts (ADR 002 §Scenario
  replay).

## 7. Validation and failure semantics

- plan validation (`plan.rs:202-287`): `FaultPlan::new` / `with_fault` /
  `replace_fault` / `validate` enforce §2 rules. Failures return
  `ValidationError`; nothing partial is constructed. `LivePolicy::publish` /
  `publish_expected` validate before storing; `Invalid` publishes nothing.
  `publish_expected` on stale base returns
  `Conflict { expected, found }` and publishes nothing (CAS loop for races).
  `ChaosStream` / `BidirectionalChaosStream` constructors map plan errors to
  `EngineError::Validation`; `update_live`’s `.expect("published policies
  are validated")` holds because only validated plans publish.
- runtime failures (not validation):
  - inner `Ok(0)` → `WriteZero` I/O error (`engine.rs:932-937`).
  - inner I/O errors propagate from `poll_queue` / `poll_flush` /
    `poll_shutdown`; the write path owns error reporting, the read pump
    ignores pump errors.
  - limit-exhausted / disconnect / finite-blackhole terminations → write
    `ConnectionAborted` after drain; read EOF (`Graceful`) or
    `ConnectionReset` (`HardReset`, bidirectional only).
  - backpressure is `Pending` + timer wakeup, never silent drop of preserving
    bytes (ADR 001: a live update must never forget owned bytes; the barrier
    swap enforces it).
  - destructive accounting is explicit: only blackhole counts
    `bytes_discarded`; preserving faults conserve bytes exactly (proptests
    `preserving_accept_conserves_bytes`,
    `preserving_combination_conserves_bytes_under_fragmentation`).

## 8. Review checklist

Verify file by file (line refs above are the contract):

- `lib.rs`: `forbid(unsafe_code)` present; `Direction` serde lowercase +
  `as_str` stable; `StreamCapabilities` three bools; re-export list matches
  §2 (no missing `FaultId`/`RngVersion`/`PublishError`/evidence types).
- `plan.rs`: ordered `FaultPlan` preserved by `without_fault` /
  `replace_fault`; `FAULT_TYPE_NAMES` is exactly seven entries in legacy
  order (evidence/metrics dependency); `FaultKind::type_name` covers all
  eight kinds while `type_index` returns `None` for `StreamLoss`;
  `StreamLossConfig` probabilities validate finite `[0, 1]` (including
  deserialized values); `STREAM_LOSS_GRAIN_BYTES` is `32768`;
  `DisconnectConfig.after` has `#[serde(default)]` for
  pre-`after` JSON; every `ValidationError` variant reachable and messaged;
  `NonZeroU64` fields plus explicit zero guards agree.
- `engine.rs`: `ArmedTimer` keyed by exact deadline (no shared-`Sleep`
  reuse); `TokenBucket` microtoken math (burst starts full, idle caps at one
  burst, Tokio-clock only, ceiling-ms throttle accounting); compile
  first-wins/dominance rules (§2–3) match tests; `accept` reports owned
  prefix only, empty→0, full→0→`Pending` upstream, never over-allocates;
  the stream-loss path truncates preserve runs to remaining capacity so a
  non-full queue always makes progress; activation indices 0–6 match
  `type_index`, stream loss uses only the additive `stream_loss_*`
  counters with unique-chunk / counted-once-bytes rules;
  `publish_termination` first-wins end-to-end (handle + local + evidence);
  `poll_due_termination` arms both disconnect and finite-blackhole deadlines
  and otherwise parks on the handle.
- `stream.rs`: `poll_write` observes live generation before the direct path;
  direct bytes counted via `note_direct` and added in `summary` /
  `mirror_engine`; post-accept immediate drive + read pump present (relay
  never flushes mid-stream); graceful→EOF only after delivering live bytes
  and draining; `MAX_EVIDENCE_FAULTS = 128` truncation flag set;
  `BidirectionalChaosStream` serves flushed `output` before inner reads and
  requires empty `output` before downstream swaps; vectored writes join then
  delegate.
- `policy.rs`: `ArcSwap` single-load `snapshot()` is the only
  generation+plan+namespace source for stream transitions; generations start
  at 1 and increment exactly once per successful publish; `publish_expected`
  conflict returns `{expected, found}` without mutation.
- `rng.rs`: golden vectors in tests match §6 exactly; direction constants
  `0x5550` / `0x444e` unchanged; `below(0) == 0`, `bernoulli` edges at 0/1,
  threshold `p * u64::MAX`; `derive_policy_seed` sensitive to all three
  inputs.
- `Cargo.toml`: no new deps beyond the six direct ones without ADR-level
  justification; `proptest` + `serde_json` remain dev-only.
- `docs/architecture.md` M009 list vs §3 above: wording matches
  (acceptance vs barrier, burst latency, microtoken bucket, blackhole
  deadlines, prefix-only limit, inter-slice delay, `after == ZERO` boundary,
  shutdown-only slow-close, level-triggered first-wins termination, drain
  before swap).
- ADRs: no HTTP/listeners in core (001), no global RNG, no immediate
  structural swap that forgets bytes, no OS-entropy chaos decisions (002).

Commands (run from the workspace root):

```sh
cargo test -p eggchaos-core
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --check
```

Minimum closure evidence per repo discipline: unit tests per fault state
machine, byte-conservation proptests, half-close/shutdown tests,
bounded-buffer/backpressure tests, RNG golden vectors, JSON round trips,
and a record of any platform/oracle evidence that could not be run (never
substitute inspection for execution).
