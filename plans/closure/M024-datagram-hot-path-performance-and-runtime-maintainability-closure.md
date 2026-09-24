# M024 — Datagram Hot-Path Performance and Runtime Maintainability — Closure

Status: closed
Exact implementation candidate: `ca46801bb5f12a9ecf9232f33d8840bd0c09afad`
Depends on: M023 (closed at `ae2ab73`)
Closure commit: (this file's commit; code candidate above)

## What M024 did

Semantics-preserving performance/maintainability pass over the ADR 003
datagram stack. No new fault kinds, no ADR 003 semantic change, no native
wire-contract change, no per-client upstream socket ownership change.

- WP1: benchmark harness now reports three baselines (direct UDP echo,
  benchmark-local bare fixed-target relay with identical socket topology but
  no `DatagramDirectionEngine`, eggchaos fixed-target with empty plans) in
  two modes — sequential RTT (1 outstanding, p50/p95) and windowed
  throughput (32 outstanding sequence-tagged datagrams, duplicates/loss
  accounted, bounded resends plus an overall deadline so a dropped candidate
  can never hang the run). The bare relay lives in the benchmark crate only.
  The M023 direct-vs-eggchaos metrics are retained for historical comparison.
- WP2: pre-change baseline on Apple M4 Pro / macOS arm64 / rustc 1.89.0
  (production tree `3c3a7ba`, new harness): direct 36,831/s p95 50 µs; bare
  21,108/s p95 80 µs; empty 20,007/s p95 82 µs; windowed
  135,357/89,091/76,257. Topology-matched medians 0.9479 (seq throughput),
  1.025 (seq p95), 0.8559 (windowed) justified freezing the new budget at
  ≥0.7 / ≤1.6 / ≥0.7 with headroom for host variance. Scheduler probes
  confirmed O(n) scan (peek 1→1374 ns depths 1→1024) and O(n log n)
  sort-on-every-check (not-ready drain 10→711 ns). Host profiler attempts:
  `xctrace` requires full Xcode (absent); `sample` produced unsymbolized
  stacks, so profiling evidence rests on the built-in timing probes plus the
  end-to-end triad, which measure the exact suspected hot paths.
- WP3: `DatagramDirectionEngine.queue` is now a min-heap (`BinaryHeap<Reverse<Candidate>>`)
  keyed by `(release_at, ingress_ordinal, copy_index)`: O(1) deadline peek,
  O(log n) insert, O(k log n) drain, no task-per-datagram. Payload and
  generation never participate in ordering. The 14-case
  `datagram_golden_traces.json` corpus is byte-for-byte unchanged.
- WP4: `admit` returns `DatagramAdmission::{Consumed, Immediate, Queued}`.
  Empty plans skip candidate allocation entirely; any all-zero-delay
  admission with an empty scheduler is accounted exactly as queue-then-drain
  (admitted, queued/high-water, emitted) and returned for immediate send
  without entering the heap. Immediate items are sorted by
  `(ordinal, copy)` to match scheduler drain order (required after cascading
  duplication, caught by test). Immediate requires an empty scheduler, which
  keeps emission observably identical to queue-then-drain. No public
  socket/runtime concept leaks into `eggchaos-core`.
- WP5: only profiled costs addressed. Steady-state bandwidth-bucket lookup
  avoids cloning the fault id; stage vectors are capacity-hinted; association
  egress accounting batches to one record update per direction per drain;
  evidence snapshots refresh only when admission/emission/error state
  changed. No `SmallVec`, pools, allocators, or new dependencies. The
  reusable-receive-buffer `Bytes` copy is retained as legitimate ownership
  transfer.
- WP6: new-client setup reserves a `Starting` slot (global + per-proxy
  capacity held) and runs UDP bind/connect without the registry lock.
  Exactly one racing creator owns setup; waiters yield until publication or
  abandonment, then observe `Active` or take over creation. Publication,
  failure abandonment, and administrative drains each release capacity
  exactly once and wake waiters; a drain that removes a `Starting` slot
  tears down the unpublished worker before any task/socket/count can leak;
  idle reaping removes a slot only if it still holds the same association.
- WP7: `runtime/datagram.rs` (1729 lines) is now `runtime/datagram/`
  (`mod.rs` 32, `model.rs` 302, `registry.rs` 570, `association.rs` 520,
  `supervisor.rs` 208, `tests.rs` 614) behind one `DatagramRuntime`
  authority and unchanged `ControlState` integration and public re-exports.
- WP8: this note; rebenchmark below; full gate suite on the exact candidate.

## Performance evidence (before → after, same host class)

Candidate `ca46801`, Apple M4 Pro, macOS arm64, rustc 1.89.0, 1200-byte
payloads, 2000 datagrams/case, 3 rounds, window 32. Raw reports:
`qualification/performance/2026-09-24-macos-arm64-m024-before.json` and
`...-m024-after.json` (42 samples + 4 scheduler probes each).

| Metric (median) | Before | After |
| --- | --- | --- |
| direct sequential dgrams/s (p95 µs) | 36,831 (50) | 39,149 (43) |
| bare relay sequential dgrams/s (p95 µs) | 21,108 (80) | 21,494 (71) |
| empty-plan sequential dgrams/s (p95 µs) | 20,007 (82) | 20,566 (79) |
| direct / bare / empty windowed dgrams/s | 135,357 / 89,091 / 76,257 | 140,911 / 82,137 / 82,731 |
| matched empty/bare seq throughput | 0.9479 | 0.9568 |
| matched empty/bare seq p95 | 1.025 | 1.1127 |
| matched empty/bare windowed | 0.8559 | 1.0072 |
| retained M023 empty/direct ratio | 0.5432 / 1.64 | 0.5253 / 1.8372 |

Budgets: matched ≥0.7 / ≤1.6 / ≥0.7 all pass; M023 floor (≥0.45, ≤2.5)
passes. Repeatable improvement in an avoidable-cost dimension (windowed
engine overhead eliminated: 0.8559 → 1.0072) with no material sequential
regression. One intermediate after-run under host load ≈21 (a concurrent
foreign `cargo xtask verify`) collapsed absolutes and failed the M023
floor; it was discarded and re-run clean — recorded here so a favorable
single sample is never cherry-picked silently.

Scheduler probes (ns; peek / not-ready drain / per-item full drain):

| Depth | Before | After |
| --- | --- | --- |
| 1 | 1.1 / 10.4 / 42.0 | 0.6 / 3.0 / 42.0 |
| 32 | 35.6 / 34.0 / 7.8 | 0.5 / 2.7 / 29.9 |
| 256 | 352.7 / 258.4 / 4.4 | 0.4 / 2.6 / 54.7 |
| 1024 | 1374.1 / 711.2 / 5.1 | 0.4 / 2.6 / 40.4 |

Per-turn costs are now flat with depth; per-item full-drain cost rose
(heap pops vs one memmove) and is negligible next to socket I/O, as the
end-to-end improvement confirms.

## Verification on exact candidate `ca46801`

- `./scripts/check.sh` (fmt, clippy `-D warnings`, workspace tests,
  doc): pass.
- `./scripts/benchmark_datagram.sh`: `datagram_budget: pass`,
  `matched_budget: pass` (summary above).
- `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh`: 8/8 targets pass.
- `./scripts/qualify_eggfetch.sh`: pass.
- Pinned oracle `TOXIPROXY_SERVER=... EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1
  ./scripts/qualify_toxiproxy_v2_12.sh`: differential pass, 50/50 vs
  toxiproxy-server 2.12.0 (checksum verified).
- `./scripts/release-smoke.sh`: pass (incl. publish-order proof).
- `cargo audit --deny warnings`, `cargo deny check advisories licenses bans
  sources`: pass.
- Hosted CI (push on `ca46801`): run 36036551214 — ubuntu failed
  solely on `cargo fmt --check` (a whitespace-only import collapse in the
  moved `datagram/tests.rs`, applied cosmetically in the closure commit
  `b2fb031`; no semantic change); macOS/Windows were cancelled as a
  consequence. Re-run on the closure commit (run 36037905729) is green on
  ubuntu/macos/windows.
- Release qualification workflow: first dispatched on `ca46801` (run
  36036561705) — 5/5 artifacts green, qualify failed at the same
  formatting gate before functional steps. Re-dispatched on the closure
  commit (run 36037910167) — qualify green (release-smoke, datagram
  benchmark incl. both budgets, fuzz 10k, pinned-oracle Toxiproxy,
  Eggfetch, artifact smoke) plus 5/5 artifacts green. The closure tree
  differs from `ca46801` only by formatting, benchmark artifact refresh,
  and planning docs.

Focused tests (all green): unchanged 14-case golden corpus; heap equal/mixed
deadline ordering; count/byte overflow after scheduler replacement;
empty-plan immediate-vs-queued evidence equality; empty-plan overflow/oversize
consumption incl. the full-scheduler bound case; 1024-depth ordered drain;
cascading-duplicate copy order; generation snapshot with queued candidates
(pre-existing); concurrent first datagrams converge to one association;
simultaneous new clients respect global/per-proxy caps with exact rejection
accounting and full capacity recovery after kills; delete-during-setup-storm
leaks no capacity; existing multi-client/multi-response/unsolicited,
idle-expiry, kill/disable/re-enable, capacity/oversize, rollback, and IPv6
tests unchanged and green.

## Invariants held

- `datagram_golden_traces.json` unchanged; stream RNG/fault semantics
  untouched.
- Loss/overflow/oversize/send-error/administrative-discard stay distinct;
  queue count/byte bounds never weakened (new bound-case test).
- Immediate emission never bypasses evidence/cancellation accounting.
- Per-client connected upstream sockets and response isolation unchanged.
- No association/task/socket detached; no registry lock held across
  network I/O after WP6.
- Native JSON/TOML/CLI/Toxiproxy contracts unchanged; `unsafe_code =
  "forbid"` holds; no new production dependencies.

## Follow-on activation

M024 closes this optimization/maintenance pass. It activates no successor:
richer datagram semantics still require a separate plan/ADR per the M024
plan's follow-on rule, and none is registered. `plans/registry.md` moves
M024 `ready` → `closed`; no other milestone changes state (none blocked).
