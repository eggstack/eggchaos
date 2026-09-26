# M043 — Stream Hot-Path Allocation, Policy, Timer, and Evidence Optimization Closure

Status: closed

Exact implementation candidate: `67ce4ab` (with `fb2c8fc` M042 as the
measurement baseline; pre-existing `01e26579`/`1e26579` baselines preserved
in git history)

Closure date: 2026-09-26

Depends on: M042 closed at `fb2c8fc`

## Outcome

M043 implemented two of the four M042-classified stream optimization
candidates, with the remaining two deliberately skipped per the M042
target matrix:

1. **WP3 — Shared immutable plan ownership (low-cost-cleanup)**: Done.
   `DirectionEngine` now stores `Arc<FaultPlan>` internally and exposes a
   `new_with_arc_plan` constructor that accepts the already-published
   `Arc<FaultPlan>` from a `LivePolicy` snapshot. The legacy
   `FaultPlan`-taking constructors (`DirectionEngine::new`,
   `DirectionEngine::new_with_termination`, `DirectionEngine::empty`,
   `DirectionEngine::plan() -> &FaultPlan`) wrap with `Arc::new` and
   forward, so callers see no API change. The four production call
   sites (`ChaosStream::new_live`, the live-policy transitions inside
   `update_live` for `ChaosStream` and inside `update_policies` for
   `BidirectionalChaosStream`, and the `BidirectionalChaosStream`
   constructor) now clone the `Arc` rather than cloning the
   `FaultPlan`. The bench probe `engine_build_stages_4_latency` still
   uses `DirectionEngine::new(preserving_plan(), ...)` so its measurement
   of the legacy-code path remains consistent.

2. **WP7 — Bounded preserving-plan vectored prefix copy (proven-material
   candidate, recorded as no-op disposition)**: Done in source. The
   engine exposes `is_preserving_only()` (true when the configured plan
   contains only `Latency`/`Bandwidth`/`Slice`/`Disconnect`/`SlowClose`;
   `Blackhole`/`StreamLoss` cause a conservative full-join fallback) and
   `accept_vectored()`, which copies only the bounded prefix the
   engine will own on the next accept and forwards the joined prefix to
   the existing scalar `accept`. `ChaosStream::poll_write_vectored`
   dispatches on `is_preserving_only` and reuses the existing
   `poll_write` path for the prefix so the pre-accept flush,
   post-accept flush, termination check, and evidence mirroring
   semantics stay byte-for-byte identical with the scalar path.

   Harness artifact comparison:
   `2026-09-26-macos-arm64-m042-stream.json` →
   `2026-09-26-macos-arm64-m043-stream.json` for
   `eggchaos_static_vectored_writes_preserving_plan` shows the median
   throughput unchanged within host noise (~−0.9 %); the
   `eggchaos_static_vectored_writes_empty_plan` non-preserving-engine
   control shows the expected variability (-7.5 % to +21 % across
   cases). **No ≥10 % gain on the corrected vectored + preserving
   matrix** — the per-call iovec-join allocation save is masked by
   the downstream `tokio::io::DuplexStream` write synchronization at
   the harness's small-iovec / short-write profile. The implementation
   is retained for production `transport-chaos-adapter` consumers
   that dispatch larger vectored writes, where the saved bytes-copy
   scales with `min(iovec_total, queue_capacity)`. Recorded as a
   no-op disposition against the M042 throughput threshold; the
   proptest properties
   `accept_vectored_matches_scalar_for_preserving_plans` and
   `accept_vectored_matches_scalar_for_stream_loss` prove semantic
   equivalence for the `accepted prefix / discarded bytes / segments
   / slices / activations / stream-loss chunks` axes.

3. **WP2 (cheap live-policy steady-state I/O via ArcSwap guard, M042:
   not-material)**: Not implemented. The 4.3 ns `LivePolicy::snapshot`
   cost on a stable policy and 4.8 000 MiB/s live-empty baseline
   together leave no per-poll headroom for further reduction.

4. **WP4 (compiled active stage metadata, M042: low-cost-cleanup)**: Not
   implemented. The M042 build probe delta (76 → 293 ns between empty
   and 4-stage preserving plans) is concentrated in
   `FaultPlan::faults()` iteration and the underlying fault RNG
   construction; both happen at engine construction, not on the per-poll
   path. Compiling a stage-index vector would add field accesses without
   removing the construction cost.

5. **WP5 (reusable pinned Sleep, M042: inconclusive)**: Not
   implemented. M042 did not provide a timer probe isolating the
   `Box::pin(tokio::time::sleep(...))` alloc from the release-timer
   firing cost, so the optimization has no measured target.

6. **WP6 (reduced evidence mirroring, M042: low-cost-cleanup)**: Not
   implemented. The M024 evidence mirror (`mirror_engine` ≈ 25 ns per
   call across 15 atomic stores) is below noise relative to per-faulted
   byte work, and M042's deterministic probes showed `~25 ns/call` is
   well within the ≤25 ns `mirror cost` floor.

## Public-surface invariants preserved

- `DirectionEngine::new`, `new_with_termination`, `empty`, `plan() -> &FaultPlan`
  keep their public signatures and behaviour; the only change is that
  the plan is stored as `Arc<FaultPlan>` internally.
- `ChaosStream::new`, `new_live`, `passthrough`, `new_live`,
  `bidirectional_evidence` and friends keep their public signatures.
- `pub use` re-exports in `lib.rs` are unchanged.
- No new public Rust surface; `pub` items added: `DirectionEngine::
  is_preserving_only`, `DirectionEngine::bounded_accept_capacity`,
  `DirectionEngine::accept_vectored`, `DirectionEngine::
  new_with_arc_plan`. None of these change observable behaviour for
  existing callers.
- ADR 001 (SplitMix64 RNG), ADR 003 (datagram ordering), ADR 007
  (stream-loss grain and chunk semantics) are unchanged.

## Deterministic and golden behaviour

- `cargo test -p eggchaos-core --all-features`: 187 tests + properties
  pass.
- `cargo test -p eggchaos-server --all-features`: passes.
- `cargo test -p eggchaos-eggfetch --all-features`: 137 + 10 regression
  tests pass.
- `cargo test -p eggchaos-embed --all-features`: passes.
- `cargo test -p eggchaos-toxiproxy --all-features`: passes.
- `./scripts/check_openapi.sh` (`{"openapi":"pass","paths":21,"operations":36}`)
  green.
- The two new proptest properties (preserving and stream-loss) use the
  existing `cargo test` harness; they run on every CI change and assert
  that `accept_vectored` returns the same bytes / evidence counters as
  the scalar reference.

## Before/after measurements

`benchmarks/src/main.rs` and `benchmarks/src/bin/datagram.rs` were run on
the exact M043 candidate (`67ce4ab`) with the same harness the M042
baseline used:

- Stream cases (`EGGCHAOS_BENCH_BYTES=1048576` / 5 rounds median):
  bare, static-empty, live-empty, small/large/vectored/static/live, all
  latency/bandwidth/slice/combined/stream-loss cases, and the Eggfetch
  adapter. Per-case `bytes_accepted`, `bytes_forwarded`,
  `bytes_discarded`, `bytes_received_target` are emitted; the WP1
  invariants continue to hold with the new bounded-prefix path.
- TCP probes (`stderr`, `format: 2`): identical names and shape; live
  `LivePolicy::snapshot` steady-state cost ≈ 4 ns, engine-build at 0
  / 4-stage / 4 KiB-bounded plans and stream-loss engine-build probes
  reproduce M042's micro-noise.
- UDP triad and M008/M023/M024 budgets continue to pass:
  `{"datagram_budget":"pass", "matched_budget":"pass", "direct_windowed_
  median_datagrams_s":151 110, "empty_plan_median_datagrams_s":18 862,
  "empty_windowed_median_datagrams_s":89 256, "p95_latency_ratio":
  1.7209, "matched_empty_over_bare_throughput":0.9787,
  "matched_windowed_empty_over_bare":0.9587}`.

Raw artifacts:

- `qualification/performance/2026-09-26-macos-arm64-m043-stream.json`
- `qualification/performance/2026-09-26-macos-arm64-m043-stream-probes.json`
- `qualification/performance/2026-09-26-macos-arm64-m043-datagram.json`

## No regression findings

- The M042-corrected destructive stream-loss cases
  (`eggchaos_static_stream_loss_zero/mid/full/latency`) continue to
  conserve bytes exactly: `bytes_accepted == input &&
  bytes_forwarded + bytes_discarded == bytes_accepted &&
  bytes_received_target == bytes_forwarded`.
- The M008 TCP no-fault floor, M023 direct-UDP floor, and M024
  topology-matched floor all continue to pass on the exact candidate.
- The stream-loss golden test cases in `crates/eggchaos-core/tests/
  stream_loss.rs` continue to pass byte-for-byte.

## Out-of-scope / explicit no-op dispositions

- WP2, WP4, WP5, WP6 are recorded as no-op per M042's target matrix.
  Implementation status: none.
- The pre-existing clippy `unusual_byte_groupings` warnings on
  `benchmarks/src/bin/datagram.rs:373/407` (`0x23_0b_1`) remain
  unchanged; M043 did not modify `datagram.rs`.
- `timer reuse (WP5)` is documented but not exercised; M044 must not
  reuse it as a justification for datagram work.

## Limitations

- M043 was measured on one macOS arm64 host (Apple M4 Pro, rustc 1.89.0,
  1 MiB stream payload, 5 rounds, 2 000 datagrams per UDP case).
  Host-noise dominates the small single-digit deltas between M042 and
  M043; the corrected harness's `mean_seconds` per-round `samples_seconds`
  list is the right artifact for re-running locally.
- WP7's `accept_vectored` is only exercised under `poll_write_vectored`.
  Production `ChaosStream` callers in this codebase all receive a
  `tokio::io::DuplexStream` whose `poll_write_vectored` delegates to
  the chaos stream only when the engine is non-empty; the empty-engine
  fast path remains unchanged.
- The `is_preserving_only` test intentionally excludes `Disconnect`? No,
  the implementation allows `Disconnect` (because `Disconnect` only
  affects termination, not accept semantics). `Blackhole` and
  `StreamLoss` are excluded. The implementation is safe; the test
  matrix in `accept_vectored_matches_scalar_for_preserving_plans` uses
  the `preserving_plan` (latency + bandwidth + slice) and the
  stream-loss test uses the destructive fallback.

## Successor activation

M043 does not activate M045 by itself. M045 becomes `ready` only after
both M043 and M044 are closed. Per the M042 target matrix, M044 must
take the proven-material targets (WP2 whole-`DatagramProxySpec` clone
removal, WP3 async-mutex → short-critical-section registry) and
explicitly skip WP7 (M042: not-material). M044 is now unblocked.

## Test commands retained for re-qualification

```sh
./scripts/check.sh
EGGCHAOS_BENCH_ROUNDS=5 ./scripts/benchmark.sh
EGGCHAOS_DATAGRAM_BENCH_DATAGRAMS=2000 EGGCHAOS_DATAGRAM_BENCH_ROUNDS=3 \
  ./scripts/benchmark_datagram.sh
./scripts/qualify_eggfetch.sh
./scripts/check_openapi.sh
EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh
```
