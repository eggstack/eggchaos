# M036 — Deterministic Stream-Loss Core and Evidence Closure

Status: closed  
Depends on: M035 (closed), ADR 007  
Role: post-v2.12 stream-loss semantic foundation  
Candidate: see git rev below; M039 records the final combined authority.

## Scope recap

Add the deterministic userspace TCP byte-stream loss primitive defined by
ADR 007 to `eggchaos-core`, including fragmentation-independent logical
chunking, burst correlation, bounded ownership behavior, additive evidence,
and golden/property tests. M036 is intentionally core-first: native HTTP,
configuration, CLI, SDK, embed, and Toxiproxy surfaces are unchanged in
this milestone and own their handoff in M037 / M038.

## Implementation summary

### Type, constant, and validation (`crates/eggchaos-core/src/plan.rs`)

- `StreamLossConfig { loss_rate: Probability, correlation: Probability }`
  with finite `[0, 1]` validation enforced by `FaultPlan::validate`
  even for deserialized values.
- `STREAM_LOSS_GRAIN_BYTES = 32768` (`pub const`); `STREAM_LOSS_TYPE_NAME
  = "stream-loss"` (`pub const`).
- `FaultKind` gains `StreamLoss(StreamLossConfig)` (eighth variant).
- `FAULT_TYPE_NAMES` is unchanged — exactly seven legacy entries in legacy
  order; the legacy activation-array contract remains frozen.
- `FaultKind::type_name` returns the spelling directly (now covers every
  variant). `FaultKind::type_index` returns `Option<usize>`: `Some(0..6)`
  for the seven legacy faults and `None` for `StreamLoss`, which reports
  only through additive named evidence. Existing seven-slot consumers
  (`runtime/metrics.rs`, server snapshot/mirroring) keep their meaning
  by indexing only when `type_index()` is `Some`.

### RNG domain helper (`crates/eggchaos-core/src/rng.rs`)

- `derive_stream_loss_seed(run_seed, proxy, connection_key, direction,
  fault_id)` mixes a fixed domain constant into `derive_seed`. Existing
  vectors are byte-identical; only stream-loss draws change.
- Golden: `derive_stream_loss_seed(7, "proxy", 41, Upstream, "loss")`
  pins to `196256752510381490` (pinned test).

### Engine (`crates/eggchaos-core/src/engine.rs`)

- `StreamLossConnState { rng, decided: Option<(u64, bool)>, prev_dropped }`
  per active `StreamLoss` fault, keyed to absolute accepted offset.
- New `decide_stream_loss_chunk(chunk)` evaluates every active loss fault
  in plan order using fault-local state/RNG. Frozen composition rule:
  drop if any active fault selects drop; bytes counted once; chunks
  counted once across faults.
- New `accept_with_stream_loss` classifies the limit-bounded input into
  chunk-bounded runs; dropped runs resolve immediately without retained
  payload; preserve runs are truncated to remaining queue capacity (so
  the pre/post-accept drive always makes progress and never stalls with
  an empty queue and no timer armed) and routed through
  `push_preserve_run` (slicer/latency/bandwidth, survivors only).
- Blackhole remains dominant: while `blackhole_discarding` is true, the
  loss path is bypassed and `stream_loss_offset` / chunk state freeze,
  so blackholed bytes never advance loss chunk identity.
- The limit counts discarded bytes toward exact N-byte termination.
- Activation indices 0/1/3/5 engage once per `accept` in which they
  queue at least one byte; `StreamLoss` owns no legacy slot.
- Generation swap restarts `stream_loss_offset`/`stream_loss_evaluated`
  and every fault-local chunk state (ADR 007: no cross-generation
  loss-state migration).

### Stream evidence (`crates/eggchaos-core/src/stream.rs`)

- `EngineEvidence`, `DirectionSummary`, `StreamEvidence`, and
  `DirectionEvidenceSnapshot` each gain three additive fields
  (`stream_loss_chunks_evaluated`, `stream_loss_chunks_dropped`,
  `stream_loss_bytes_discarded`), all `#[serde(default)]` so older
  documents still decode.
- `ActiveFault.fault_type` now comes from `FaultKind::type_name()` so it
  reports `"stream-loss"` correctly without depending on legacy slot
  indexing.

### Downstream compile-only fixtures

- `crates/eggchaos-protocol/src/stream.rs`: temporary `unreachable!` arm
  in `FaultKindV1::from_core`. Every M036 construction path keeps the
  arm unreachable; M037 replaces it with the versioned `stream-loss`
  DTO. Validation and `into_core` are unchanged.
- `crates/eggchaos-toxiproxy/src/lib.rs`: temporary `unreachable!` arm
  in `attrs_from_kind` for `FaultKind::StreamLoss`. Strict v2.12 still
  rejects `packet_loss` as an invalid toxic; M038 adds the explicit
  profile-aware reverse mapping.
- `crates/eggchaos-experiment/src/fingerprint.rs`: deterministic
  compile arm for v2 fingerprints so adding the variant does not
  silently break scenario-v2 golden corpus for plans that never use it.

### Documentation

- `docs/architecture.md`: stream-loss paragraph in the fault-semantics
  block, framed explicitly as userspace stream-chunk loss (not IP/TCP
  packet loss).
- `architecture/core-fault-engine.md`: API inventory, evidence contract,
  per-fault semantics, write/flush contract, generation swap notes, RNG
  helpers, and checklist updated for `StreamLoss`; explicit reminders
  that `FAULT_TYPE_NAMES` stays exactly seven entries and stream loss
  uses only the additive counters.

## Tests

Added `crates/eggchaos-core/tests/stream_loss.rs` (29 tests, all green):

- Frozen types, constants, validation (`-epsilon`, `0`, `1`, `>1`,
  `NaN`/`Inf`); JSON round-trip; legacy seven-slot contract preserved.
- `loss_rate = 0` preserves every byte; `loss_rate = 1` discards
  everything with zero high-water.
- Fixed-grain boundary cases at `32767 / 32768 / 32769` bytes.
- One 96 KiB write vs equivalent `1024`-byte fragments vs byte-at-a-time
  writes yield identical survivors and counters; randomized proptest
  fragmentation invariant.
- `correlation = 0` deterministic baseline (seed namespace sensitive);
  `correlation = 1` deterministic burst continuation as a survivor
  prefix.
- Connection-level `probability = 0` never activates and
  `probability = 1` always activates.
- Adding an unrelated fault does not perturb loss decisions (loss RNG
  stream is fault-local).
- Two active stream-loss faults compose by union without double-counting.
- Composition with latency, bandwidth, slicer, limit-data (inside drop
  and inside preserve), blackhole (state freeze), graceful/hard
  disconnect, slow-close, flush after mixed traffic, shutdown with
  queued survivors, and bounded saturation that must not skip to a
  later dropped range.
- Live generation replacement drains survivors and restarts loss state
  (the same identity/config replays the same pattern post-swap).
- Pinned golden: `derive_stream_loss_seed(7, "proxy", 41, Upstream,
  "loss") = 196256752510381490`; the 96 KiB mixed trace under the
  frozen identity preserves chunks `0` and `2`, drops chunk `1`
  (accepted 98304, forwarded 65536, discarded 32768).
- Legacy evidence without `stream_loss_*` keys still decodes with the
  defaults zero; `activations` is exactly length 7.

The existing `eggchaos-core` unit suite (57 tests) and golden datagram
corpus (1 test) remain unchanged and green.

## Verification

Run on the candidate `ca527db`-line workspace (see git rev in the
registry):

```sh
cargo fmt --all -- --check
cargo clippy -p eggchaos-core --all-targets --all-features -- -D warnings
cargo test -p eggchaos-core --all-features
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
./scripts/check.sh
```

All five commands exit 0 on the exact candidate. `scripts/check.sh`
covers the full workspace gate (`cargo fmt`, `cargo clippy
--workspace --all-targets --all-features -- -D warnings`, `cargo test
--workspace --all-features`, `cargo doc --workspace --all-features
--no-deps`).

The protocol, server, CLI, and toxiproxy crates still compile and pass
their full existing suites; M036 introduces only the temporary
compile-only `unreachable!` arms documented above. There is no
behavioral change for callers that do not name `FaultKind::StreamLoss`.

## Acceptance checklist (per M036)

- [x] `StreamLoss` exists as a protocol-neutral core primitive
  (`crates/eggchaos-core/src/plan.rs`).
- [x] Logical loss decisions are independent of caller write
  fragmentation (`stream_loss.rs`: identical-stream tests, randomized
  proptest).
- [x] 32 KiB grain and burst-correlation contract are golden-tested
  (`stream_loss.rs`: golden chunk seed, grain boundary, correlation
  tests).
- [x] Dropped bytes are never retained in an unbounded buffer
  (`loss_rate_one_discards_everything_without_retaining_payload`,
  `loss_inside_limit_boundary_preserves_exact_prefix`,
  `loss_plus_blackhole_keeps_blackhole_dominant_and_freezes_loss_state`).
- [x] Mixed preserve/drop writes obey Tokio prefix ownership
  (`saturation_stops_at_the_first_unowned_preserving_prefix`).
- [x] All existing fault interactions have explicit tests
  (`loss_plus_*`).
- [x] Legacy seven-slot activation evidence is byte/shape stable
  (`stream_loss_names_and_grain_are_frozen`,
  `legacy_evidence_stays_decodable_and_seven_slots_stable`, existing
  `activations_count_per_type_deterministically` test unchanged).
- [x] Additive stream-loss evidence reconciles with aggregate discard
  accounting (`assert_accounting` + the per-test equality checks).
- [x] Existing stream/datagram RNG golden corpora remain unchanged
  (golden vectors test in `rng.rs` passes unchanged).
- [x] No native HTTP/CLI/Toxiproxy compatibility claim is made
  prematurely (the only protocol/toxiproxy touch points are temporary
  compile-only `unreachable!` arms).
- [x] All required tests pass on one exact candidate (see
  Verification).
- [x] Closure note records candidate and any residual limitations (this
  document).

## Stop/rejection review

- Loss decisions depend on no source of fragmentation: `derive_seed` +
  `derive_stream_loss_seed` are derived only from `(run_seed, proxy,
  connection_key, direction, fault_id)`, and chunk visits are absolute
  offset keyed.
- No IP/TCP packet loss terminology in user-facing docs; the
  `STREAM_LOSS_TYPE_NAME` and OpenAPI description (added in M037) keep
  the spelling `stream-loss`.
- `DatagramFaultKind::Loss` is reused nowhere on stream traffic.
- Legacy activation arrays are unchanged in length, order, and index
  meaning.
- Dropped payloads are not retained (`accept_with_stream_loss` resolves
  dropped runs without copying into the queue).
- No preserving byte is skipped ahead of a later dropped byte
  (`accept_with_stream_loss` honors the `accepted_cap` limit and stops
  at the first preserving byte the bound cannot own).
- Existing limit/flush/shutdown semantics are preserved (golden tests +
  existing core suite).
- The implementation is a heterogeneous `DirectionEngine` state machine,
  not a second wrapper type.

## Follow-on activation

A clean M036 closure makes **M037 ready**. M037 must expose the already-
proven core primitive through native v1 / config / CLI / Scenario /
OpenAPI / Python / TypeScript / embed / Python-native surfaces without
creating a second semantic authority. M037's entry criterion (the exact
`StreamLossConfig` shape, fixed grain, evidence field names, multi-loss
composition rule, and golden decision vectors) is met by this M036
candidate.

## Additive M040 corrective reference (2026-09-26)

This historical M036 implementation record is preserved. Its implementation
commit is `36ddf1a4ba78879938ca65c68193565355bc9be4`; final ADR 007 corrective
qualification and repository authority are recorded in
`plans/closure/M040-post-v2-12-stream-loss-corrective-requalification-closure.md`.
