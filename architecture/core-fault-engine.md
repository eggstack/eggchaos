# Core fault engine (`eggchaos-core`)

Part of [Eggchaos architecture overview](overview.md).

Evidence-first review handoff for the protocol-neutral deterministic
byte-stream fault substrate. All behavior below is read from the sources
cited at HEAD (`e050fa2`, M041); no inference about missing code.

Sources:

- `crates/eggchaos-core/src/lib.rs`
- `crates/eggchaos-core/src/plan.rs`
- `crates/eggchaos-core/src/engine.rs`
- `crates/eggchaos-core/src/stream.rs`
- `crates/eggchaos-core/src/policy.rs`
- `crates/eggchaos-core/src/rng.rs`
- `crates/eggchaos-core/src/datagram.rs`
- `crates/eggchaos-core/tests/stream_loss.rs`
- `crates/eggchaos-core/tests/datagram_golden.rs`
- `crates/eggchaos-core/Cargo.toml`
- `docs/architecture.md`
- `plans/adrs/001-stream-fault-engine-boundary.md`
- `plans/adrs/002-determinism-and-live-mutation.md`

## 1. Purpose and boundary

`eggchaos-core` is protocol-neutral. It owns typed fault plans, validation,
deterministic identity-scoped randomness, and the write-side impairment state
machine over caller-owned Tokio-compatible full-duplex streams. From
`docs/architecture.md:3-18`:

- dependency direction `eggchaos-cli -> eggchaos-server ->
  eggchaos-protocol -> eggchaos-experiment -> eggchaos-core` (full chain
  per `architecture/overview.md`; adapters (`eggchaos-toxiproxy`,
  `eggchaos-eggfetch`) sit over server/core authority and never hold
  alternate state);
- core does not know about HTTP, listeners, CLIs, Toxiproxy, or Eggfetch;
- empty plan delegates directly to the wrapped Tokio stream without queue or
  timer (`crates/eggchaos-core/src/stream.rs`: `ChaosStream::poll_write` /
  `poll_write_vectored` empty-engine direct path at `stream.rs:507-523,
  586-594`; `engine.rs`: `DirectionEngine::empty` at `engine.rs:572-603` /
  `is_empty` at `engine.rs:606-608`);
- `#![deny(unsafe_code)]` in `crates/eggchaos-core/src/lib.rs:2`;
- minimal direct deps in `crates/eggchaos-core/Cargo.toml`:
  `arc-swap`, `bytes`, `serde`, `thiserror`, `tokio` (`Cargo.toml:15-20`);
  dev-deps `proptest`, `serde_json` (`Cargo.toml:22-24`). Description:
  “Deterministic bounded byte-stream fault injection for Tokio”
  (`Cargo.toml:13`).

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
  (`docs/architecture.md:33-37,95-101`, `engine.rs`, `stream.rs`).

## 2. Public API inventory

### Datagram sibling engine (`datagram.rs`)

The datagram engine is a separate whole-message API. `DatagramPlan`
(`datagram.rs:95-155`) preserves ordered `DatagramFaultSpec` stages
(`datagram.rs:57-62`) and validates unique IDs, the 256-stage cap, bounded
duplicate parameters (`additional_copies` in `1..=16`, amplification cap
4096), and delay/jitter/hold bounds (at most 24 h)
(`datagram.rs:107-148`). Its six `DatagramFaultKind` stages
(`datagram.rs:65-86`) are delay with symmetric jitter, loss, bounded
duplication, hold-based reorder, payload corruption, and whole-datagram
bandwidth, named by `DATAGRAM_FAULT_TYPE_NAMES` (`datagram.rs:17-24`).
`DatagramQueueLimits` (`datagram.rs:38-54`) bounds queued candidate count,
queued bytes, and the per-datagram maximum (hard caps 1M datagrams / 1 GiB /
65535 bytes), and classifies candidates larger than the configured datagram
maximum as oversize.

`DatagramDirectionEngine::admit` (`datagram.rs:355-518`) receives one
complete `Bytes` payload and one `PublishedDatagramPolicy` snapshot and
returns a `DatagramAdmission` (`datagram.rs:275-282`):
`Consumed` (oversize at `datagram.rs:368-370`, fully configured-loss with an
empty candidate set at `datagram.rs:495-497`, or fully overflowed at
`datagram.rs:513-517`), `Immediate(vec)` (no release delay while the
scheduler is empty — accounted exactly as queue-then-drain via
`emit_immediate` at `datagram.rs:555-583`, including queued/high-water
counters, and sent without entering the heap), or `Queued` (the scheduler
owns candidates via `enqueue` at `datagram.rs:533-551`). Immediate items are
sorted by `(ingress ordinal, copy index)` (`datagram.rs:492`) so both paths
emit identical order; an empty plan returns before any candidate vector is
allocated (`datagram.rs:374-385`). Candidate RNG uses the separate
`derive_datagram_seed` domain (`datagram.rs:27-35`, golden vector at
`datagram.rs:720-727`) plus ingress ordinal, copy index, fault ID,
direction, proxy/association identity, and seed namespace
(`datagram.rs:388-399`). `take_ready` (`datagram.rs:645-661`) pops due
candidates from a min-heap keyed by `(deadline, ingress ordinal, copy
index)` (`Candidate::cmp` at `datagram.rs:259-263`) — O(1) deadline peek,
O(log n) insert, O(k log n) drain of k ready candidates — with the same
total order the previous full-queue sort produced; the frozen 14-case golden
corpus in `tests/datagram_golden.rs:57-62` pins byte-exact traces. Old
generations remain queued with their already-decided outcomes after policy
publication (`generation_snapshot_deadline_and_bounds_are_preserved`,
`datagram.rs:855-882`). Queue overflow drops the newest candidate and is
counted separately from configured loss (`queue_overflow` vs
`configured_loss`, `DatagramEvidence` at `datagram.rs:201-224`). No payload
is retained in evidence.

Unlike `ChaosStream`, this API has no `AsyncWrite`, byte-stream buffering,
flush/shutdown contract, transport socket, or connection-level probability.
It is an engine primitive; the server UDP runtime (`runtime/datagram/`,
`DatagramRuntime`) owns listener and association lifecycle (see
[overview §5](overview.md) and `docs/architecture.md:103-114`).

### `lib.rs`

- `Direction` (`crates/eggchaos-core/src/lib.rs:37-44`):
  `Upstream` (client→target), `Downstream` (target→client). Serde
  `lowercase`. `as_str()` (`lib.rs:46-54`) returns stable `"upstream"` /
  `"downstream"`.
- `StreamCapabilities` (`lib.rs:56-65`):
  `{ graceful_shutdown: bool, half_close: bool, hard_reset: bool }`.
  Transport capabilities an embedding runtime may expose. Serde
  serializable.

Re-exports (`lib.rs:11-34`):

- `datagram.rs`: `derive_datagram_seed`, `DatagramAdmission`,
  `DatagramDirectionEngine`, `DatagramEvidence`, `DatagramFaultKind`,
  `DatagramFaultSpec`, `DatagramLivePolicy`, `DatagramPlan`,
  `DatagramQueueLimits`, `DatagramScheduled`, `PublishedDatagramPolicy`,
  `DATAGRAM_FAULT_TYPE_NAMES`.
- `engine.rs`: `DirectionEngine`, `EngineEvidence`, `TerminationHandle`,
  `TerminationInfo`, `TerminationRequest`.
- `plan.rs`: `BandwidthConfig`, `BlackholeConfig`, `DisconnectConfig`,
  `FaultId`, `FaultKind`, `FaultPlan`, `FaultSpec`, `LatencyConfig`,
  `LimitDataConfig`, `Probability`, `RngVersion`, `SliceConfig`,
  `SlowCloseConfig`, `StreamLossConfig`, `ValidationError`,
  `FAULT_TYPE_NAMES`, `STREAM_LOSS_GRAIN_BYTES`, `STREAM_LOSS_TYPE_NAME`.
- `policy.rs`: `LivePolicy`, `PolicyConflict`, `PublishError`,
  `PublishedPolicy`.
- `rng.rs`: `derive_policy_seed`, `derive_schedule_policy_seed`,
  `derive_seed`, `derive_stream_loss_seed`, `DeterministicRng`,
  `RngEvidence`.
- `stream.rs`: `ActiveFault`, `BidirectionalChaosStream`,
  `BidirectionalEvidenceSnapshot`, `ChaosStream`,
  `DirectionEvidenceSnapshot`, `DirectionSummary`, `EngineError`,
  `LiveBidirectionalEvidence`, `StreamEvidence`, `MAX_EVIDENCE_FAULTS`.

### `plan.rs` — typed plans

- `RngVersion` (`plan.rs:6-12`): `V1` (default). “SplitMix64 with the
  eggchaos v1 domain-separation encoding.”
- `FaultId` (`plan.rs:14-38`): opaque validated `String`. `new`
  (`plan.rs:20-26`) rejects empty or `len > 128` with
  `ValidationError::InvalidFaultId`. `as_str()`, `Display`.
- `Probability` (`plan.rs:40-63`): `f64` in closed `[0, 1]`. `new`
  (`plan.rs:46-52`) rejects NaN/infinite/out-of-range with
  `ProbabilityOutOfRange`. `get()`. `Default` is `1.0` (`plan.rs:59-63`).
- Config structs, all `Clone, Copy, PartialEq, Eq, Serialize, Deserialize`:
  - `LatencyConfig` (`plan.rs:65-74`):
    `{ delay: Duration, jitter: Duration, max_buffer_bytes: NonZeroU64 }`.
    Jitter is symmetric, clipped at zero total delay.
  - `BandwidthConfig` (`plan.rs:76-83`):
    `{ bytes_per_second: NonZeroU64, burst_bytes: NonZeroU64 }`.
  - `BlackholeConfig` (`plan.rs:85-90`):
    `{ close_after: Option<Duration> }`. `None` = indefinite.
  - `LimitDataConfig` (`plan.rs:92-97`): `{ bytes: NonZeroU64 }`.
  - `SlowCloseConfig` (`plan.rs:99-104`): `{ delay: Duration }`.
    Shutdown only, never ordinary writes.
  - `SliceConfig` (`plan.rs:106-115`):
    `{ average_size: NonZeroU64, variation: u64, delay: Duration }`.
    Lower bound remains at least one.
   - `DisconnectConfig` (`plan.rs:125-132`):
     `{ after: Duration #[serde(default)], hard_reset: bool }`
     (`#[serde(default)]` at `plan.rs:128`).
     `after == ZERO` terminates at the first defined contract boundary (first
     write/flush poll after activation). Positive `after` defers to that
     monotonic deadline while preserving bytes accepted before it. Comment
     records the M009 corrective note (`plan.rs:117-124`): `after` was added
     pre-1.0 to express Toxiproxy delayed `reset_peer`.
  - `StreamLossConfig` (`plan.rs:134-149`): `{ loss_rate: Probability,
    correlation: Probability }`. Deterministic userspace stream-chunk loss
    (ADR 007), not IP/TCP packet loss. `STREAM_LOSS_GRAIN_BYTES = 32768`
    (`plan.rs:158`, declared as `32 * 1024`) is the frozen v1 logical grain;
    `STREAM_LOSS_TYPE_NAME = "stream-loss"` (`plan.rs:164`) is the stable
    native spelling (only the Toxiproxy presentation may say `packet_loss`).
- `FaultKind` (`plan.rs:166-185`): 8 variants: the 7 legacy ones above plus
  `StreamLoss(StreamLossConfig)`. `FaultKind` is `PartialEq` (not `Eq`:
  loss probabilities are `f64`-backed).
- `FAULT_TYPE_NAMES` (`plan.rs:190-198`):
  `["latency","bandwidth","blackhole","limit-data","slow-close","slice",
  "disconnect"]` stays exactly seven entries in legacy order. Order is
  part of the evidence contract and must not change.
  `FaultKind::type_name()` (`plan.rs:206-217`) matches directly
  (`stream-loss` included); `FaultKind::type_index()`
  (`plan.rs:224-235`) returns `Option<usize>` — `Some(0..6)` for
  legacy faults, `None` for `StreamLoss`, which reports through additive
  named evidence instead of a legacy activation slot.
- `FaultSpec` (`plan.rs:238-247`):
  `{ id: FaultId, probability: Probability, kind: FaultKind }` plus
  `validate()`.
- `FaultPlan` (`plan.rs:249-319`): ordered validated `Vec<FaultSpec>`.
  `new(Vec<FaultSpec>)` (`plan.rs:257-266`) rejects duplicates and invalid
  stages; `empty()` (`plan.rs:268-270`), `faults()` (`plan.rs:272-274`),
  `is_empty()` (`plan.rs:276-278`), `get(id)` (`plan.rs:280-282`),
  `with_fault` (`plan.rs:284-291`), `without_fault` (order-preserving
  retain, `plan.rs:293-296`), `replace_fault` (in-place preserve order,
  append if missing, `plan.rs:298-311`), `validate()`
  (`plan.rs:313-318`).
- `FaultSpec::validate` (`plan.rs:321-348`): latency zero-capacity,
  slice `variation >= average`, bandwidth zero rate/burst, limit zero
  bytes, and stream-loss non-finite/out-of-range probabilities
  (`plan.rs:337-344`, surfaces as `ProbabilityOutOfRange`).
- `ValidationError` (`plan.rs:350-374`, `thiserror`):
  `ProbabilityOutOfRange`, `InvalidFaultId` (`1..=128 bytes`),
  `DuplicateFaultId(String)`, `ZeroCapacity`, `InvalidSlice`
  (“variation must be smaller than average size”), `InvalidRate`,
  `ZeroLimit`.

### `engine.rs` — directional state machine

- `TerminationRequest` (`engine.rs:23-30`): `Graceful`, `HardReset`.
- `TerminationInfo` (`engine.rs:32-41`):
  `{ request, direction: Direction, fault_id: Option<String> }`.
- `TerminationHandle` (`engine.rs:56-124`): durable level-triggered signal
  (`Arc<Mutex<Option<TerminationInfo>>>` + `Notify`). `new()`
  (`engine.rs:69-76`), `Default` (`engine.rs:61-65`),
  `get() -> Option<TerminationInfo>` (`engine.rs:79-81`),
  `publish(info) -> bool` (`engine.rs:85-94`, first wins; `true` only when
  this call set the value), `terminated().await` (`engine.rs:98-106`,
  resolves immediately if already published). Private `poll_terminated`
  (`engine.rs:108-123`) registers the `Notify` waker before checking so a
  racing publish wakes the waiter.
- `EngineEvidence` (`engine.rs:126-187`): `Copy` counters:
  `bytes_accepted`, `bytes_forwarded`, `bytes_discarded`, `segments`,
  `slices`, `buffered_bytes`, `high_water_bytes`, `injected_delay_ms`
  (latency), `throttled_delay_ms` (bandwidth), `termination:
  Option<TerminationRequest>`, `activations: [u64; 7]` indexed by
  `FaultKind::type_index` (activation rule documented at
  `engine.rs:150-163`), `rng_version: RngVersion`, plus additive
  stream-loss fields `stream_loss_chunks_evaluated`,
  `stream_loss_chunks_dropped`, `stream_loss_bytes_discarded`
  (`engine.rs:164-184`: unique chunks / counted-once bytes across all
  active stream-loss faults; the byte field is included in aggregate
  `bytes_discarded`). Activation rule is documented inline: preserving stages
  (latency, bandwidth, slice) and limit count once per `accept` they engage
  in; blackhole counts per discarding call; termination-requesting stages
  (disconnect, limit exhaustion, finite blackhole close) count once on first
  publish; slow-close counts once when a positive shutdown delay is enforced.
  Stream loss owns no legacy activation slot; in the loss path preserving
  stages engage once per `accept` call in which they queue at least one
  byte.
- `DirectionEngine` (struct `engine.rs:342-380`):
  - constructors: `new(plan, run_seed, proxy, connection_key, direction)`
    (`engine.rs:396-411`),
    `new_with_termination(..., term_handle)` (`engine.rs:415-569`, shares
    the durable handle so a live transition never erases a due request),
    `empty()` (`engine.rs:572-603`, cheap no-stage path, `max_buffer = 1`,
    direction `Upstream`).
  - observers: `is_empty()` (`engine.rs:606-608`), `plan()`
    (`engine.rs:611-613`), `direction()` (`engine.rs:616-618`),
    `evidence()` (`engine.rs:621-626`, fills `buffered_bytes` /
    `high_water_bytes` from live queue state), `termination_request()`
    (`engine.rs:629-631`), `termination_info()` (`engine.rs:634-636`),
    `termination_handle()` (`engine.rs:639-641`), `buffered_bytes()`
    (`engine.rs:644-646`), `capacity_bytes()` (`engine.rs:649-651`),
    `queue_is_empty()` (`engine.rs:654-656`).
  - pipeline: `accept(&mut self, input: &[u8]) -> usize`
    (`engine.rs:1013-1208`),
    `poll_due_termination(cx) -> Poll<TerminationInfo>`
    (`engine.rs:1215-1257`, never `Ready(None)`; arms deadline timers else
    parks on the handle), `poll_flush(cx, inner)`
    (`engine.rs:1299-1308`), `poll_shutdown(cx, inner)`
    (`engine.rs:1311-1336`).
  - internals: `Queued { bytes: Bytes, release: Instant }`
    (`engine.rs:189-193`), deadline-keyed `ArmedTimer`
    (`engine.rs:195-230`; comment at `engine.rs:195-203` documents the
    prior stale-`Sleep` reuse bug and the fix), `TokenBucket` (microtoken
    fixed point, `engine.rs:232-307`, see §3), `BlackholeState { deadline,
    fault_id }` (`engine.rs:309-313`), `DisconnectState { deadline,
    hard_reset, fault_id }` (`engine.rs:315-320`).
   - compile policy in `new_with_termination` (`engine.rs:415-569`):
     per-fault `derive_seed(...)` + one `bernoulli(probability)` draw in plan
     order (`engine.rs:432-440`); inactive faults push `(false, rng)` and
     contribute no state. Active-fault combination is first-wins except:
     `max_buffer` is the `min` across active latency bounds over a `64*1024`
     default (then `.max(1)`, `engine.rs:442-444,549`); first `LimitData`
     wins (`engine.rs:445-449`); indefinite blackhole dominates finite, else
     earliest finite deadline wins (`engine.rs:450-471`); first `Bandwidth`
     wins (`engine.rs:474-478`); any `Slice` sets `slicer_active`
     (`engine.rs:479-481`); earliest `Disconnect` deadline wins (ties prefer
     `hard_reset`, `engine.rs:482-500`); first active `SlowClose` sets the
     shutdown delay (`engine.rs:524-536`). Every active `StreamLoss` fault
     additionally gets a `StreamLossConnState` (`engine.rs:322-339`,
     constructed at `engine.rs:508-521`) holding a dedicated chunk RNG
     seeded by `derive_stream_loss_seed` (domain-separated from activation
     and per-segment draws), the newest decided chunk/outcome, and the
     previous-drop bit. No engine-wide stream-loss combination exists:
     composition is applied per logical chunk at accept time.
   - `accept` with stream loss (`accept_with_stream_loss`,
     `engine.rs:896-1002`): classifies the limit-bounded input into
     chunk-bounded runs keyed to the absolute `stream_loss_offset`
     (`engine.rs:908-916`; frozen while blackhole owns the prefix per the
     offset docs at `engine.rs:367-374`).
     `decide_stream_loss_chunk` (`engine.rs:760-802`) evaluates each chunk
     across every active loss fault in plan order (drop if any drops;
     per-fault `prev_dropped` advances from the fault's own decision) and
     counts unique evaluated / dropped chunks once. Dropped runs resolve
     immediately with no retained payload; preserving runs are truncated to
     the remaining queue capacity (`engine.rs:932-936`: a longer run must
     never stall the write with no timer armed) and flow through
     `push_preserve_run` (`engine.rs:810-884`; slicer/latency/bandwidth,
     survivor bytes only). Acceptance stops at the first preserving byte
     the bound cannot own and never skips ahead to later drops. The limit
     counts discards (`engine.rs:979-998`); `bytes_discarded` /
     `stream_loss_bytes_discarded` reconcile with `accepted = forwarded +
     discarded + buffered`.

### `policy.rs` — live publication

- `PublishedPolicy` (`policy.rs:11-22`):
  `{ generation: u64, plan: Arc<FaultPlan>, seed_namespace: u64 }`.
  Immutable snapshot; plan/generation/namespace move together via one atomic
  load. Manual updates retain the current namespace; scenario runs publish
  derived namespaces (see `rng.rs`).
- `LivePolicy` (struct `policy.rs:25-28`, `Arc<ArcSwap<PublishedPolicy>>`;
  impl `policy.rs:62-137`): `new(plan, seed_namespace)`
  (`policy.rs:63-72`) starts at generation `1`; `Default`
  (`policy.rs:38-42`) is empty plan + namespace `0`.
  `snapshot() -> Arc<PublishedPolicy>` (`policy.rs:75-77`),
  `plan()` (`policy.rs:79-81`), `generation()` (`policy.rs:83-85`),
  `seed_namespace()` (`policy.rs:87-89`),
  `publish(plan, seed_namespace)` (`policy.rs:91-104`) validates then
  stores generation+1, `publish_expected(plan, seed_namespace, expected)`
  (`policy.rs:109-136`) validates, returns `Conflict` without changing
  state on stale base, else CAS-loops (`compare_and_swap` + `Arc::ptr_eq`
  proof).
- `PolicyConflict` (`policy.rs:44-51`): `{ expected, found }`.
- `PublishError` (`policy.rs:53-60`):
  `Invalid(ValidationError)` (nothing published),
  `Conflict(PolicyConflict)` (nothing published).

### `rng.rs` — determinism primitive

- `RngEvidence` (`rng.rs:7-14`): `{ version: RngVersion, seed: u64 }`.
- `derive_seed(run_seed, proxy, connection_key, direction, fault)`
  (`rng.rs:17-36`): stable sub-seed from explicit identity. Mixes
  `run_seed + 0x9e3779b97f4a7c15`, each byte of `proxy` + `fault.id`,
  `connection_key.rotate_left(17)`, direction constant (`0x5550` upstream /
  `0x444e` downstream), final `splitmix` (`rng.rs:38-44`).
- `derive_policy_seed(scenario_seed, run_id, event_index)`
  (`rng.rs:54-63`, contract documented at `rng.rs:46-53`): pure function of
  the triple; never scheduling/wall/connection-order dependent.
- `derive_schedule_policy_seed(scenario_seed, execution_key,
  schedule_fingerprint, compiled_event_index)` (`rng.rs:87-112`,
  contract at `rng.rs:65-86`): run_id-independent Scenario V2 namespace;
  daemon `run_id`, scheduling, wall time, and map order do not participate.
- `derive_stream_loss_seed(...)` (`rng.rs:122-131`, rationale at
  `rng.rs:113-121`): fixed additive domain constant over `derive_seed`;
  existing seed vectors byte-identical. Datagram sibling
  `derive_datagram_seed` (`datagram.rs:27-35`) XORs a separate datagram
  domain; golden vector at `datagram.rs:720-727`.
- `DeterministicRng` (`rng.rs:133-167`): `new(seed) const`
  (`rng.rs:140-142`), `next_u64()` (`rng.rs:144-147`, `state +=
  0x9e3779b97f4a7c15` then `splitmix`), `below(upper)`
  (`rng.rs:149-155`, `0` for empty range else `next % upper`),
  `bernoulli(p)` (`rng.rs:157-166`, `false` if `<= 0`, `true` if `>= 1`,
  else `next <= (p * u64::MAX) as u64`).
- Golden vectors in `rng.rs:169-243` (see §6).

### `stream.rs` — Tokio adapters

- `DirectionSummary` (`stream.rs:19-58`): serializable per-direction
  counters mirroring `EngineEvidence` plus transparent direct-path bytes
  (see below) and the additive `stream_loss_*` fields
  (`stream.rs:43-53`). No payloads.
- `MAX_EVIDENCE_FAULTS` (`stream.rs:60-61`): `128`.
- `ActiveFault` (`stream.rs:63-70`): `{ id: String, fault_type: String }`
  (`FAULT_TYPE_NAMES` spelling, no payloads).
- `StreamEvidence` (`stream.rs:72-272`): lock-shared atomics
  (`observed_generation`, `pending_generation`, `seed_namespace`,
  accepted/forwarded/discarded, `high_water_bytes`, `transitions`,
  `activations`, `active_faults: Mutex<(Vec<ActiveFault>, bool)>`,
  `direct_bytes`). Readers (`stream.rs:102-198`): `observed_generation()`,
  `pending_generation()`, `seed_namespace()`, `transitions()`,
  `byte_counts()`, `high_water_bytes()`, `activations()`, `active_faults()
  -> (Vec<ActiveFault>, truncated: bool)`, `snapshot()` (`stream.rs:160-188`
  into `DirectionEvidenceSnapshot`). Writers: `refresh_policy`
  (`stream.rs:199-219`, truncates at 128 with flag), `mirror_engine`
  (`stream.rs:220-262`, adds `direct_bytes` to accepted/forwarded),
  `note_direct` (`stream.rs:263-271`, direct path accepts+forwards
  atomically).
- `EngineError` (`stream.rs:274-280`): `Validation(#[from]
  ValidationError)`.
- `ChaosStream<T>` (struct `stream.rs:288-297`; constructors
  `stream.rs:299-378`; observers/summary `stream.rs:379-451`):
  `new(inner, plan, run_seed, proxy, connection_key, direction)`,
  `passthrough(inner, direction)`, `new_live(inner, policy, proxy,
  connection_key, direction)` (compiles from the atomic snapshot’s plan +
  `seed_namespace`), `direction()`, `observed_generation()`,
  `pending_generation()`, `stream_evidence()`, `termination_handle()`,
  `termination_info()`, `poll_termination()` (`stream.rs:408-416`,
  `Pending` for empty engines), `summary()` (engine evidence + direct
  bytes), `into_inner/get_ref/get_mut`. `AsyncRead`
  (`stream.rs:456-498`) read-pumps releasable queued writes on every read
  and maps drained graceful termination to EOF (live bytes delivered
  first). `AsyncWrite` (`stream.rs:500-600`) implements the §4 contract
  including vectored writes (joins `IoSlice`s then `poll_write`) and
  `update_live` barrier (`stream.rs:603-644`, see §5). Termination write
  error is `ConnectionAborted` (`terminated_error`, `stream.rs:647-663`).
- `BidirectionalChaosStream<T>` (struct `stream.rs:777-790`;
  constructors/observers `stream.rs:792-926`; `update_policies`
  `stream.rs:928-1000`; read `stream.rs:1003-1105`; write
  `stream.rs:1107-1185`): physical stream with independent
  upstream/downstream `DirectionEngine`s plus `output: VecDeque<u8>` for
  downstream delivery. `new_live(inner, upstream, downstream, proxy,
  connection_key)`, `upstream_termination()`,
  `downstream_termination()`, `update_policies` barrier. Read path drives
  downstream queue into `output` via internal `CaptureWriter`
  (`stream.rs:1187-1202`), serves flushed output before the next inner read
  (peer-close safety), maps drained graceful to EOF and hard-reset to
  `ConnectionReset`. Write path mirrors `ChaosStream` for the upstream
  engine. Read-side also pumps the upstream queue for pooled connections.

## 3. Per-fault semantics (M009 baseline, `docs/architecture.md:51-93`)

Release-baseline execution, each grounded in `engine.rs` / `stream.rs`:

- latency (`LatencyConfig`): each accepted segment gets an independent
  `accept_time + base_delay + deterministic_jitter` deadline
  (`push_preserve_run` latency arm at `engine.rs:833-849`; same shape on the
  non-loss path at `engine.rs:1107-1122`). Segments accepted together share
  similar deadlines and drain as a burst; the delay is not serialized once
  per write (test `latency_does_not_multiply_across_fragmented_writes`,
  `stream.rs:1349-1371`). Jitter is symmetric in
  `±jitter_ms` via `rng.below(2*jitter+1) - jitter`, total floored at zero.
  Byte order preserved. `injected_delay_ms` accumulates per-segment millis.
  Bound is `max_buffer_bytes`; compile takes the minimum across active
  latencies over a 64 KiB default (`engine.rs:442-444`). Activation index 0
  once per engaging `accept` (test
  `activations_count_per_type_deterministically`, `engine.rs:1367-1461`).
- bandwidth (`BandwidthConfig`): integer fixed-point token bucket
  (`engine.rs:232-307`). Sustained `bytes_per_second`, capacity
  `burst_bytes`, starts full (documented, tested in
  `token_bucket_grants_initial_burst_then_limits`, `engine.rs:1489-1500`).
  Microtokens (`bytes * 1e6`); refill only from monotonic Tokio-clock
  elapsed time; `consume(len, now, earliest)` (`engine.rs:276-306`)
  returns `(release, throttled_ms)` where the throttle delay is ceiling
  millis beyond the latency-derived earliest time. Excess stays queued until
  tokens refill; long idle grants at most one burst
  (`token_bucket_capped_after_long_idle`, `engine.rs:1503-1513`;
  cumulative-rate test at `engine.rs:1516-1526`). First active bandwidth
  config wins (`engine.rs:474-478`). Activation index 1 once per engaging
  `accept`; `throttled_delay_ms` accumulates.
- blackhole/timeout (`BlackholeConfig`): `close_after = None` discards
  indefinitely until policy transition or runtime cancellation and never
  self-terminates (`blackhole_counts_discarded_bytes`,
  `stream.rs:1498-1511`). `close_after = Some(d)` discards until the
  deadline, then publishes graceful termination that fires even with no
  further application write via `poll_due_termination` + `term_timer`
  (`finite_blackhole_terminates_at_deadline_without_further_writes`,
  `stream.rs:1514-1543`). Discard path bypasses the bound (no bytes
  retained) but honors the remaining limit prefix
  (`engine.rs:1044-1078`; `finite_blackhole_reports_prefix_only_and_terminates`,
  `engine.rs:1558-1587`). Counts `bytes_discarded`, `segments`, activation
  index 2 per discarding call plus one on finite close. Compile: indefinite
  dominates finite, else earliest finite wins (`engine.rs:450-471`).
- limit-data (`LimitDataConfig`): accepts/forwards at most the remaining
  count and returns only the accepted prefix length
  (`engine.rs:1019-1042` budget/cap, `engine.rs:1185-1204` exhaustion;
  proptest `limit_data_accepts_exact_prefix`, `engine.rs:1721-1752`). At
  zero, graceful termination is published after the accepted prefix
  resolves; the caller suffix is never reported as accepted. Later writes
  fail deterministically with `ConnectionAborted` once the prefix drains
  (`limit_data_reports_only_accepted_prefix_then_terminates`,
  `stream.rs:1269-1294`, exact-boundary table
  `limit_data_boundaries_are_exact`, `stream.rs:1297-1327`). First limit
  wins. Activation index 3 once per engaging `accept` plus once on
  exhaustion.
- slicer (`SliceConfig`): deterministic symmetric sizes in
  `[average - variation, average + variation]`, lower-bounded at one, drawn
  from the first active slicer’s fault-local stream via
  `rng.below(2*variation+1)` (`next_slice_size`, `engine.rs:725-748`).
  Configured `delay` is applied between logical slices via `slice_cursor`
  staggering, not once per caller write
  (`slicer_inter_slice_delay_staggers_delivery`, `stream.rs:1473-1495`;
  3×4-byte slices with 50 ms inter-slice delay drain at +100 ms). Preserves
  bytes exactly (proptest `preserving_accept_conserves_bytes`,
  `engine.rs:1698-1718`, fragmentation test to 200 bytes at
  `stream.rs:1713-1760`). Counts `slices`, activation index 5.
- disconnect (`DisconnectConfig`): publishes at `now + after`;
  `after == ZERO` is due at the first contract boundary
  (`refresh_sync_terminations` on `accept` /
  `poll_due_termination`, `engine.rs:670-693`; test
  `zero_delay_disconnect_is_due_at_first_boundary`,
  `engine.rs:1529-1555`). `hard_reset = true` requests hard reset, else
  graceful. Bytes accepted before the deadline still drain
  (`disconnect_signals_are_durable_and_typed`, `stream.rs:1546-1588`).
  Delayed disconnect fires with no further writes via the deadline timer
  (`delayed_disconnect_fires_without_further_writes`,
  `stream.rs:1591-1619`). Compile keeps the earliest deadline, ties
  preferring hard reset (`engine.rs:482-500`). Activation index 6 once on
  first publish.
- slow-close (`SlowCloseConfig`): delays `poll_shutdown` only, never ordinary
  writes. `poll_shutdown` first drains the queue, then arms
  `shutdown_deadline = now + delay` once and waits via `shutdown_timer`
  (`engine.rs:1311-1336`; `shutdown_delivers_pending_latency_queue`,
  `stream.rs:1649-1663`). First active delay wins
  (`engine.rs:524-536`). Activation index 4 once when a positive shutdown
  delay is enforced (`slow_close_shutdown_counts_activation`,
  `stream.rs:1878-1890`).
- stream-loss (`StreamLossConfig`, ADR 007, M036): userspace logical-chunk
  loss with grain `STREAM_LOSS_GRAIN_BYTES = 32768`; byte offset `n`
  belongs to chunk `n / 32768`, decided once and reused across writes.
  Per active fault: `p(drop[0]) = loss_rate`,
  `p(drop[n]) = min(1, loss_rate + correlation)` after a dropped chunk,
  drawn from the fault-local chunk stream (`derive_stream_loss_seed`,
  `rng.rs:122-131`). `FaultSpec::probability` gates per-connection
  activation first; inactive faults decide nothing. Multiple active loss
  faults compose by union (drop if any drops; fault-local `prev_dropped`;
  bytes counted once). Blackhole dominates while discarding (loss state
  frozen: the blackhole path at `engine.rs:1044-1078` returns before the
  stream-loss dispatch at `engine.rs:1080-1082`, and the offset cursor is
  frozen per `engine.rs:367-374`); the limit counts the accepted prefix
  including discards (`engine.rs:979-998`); latency/bandwidth/slicer see
  survivors only and never move grain boundaries;
  disconnect/slow-close semantics are unchanged. `loss_rate = 0` preserves
  exactly; `loss_rate = 1` discards everything with zero high-water. Golden
  corpus plus fragmentation proptests live in
  `crates/eggchaos-core/tests/stream_loss.rs` (chunk-seed golden
  `196256752510381490` at `stream_loss.rs:1000-1053`, pinned mixed trace at
  `stream_loss.rs:1055+`).

Termination is never a TCP behavior in core: the runtime edge maps
`Graceful` / `HardReset` to shutdown vs reset using `StreamCapabilities`.
Ordinary `poll_shutdown` is never advertised as RST
(`docs/architecture.md:33-37`).

## 4. Write/flush contract

From `docs/architecture.md:42-50`, ADR 001 §Required AsyncWrite correctness,
`engine.rs:1004-1336`, `stream.rs:500-600,1107-1185`:

- `poll_write` may report acceptance as soon as `DirectionEngine::accept`
  owns the bytes in its bounded queue; physical delivery is not required.
  `poll_flush` is the barrier guaranteeing every preserving accepted byte
  has reached the inner writer (`poll_queue` drains in release order then
  `inner.poll_flush`, `engine.rs:1259-1308`).
- `accept` returns the owned prefix length only. Zero means: empty input
  (`engine.rs:1014-1016`), full bound, exhausted limit
  (`engine.rs:1019-1038`), or due termination. Blackhole discard path
  reports the accepted (discarded) prefix instead of buffering
  (`engine.rs:1044-1078`). Stream-loss discard runs likewise report the
  discarded prefix without consuming the bound; preserve runs are truncated
  to remaining capacity (`engine.rs:932-936`) so a call either consumes
  bytes or leaves the pre/post-accept drive's wakeup armed (never a bare
  `Pending` with an empty queue).
- when the bound is full, `poll_write` returns `Pending` after arming the
  release timer (`ArmedTimer::poll` keyed by exact deadline,
  `engine.rs:213-229`). It never reports zero-length success for a non-empty
  write and never allocates beyond the limit: `accepted_cap =
  min(input, limit, capacity)` (`engine.rs:1039-1042`); the chunk loop
  breaks if `buffered + chunk > max_buffer` (`engine.rs:1097-1099`);
  `offset == 0` returns 0 which the stream maps to `Pending` (or
  `ConnectionAborted` if terminated and drained). Empty caller input `b""`
  still returns `Ok(0)` per Tokio (`stream.rs:530-532`).
- `poll_write` opportunistically drives releasable bytes before `accept`
  (frees capacity, arms timers, never blocks acceptance) and drives
  already-due bytes immediately after `accept`, because the embedding relay
  never flushes mid-stream (`stream.rs:527-529,537-545`). Errors from the
  pre-accept drive fail the write; errors from the post-accept drive surface
  on the next pre-accept drive. Reads additionally pump releasable queued
  writes (`read_pumps_releasable_queued_writes`, `stream.rs:1622-1646`).
- `poll_queue` forwards via `inner.poll_write`; inner `Ok(0)` becomes
  `WriteZero` I/O error (`engine.rs:1277-1282`); partial inner writes
  advance the head slice (`engine.rs:1283-1290`); `bytes_forwarded` and
  `buffered` track exactly. `poll_shutdown` drains the queue first, then
  enforces slow-close, then `inner.poll_shutdown` (`engine.rs:1311-1336`).
- empty engines take the direct path: delegate to `inner.poll_write` /
  `poll_write_vectored`, count via `StreamEvidence::note_direct`, never touch
  engine counters. Evidence/summary add direct bytes to accepted+forwarded so
  no-fault traffic reconciles (`direct_path_bytes_count_toward_evidence`,
  `stream.rs:1242-1257`).

## 5. Termination and generation swaps

Level-triggered termination (`engine.rs:658-723,1210-1257`,
`stream.rs:408-416,647-663`):

- durable: `TerminationHandle` stores the first published `TerminationInfo`;
  later publishes (including across a generation swap sharing the handle)
  return `false` and are dropped. Late waiters via `terminated().await` or
  `poll_due_termination` still observe the first request.
- sync publishers: due disconnect and finite-blackhole close in
  `refresh_sync_terminations` (`engine.rs:670-713`, called from `accept` and
  `poll_due_termination`); limit exhaustion in `accept`
  (`engine.rs:1023-1038,1185-1204`). Async arming: `poll_due_termination`
  (`engine.rs:1215-1257`) takes the earliest disconnect/finite-blackhole
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
  becomes EOF and hard-reset becomes `ConnectionReset`
  (`stream.rs:1067-1088`).
- evidence: `EngineEvidence.termination`, `DirectionSummary.termination`,
  `termination_request()` / `termination_info()`.

Barrier generation swap (ADR 002 barrier-transition,
`stream.rs:603-644,928-1000`):

- streams observe the live generation on every write/flush/shutdown (and
  `ChaosStream::poll_write` observes before the empty-engine fast path so an
  empty direct-path connection still transitions without reconnecting;
  `live_publish_from_empty_direct_path_engages_fault`,
  `stream.rs:1834-1856`).
- transition: publish `pending_generation` to evidence, drain preserving
  bytes — `ChaosStream` requires `queue_is_empty` or a ready `poll_flush`
  (`stream.rs:618-624`); bidirectional upstream uses the same rule
  (`stream.rs:935-939`), downstream additionally requires `output.is_empty`
  and drains via `CaptureWriter` (`stream.rs:967-977`). Discarded blackhole
  bytes stay discarded.
- swap via `DirectionEngine::new_with_termination` sharing the old
  termination handle, then `observed_generation = snapshot.generation`,
  `refresh_policy` (active-fault list truncated at 128 with flag),
  `pending_generation = 0`, `transitions += 1`, `mirror_engine`.
  Byte limits and RNG state restart per generation; a due termination
  survives (`live_transition_preserves_due_termination`,
  `stream.rs:1683-1710`). Stream-loss chunk decisions, correlation bits,
  and the absolute offset cursor restart with the fresh engine (ADR 007: no
  cross-generation loss-state migration). All three of
  plan/generation/seed-namespace come from one `snapshot()` load so they
  always agree.

## 6. Determinism

From ADR 002 and `rng.rs` / `engine.rs` / `datagram.rs`:

- identity components per fault instance: `run_seed`, `proxy_identity`,
  `connection_key` (standalone: proxy-local monotonic accept ordinal,
  included in evidence; embedded callers may supply a stable app key),
  `direction`, `fault_identity`, `rng_version` (`RngVersion::V1`).
  No draw is shared across fault instances or directions; no
  process-global or scheduler-order RNG.
- seed namespaces: manual control updates retain the policy’s current
  namespace; scenario runs publish `derive_policy_seed(scenario_seed, run_id,
  event_index)` namespaces (`rng.rs:46-63`, `policy.rs:11-22`) so a scenario
  seed participates in the decisions it reproduces, while Scenario V2
  publishes run_id-independent `derive_schedule_policy_seed` namespaces
  (`rng.rs:65-112`).
- algorithm: frozen SplitMix64-v1 (`rng.rs:38-44,138-147`). Golden vectors
  pinned in `rng.rs:169-243`:
  - `DeterministicRng::new(42).next_u64()` → `2949826092126892291`,
    then `5139283748462763858` (`rng.rs:174-184`);
  - `derive_seed(42, "proxy", 7, Upstream, "latency")` →
    `11882912530514077282` (`rng.rs:174-184`);
  - `derive_policy_seed(7,1,0)` → `4026889766568732747`;
    `(7,1,1)` → `11250473848183634583`;
    `(8,1,0)` → `3937417822122820953`
    (each identity component participates, `rng.rs:186-193`);
  - `derive_schedule_policy_seed` vectors plus per-axis sensitivity
    (`rng.rs:195-242`); v1 vectors re-asserted byte-identical there.
  - `derive_datagram_seed(42, "proxy", 7, Upstream, "loss")` →
    `5358773348858769006` (`datagram.rs:720-727`);
  - `derive_stream_loss_seed(SEED, "proxy", 41, Upstream, "loss")` →
    `196256752510381490` (`tests/stream_loss.rs:1000-1053`).
- helpers: `below(upper)` for ranges (slice sizes, jitter offsets),
  `bernoulli(p)` for connection activation. Probability semantics
  (ADR 002): `0` never activates, `1` always activates, intermediate values
  make one deterministic Bernoulli choice per connection from the
  fault-local substream at compile time (`engine.rs:432-440`; test
  `probability_zero_never_activates_and_one_always_does`,
  `engine.rs:1628-1664` — inactive faults keep the 64 KiB default bound).
  Stream-loss chunk draws use the separate `derive_stream_loss_seed` domain
  (fixed additive constant over `derive_seed`; existing vectors
  byte-identical), advanced exactly once per absolute logical chunk in visit
  order, so loss traces are fragmentation-independent even though
  per-segment draws (slice sizes, jitter) remain fragmentation-sensitive as
  before. Queue-capacity truncation of preserve runs only re-segments
  queueing, never chunk identity. Per-segment randomness (slice sizes,
  jitter offsets) uses subsequent draws from the same fault-local substream
  in plan order, so identical `(seed, proxy, key, direction, plan)`
  reproduces identical `accept` results and evidence
  (`identical_inputs_give_identical_deterministic_state`,
  `engine.rs:1590-1625`;
  `seed_namespace_changes_probabilistic_decisions_reproducibly`,
  `engine.rs:1464-1486`; slicer determinism under scheduling noise in
  `stream.rs:1429-1470`).
- evidence carries `rng_version`; scenario replay additionally needs
  config hash, run seed + version, proxy/fault ids, connection keys,
  generation transitions with monotonic scenario-relative timestamps,
  kills/resets, termination outcomes, discarded counts (ADR 002 §Scenario
  replay).

## 7. Validation and failure semantics

- plan validation (`plan.rs:255-266,284-318` for `FaultPlan`;
  `FaultSpec::validate` at `plan.rs:321-348`): `FaultPlan::new` /
  `with_fault` / `replace_fault` / `validate` enforce §2 rules. Failures
  return `ValidationError`; nothing partial is constructed.
  `LivePolicy::publish` / `publish_expected` (`policy.rs:91-136`) validate
  before storing; `Invalid` publishes nothing. `publish_expected` on stale
  base returns `Conflict { expected, found }` and publishes nothing (CAS
  loop for races). `ChaosStream` / `BidirectionalChaosStream` constructors
  map plan errors to `EngineError::Validation`; `update_live`’s
  `.expect("published policies are validated")` (`stream.rs:628-636`,
  bidirectional at `stream.rs:941-949,979-987`) holds because only
  validated plans publish. Datagram plans revalidate on deserialization
  before publication (`DatagramPlan::validate` at `datagram.rs:107-148`,
  enforced by `DatagramLivePolicy::new` at `datagram.rs:172-181`; test at
  `datagram.rs:768-775`).
- runtime failures (not validation):
  - inner `Ok(0)` → `WriteZero` I/O error (`engine.rs:1277-1282`).
  - inner I/O errors propagate from `poll_queue` / `poll_flush` /
    `poll_shutdown`; the write path owns error reporting, the read pump
    ignores pump errors (`stream.rs:463-471`).
  - limit-exhausted / disconnect / finite-blackhole terminations → write
    `ConnectionAborted` after drain; read EOF (`Graceful`) or
    `ConnectionReset` (`HardReset`, bidirectional only).
  - backpressure is `Pending` + timer wakeup, never silent drop of preserving
    bytes (ADR 001: a live update must never forget owned bytes; the barrier
    swap enforces it). Datagram queue overflow is the bounded sibling: the
    newest candidate is dropped and counted as `queue_overflow`, never as
    configured loss.
  - destructive accounting is explicit: only blackhole counts
    `bytes_discarded` on the stream path (plus stream-loss discards, which
    reconcile into the same aggregate); preserving faults conserve bytes
    exactly (proptests `preserving_accept_conserves_bytes`,
    `engine.rs:1698-1718`;
    `preserving_combination_conserves_bytes_under_fragmentation`,
    `stream.rs:1713-1760`).

## 8. Review checklist

Verify file by file (line refs above are the contract):

- `lib.rs`: `forbid(unsafe_code)` present (`lib.rs:2`); `Direction` serde
  lowercase + `as_str` stable (`lib.rs:37-54`); `StreamCapabilities` three
  bools (`lib.rs:56-65`); re-export list matches §2 (stream + datagram +
  `derive_schedule_policy_seed` + `BidirectionalEvidenceSnapshot` /
  `DirectionEvidenceSnapshot` / `LiveBidirectionalEvidence` — no missing
  `FaultId`/`RngVersion`/`PublishError`/evidence types).
- `plan.rs`: ordered `FaultPlan` preserved by `without_fault` /
  `replace_fault`; `FAULT_TYPE_NAMES` is exactly seven entries in legacy
  order (evidence/metrics dependency); `FaultKind::type_name` covers all
  eight kinds while `type_index` returns `None` for `StreamLoss`;
  `StreamLossConfig` probabilities validate finite `[0, 1]` (including
  deserialized values, `plan.rs:337-344`); `STREAM_LOSS_GRAIN_BYTES` is
  `32768` (`plan.rs:158`); `DisconnectConfig.after` has `#[serde(default)]`
  for pre-`after` JSON; every `ValidationError` variant reachable and
  messaged; `NonZeroU64` fields plus explicit zero guards agree.
- `engine.rs`: `ArmedTimer` keyed by exact deadline (no shared-`Sleep`
  reuse, `engine.rs:195-230`); `TokenBucket` microtoken math
  (`engine.rs:232-307`: burst starts full, idle caps at one burst,
  Tokio-clock only, ceiling-ms throttle accounting); compile
  first-wins/dominance rules (`engine.rs:415-569`, §2–3) match tests;
  `accept` reports owned prefix only, empty→0, full→0→`Pending` upstream,
  never over-allocates (`engine.rs:1013-1208`); the stream-loss path
  truncates preserve runs to remaining capacity so a non-full queue always
  makes progress (`engine.rs:932-936`); activation indices 0–6 match
  `type_index`, stream loss uses only the additive `stream_loss_*`
  counters with unique-chunk / counted-once-bytes rules
  (`engine.rs:760-802,952-998`); `publish_termination` first-wins end to
  end (handle + local + evidence, `engine.rs:658-668`);
  `poll_due_termination` arms both disconnect and finite-blackhole deadlines
  and otherwise parks on the handle (`engine.rs:1215-1257`).
- `stream.rs`: `poll_write` observes live generation before the direct path
  (`stream.rs:507-513`); direct bytes counted via `note_direct` and added
  in `summary` / `mirror_engine`; post-accept immediate drive + read pump
  present (relay never flushes mid-stream); graceful→EOF only after
  delivering live bytes and draining; `MAX_EVIDENCE_FAULTS = 128`
  truncation flag set; `BidirectionalChaosStream` serves flushed `output`
  before inner reads and requires empty `output` before downstream swaps
  (`stream.rs:967-969`); vectored writes join then delegate.
- `datagram.rs`: six kinds named by `DATAGRAM_FAULT_TYPE_NAMES`;
  `admit` returns `Consumed` / `Immediate` / `Queued` with the empty-plan
  no-alloc fast path (`datagram.rs:374-385`); immediate items sorted by
  `(ordinal, copy)` matching heap drain order; heap keyed by
  `(deadline, ordinal, copy)` with O(1)/O(log n)/O(k log n) bounds;
  overflow counted separately from configured loss; empty-plan immediate
  evidence matches queue-then-drain (`datagram.rs:1056-1119`); 14-case
  golden corpus pins exact traces (`tests/datagram_golden.rs:57-62`).
- `policy.rs`: `ArcSwap` single-load `snapshot()` is the only
  generation+plan+namespace source for stream transitions; generations start
  at 1 and increment exactly once per successful publish; `publish_expected`
  conflict returns `{expected, found}` without mutation.
- `rng.rs`: golden vectors in tests match §6 exactly; direction constants
  `0x5550` / `0x444e` unchanged; `below(0) == 0`, `bernoulli` edges at 0/1,
  threshold `p * u64::MAX`; `derive_policy_seed` sensitive to all three
  inputs; v2 schedule seed sensitive to all four inputs without run_id;
  stream-loss and datagram domains separated (chunk-seed and datagram-seed
  goldens pinned).
- `Cargo.toml`: no new deps beyond the five direct ones without ADR-level
  justification; `proptest` + `serde_json` remain dev-only.
- `docs/architecture.md` M009 list vs §3 above: wording matches
  (acceptance vs barrier at `docs/architecture.md:42-50`, burst latency,
  microtoken bucket, blackhole deadlines, prefix-only limit, inter-slice
  delay, `after == ZERO` boundary, shutdown-only slow-close,
  level-triggered first-wins termination at `docs/architecture.md:95-101`,
  drain before swap; datagram runtime at `docs/architecture.md:103-114`).
- ADRs: no HTTP/listeners in core (001), no global RNG, no immediate
  structural swap that forgets bytes, no OS-entropy chaos decisions (002).

Commands (run from the workspace root):

```sh
cargo test -p eggchaos-core --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
./scripts/check.sh
```

Minimum closure evidence per repo discipline: unit tests per fault state
machine, byte-conservation proptests, half-close/shutdown tests,
bounded-buffer/backpressure tests, RNG golden vectors (stream, v2 schedule,
stream-loss chunk, datagram), JSON round trips, datagram golden-trace corpus,
and a record of any platform/oracle evidence that could not be run (never
substitute inspection for execution).
