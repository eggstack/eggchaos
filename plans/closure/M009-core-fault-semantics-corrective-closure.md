# M009 closure — Core Fault Semantics Corrective

Milestone: M009 (`009-core-fault-semantics-corrective.md`)
Candidate commit: `8c1373e90129e68e96379fc3277fade8ce087abd`
Implementation commits: `8c1373e90129e68e96379fc3277fade8ce087abd` (code), this closure record
Commands executed: see Verification
Platforms: macOS arm64 (developer host)
External oracle/version: none required for M009 (no Toxiproxy oracle surface)
Evidence artifacts: `cargo test -p eggchaos-core` (31 tests), workspace suite, fuzz `plan_json` 5000 runs, benchmark comparison output
Known limitations: listed below
Acceptance criteria verdict: pass — all nine audit findings repaired with tests
Registry transition: M009 `ready` -> `closed`
Next milestone activated: none (M011 still blocked on M010)

## Audit finding disposition

All nine 2026-09-22 findings are repaired:

1. `Disconnect` had no execution path — now publishes a durable termination
   request at `now + after` (`after == ZERO` means the first contract
   boundary), preserving bytes accepted before the deadline.
2. `TerminationRequest::HardReset` was defined but unconsumed — now carried
   in a durable `TerminationHandle`/`TerminationInfo` (mode + direction +
   fault id) observable by the runtime edge. M010 maps it to TCP shutdown or
   reset with truthful evidence.
3. `LimitData` reported the full caller length — now returns only the
   accepted prefix (`min(remaining, len)`), publishes graceful termination
   at zero, and fails later writes with `ConnectionAborted` after the prefix
   drains. Boundaries 1, N, N+1, and straddling buffers are tested.
4. Finite blackhole recovered after its deadline — now publishes graceful
   termination at the deadline, firing even with no further write via an
   armed deadline timer. Indefinite blackhole stays discard-only.
5. `SliceConfig::variation` was dead — now drives a deterministic symmetric
   size in `[average - variation, average + variation]` (floor one) from the
   fault-local SplitMix64-v1 stream.
6. `SliceConfig::delay` was dead — now staggers logical slices (3 slices x
   50ms completes flush at exactly +100ms, proving per-slice rather than
   per-write delay).
7. `BandwidthConfig::burst_bytes` was dead and throttling was a per-chunk
   delay approximation — now an integer fixed-point (microtoken) token
   bucket: sustained `bytes_per_second`, capacity `burst_bytes`, starts
   full, refills from monotonic Tokio-clock elapsed time, capped so long
   idle grants at most one burst.
8. `poll_write` waited for full drain before reporting acceptance — now
   reports bounded engine ownership immediately (`Ready(Ok(n))`) while
   opportunistically driving releasable bytes; `poll_flush` remains the
   delivery barrier. Multi-write latency queueing, exact-capacity
   backpressure with wakeup, and high-water accounting are tested.
9. Termination evidence was engine-internal — now a level-triggered shared
   handle (first published request wins, late waiters observe it).

Two further defects found during corrective testing were also fixed:

- Mixed-clock defect: deadlines used `std::time::Instant` (wall) while
  waits used `tokio::time::sleep` (mockable). Under paused Tokio time the
  release check and refill disagreed with the timers. All engine clocks now
  use `tokio::time::Instant`, deterministic under both paused and live time.
- Stale-timer defect: one cached `Sleep` was reused across queue heads via
  `get_or_insert_with`, so a fired timer for a saturated head released the
  next head early (flush finished at +50ms instead of +100ms in the slicer
  test). Replaced with deadline-keyed `ArmedTimer`s, one each for queue
  drain, termination deadline, and slow-close, since the old sharing also
  mixed queue and termination timers.

## Pre-1.0 API changes (documented)

- `DisconnectConfig` gains `after: Duration` (serde-defaulted, so old JSON
  parses as zero-delay). TOML `delay` on a `disconnect`/`reset_peer` fault
  now maps to `after`; Toxiproxy `reset_peer` `timeout` maps to `after`.
- `DirectionSummary`/`EngineEvidence` gain `slices`, `buffered_bytes`,
  `high_water_bytes`, `throttled_delay_ms`, `termination`, and
  `rng_version`. No payload bytes are exposed.
- `DirectionEngine::new_with_termination` shares the durable handle across
  live-policy swaps so a due termination survives a generation transition
  (first wins). Limit and RNG state restart per generation by design; the
  barrier still drains old-generation preserving bytes first, and
  already-discarded blackhole bytes stay discarded.

## Verification (exact candidate tree)

```text
cargo fmt --all -- --check                                             PASS
cargo clippy -p eggchaos-core --all-targets --all-features -- -D warnings PASS
cargo test -p eggchaos-core --all-features                             PASS (31 tests)
cargo test --workspace --all-features                                  PASS (43 tests total)
cargo check --manifest-path fuzz/Cargo.toml                            PASS
cargo test --manifest-path benchmarks/Cargo.toml                       PASS (0 tests, harness builds)
(cd fuzz && cargo fuzz run plan_json --sanitizer none -- -runs=5000)   PASS
benchmarks release run (1 MiB, 2 rounds)                               PASS, no gross no-fault regression
```

New coverage maps to every required test in the plan: multi-write latency
queueing, no serial delay multiplication (elapsed < 200ms for two writes),
backpressure-then-wake, token-bucket burst/sustained/idle-cap (unit +
paused-time), slicer determinism under scheduling noise, inter-slice delay,
finite/indefinite blackhole, limit prefix + termination + post-drain write
error, graceful/hard/delayed disconnect, shutdown with pending timers,
live transition with buffered data and with due termination, RNG
invariance, probability 0/1, preserving-combination fragmentation
conservation, and two proptest suites (preserving conservation, limit
prefix). `docs/architecture.md` records the corrected contract.

## Limitations (explicitly not M009 work)

- Listener/task lifecycle, native CRUD/reset/CLI, and TCP-edge reset
  application belong to M010, which consumes the termination signal.
- Atomic plan+generation+seed publication, scenario seed effectiveness,
  owned scenario runs, and metric expansion belong to M011.
- Toxiproxy route/default/reset/populate behavior and oracle evidence
  belong to M012.
- The benchmark harness wraps the relay-client side, so configured faults
  do not engage on the measured data path; it currently measures wrapper
  overhead only (empty plan ~= bare relay). Fault-engaging benchmark
  direction is M013/M008 follow-up.
- `LivePolicy::publish` still stores the plan before bumping the
  generation (a transient plan/generation skew visible to racing readers);
  atomic publication is M011 WP1.
- New-generation token buckets restart full (bounded one burst per
  generation); cross-generation token carryover is documented as out of
  scope.
