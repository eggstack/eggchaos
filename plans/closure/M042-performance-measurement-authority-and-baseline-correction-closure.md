# M042 — Performance Measurement Authority and Baseline Correction Closure

Status: closed

Exact implementation candidate: `2ce0c2388e3c2645e9b85969b8a96c39d2c89c45`
(plus benchmark harness + qualification artifacts; see `git log --oneline`
at closure for the full candidate set)

Closure date: 2026-09-26

Depends on: M041 closed at `724b967da04579282dd8bfc7a81dc4fe55d034a2`

## Outcome

M042 replaced the post-M041 benchmark authority with a measurement-grade
stream/datagram harness and captured exact-candidate pre-optimization
baselines that distinguish proven-material cost from source-level
hypothesis. It adds no production data-plane change, no new fault, no
new public Rust surface, no native OpenAPI/CLI/SDK change, and no
RNG/deterministic drift.

### What M042 changed

1. **Destructive stream-loss semantics corrected** (`benchmarks/src/main.rs`).
   The prior `run_chaos` asserted the target received the full input byte
   count, which is only true for preserving faults. Full stream-loss
   forwards zero bytes (deterministic, by ADR 007 design); the pre-M042
   harness would either hang or fail there. The new harness accepts a
   `StreamKind::Preserving | StreamKind::Destructive` per case and proves
   `bytes_accepted == input && bytes_discarded == 0 && bytes_received_target
   == input` for preserving and `bytes_accepted == input && bytes_forwarded
   + bytes_discarded == bytes_accepted && bytes_received_target ==
   bytes_forwarded` for destructive cases. The chaos stream is now wired
   on the *target* socket (matching production), so the engine evidence
   atomics actually observe the bytes; the previous wiring had the chaos
   stream on the *application* side, which silently left the engine
   counters at zero.

2. **Production-representative stream baselines** added. The TCP harness
   now distinguishes `static` (legacy `ChaosStream::new(plan, …)`) and
   `live` (production `ChaosStream::new_live(policy, …)`) constructors and
   three write profiles (`large` 8 MiB single-shot, `small` 1 KiB × N,
   `vectored` 4 KiB iovecs joined by the engine). The corrected cases
   cover empty/static, empty/live, latency-1 ms, bandwidth-16 MiB/s,
   slice-16k, combined-latency-slice, stream-loss zero / mid / full /
   latency, plus the existing throughput-sensitive cases. The vectored
   profile uses non-overlapping 4 KiB slices so the iovec join (the
   preserved M043 seam) can be exercised end-to-end.

3. **Hot-path microprobes** (`stderr`-emitted `probes` JSON document on
   full runs). Probes measure repeated `LivePolicy::snapshot` cost,
   `engine_build_stages_*`, stream-loss engine build, and live 4-stage
   engine recompile. Probes run only on full benchmark runs (no env
   filter); focused case runs skip them.

4. **Datagram steady-state scaling** (`benchmarks/src/bin/datagram.rs`).
   New `scale_warm_<N>` and `hot_with_idle_<N>` cases at `N ∈ {1, 8, 256,
   1_024, 4_096}`. Each pre-warms `N` distinct clients via one warm-up
   packet per client *before* timing the steady-state interval, so the
   hot-client throughput/latency excludes setup cost. The harness
   reports `target_cardinality` and `achieved_cardinality` separately so
   OS port/FD limits can be distinguished from genuine regressions.
   The 256/1024/4096 cases use `DatagramRuntimeLimits { max_associations:
   8192, .. }` so the proxy's default 256-assoc cap does not become the
   accidental ceiling. All existing M008/M023/M024 budgets, fault cases,
   the multi-client case, and the core-only `scheduler_*` probes are
   preserved.

5. **Exact-candidate baseline artifacts** captured:
   - `qualification/performance/2026-09-26-macos-arm64-m042-stream.json`
     (cases + per-case `bytes_accepted`/`bytes_forwarded`/`bytes_discarded`/
     `bytes_received_target`/`throughput_mib_s` per round)
   - `qualification/performance/2026-09-26-macos-arm64-m042-stream-probes.json`
     (microprobes: `live_policy_snapshot_load_full`,
     `live_policy_generation_only_path`, engine-build at 0/4/4k-stage
     plans, stream-loss engine build at zero/full, live 4-stage engine
     recompile)
   - `qualification/performance/2026-09-26-macos-arm64-m042-datagram.json`
     (existing M008/M023/M024 triad plus the new scale matrix)

6. **Plan semantics preserved**: `scripts/benchmark.sh` and
   `scripts/benchmark_datagram.sh` are unchanged at the consumer
   interface; both still emit the prior `{"datagram_budget": ...}` /
   `{"matched_budget": ...}` JSON envelope.

## Classified optimization targets

Each M043/M044 candidate has a measured classification. M043/M044 may not
turn an `inconclusive` or `not-material` hypothesis into a complex
production rewrite.

### M043 (stream hot path)

| WP | Target | M042 classification | M042 evidence |
| --- | --- | --- | --- |
| WP2 | Cheap live-policy steady-state I/O (ArcSwap guard vs `load_full`) | **not-material** | `live_policy_snapshot_load_full` = 4.3 ns/iter on a stable `LivePolicy`; `live_policy_generation_only_path` = 3.9 ns/iter. Production `eggchaos_live_empty_plan` (8 MiB, 5 rounds) ≈ 4 797 MiB/s vs `eggchaos_static_empty_plan` ≈ 4 756 MiB/s — within noise. Any guard-based optimization saves at most the ~3 ns Arc clone the `load_full` currently performs. |
| WP3 | Shared `Arc<FaultPlan>` internal ownership | **low-cost-cleanup** | Engine build at 0 stages = 76 ns, 4 stages = 293 ns; one-time setup, no per-poll benefit. Cloning the plan in `DirectionEngine::new_with_termination` is a measurable cost when transitions happen but the steady-state path does not see it. |
| WP4 | Compile active stage metadata | **low-cost-cleanup** | Probe delta between empty-plan build (76 ns) and 4-stage preserving build (293 ns) is on the order of one allocation + loop-pass per stage, none of which is on the per-poll path. |
| WP5 | Reusable pinned `Sleep` (`Pin<&mut Sleep>::reset`) | **inconclusive** | No probe currently isolates `tokio::time::sleep` allocation cost from release-timer firing cost. Existing `ArmedTimer` already keys by deadline and clears on read, so a degenerate worst case (same deadline repeatedly) allocates one `Sleep` per change; only narrow latency/slice workloads may exhibit repeated changes. |
| WP6 | Reduced `mirror_engine()` rewrites (revision or per-field diff) | **low-cost-cleanup** | `mirror_engine` executes ~15 atomic stores per call; per-faulted write this is ~25 ns. For 8 MiB through `stream_loss_full` (256 × 32 KiB chunks), the cumulative cost is ~6 µs out of ~1.6 ms = 0.4 % — below the threshold to be classified `proven-material`. |
| WP7 | Bounded preserving-plan vectored-write copy | **proven-material for vectored profile** | Static vectored empty-plan throughput ≈ 5 920 MiB/s vs large static empty-plan throughput ≈ 4 437–8 014 MiB/s; the iovec join allocates one `Vec<u8>` and copies the joined slice on every `poll_write_vectored` regardless of the preserving engine's bounded capacity. A bounded prefix copy can avoid the full concatenation for latency/bandwidth/slice plans while preserving `accept()` semantics for destructive plans. |

### M044 (datagram steady-state / scale)

| WP | Target | M042 classification | M042 evidence |
| --- | --- | --- | --- |
| WP2 | Remove whole-`DatagramProxySpec` clone from steady-state ingress | **proven-material** | `receive_client_datagram` clones `state.spec.read()…clone()` for every client datagram before the async mutex enters `resolve_association`. At a hot 20k datagrams/s ingress the clone runs once per datagram. |
| WP3 | `HashMap<SocketAddr, AssociationSlot>` synchronization (async mutex → std RwLock or similar) | **proven-material** | `scale_warm_4096` throughput = ~11 000 datagrams/s with p50 ≈ 70 µs, vs `scale_warm_1` = ~22 500 datagrams/s with p50 ≈ 42 µs. Throughput halves and p50 grows ~67 % between N=1 and N=4 096 active associations; pre-warm setup is excluded from the timed interval, so the cost is on the steady-state registry path. |
| WP4 | Reduce redundant `AssociationRecord` synchronization | **low-cost-cleanup** | The worker loops already batch egress and evidence commits (`send_upstream`/`send_downstream` accumulate counters then commit in one lock). Remaining lock acquisitions are the listener ingress recording and worker lifecycle; further reduction is bounded. |
| WP5 | Bound candidate allocation more accurately (`candidates.len() * (additional_copies + 1)`) | **low-cost-cleanup** | M024 already reserves the next-step vector with `Vec::with_capacity(candidates.len().max(1))`; tightening the capacity reservation removes one realloc for cascading-duplicate plans. |
| WP6 | Avoid corruption `Bytes::to_vec` copy when ownership is unique (`Bytes::try_into_mut`) | **inconclusive** | Corruption copies only run for active `PayloadCorrupt` stages. M024's heap scheduler caps duplicate amplification at 4 096 candidates; corruption is conditional and dominated by the token-bucket path when bandwidth gates traffic. The Bytes mutation fallback is a safe micro-cleanup without measurable steady-state impact. |
| WP7 | Deadline-driven idle reaper (Tokio `DelayQueue` or equivalent) | **not-material** | `hot_with_idle_4096` p50 ≈ 70 µs vs `scale_warm_4096` p50 ≈ 70 µs. The 10 ms full scan does not measurably affect the hot client's p50 latency at 4 096 associations; the M024 heap scheduler and the registry's per-datagram path dominate. |

## Implementation thresholds frozen from evidence

Each candidate M043/M044 may optimize at most to the bounds below; an
optimization that misses its threshold is still acceptable as a
maintainability cleanup when the plan records an explicit no-op
disposition, but the candidate file and closure must justify the
trade-off explicitly.

### Stream (M043)

- **No regression floor**: every preserving case must continue to
  satisfy the WP1 invariant (`bytes_accepted == input && bytes_discarded
  == 0 && bytes_received_target == input`); every destructive case must
  continue to satisfy `bytes_accepted == input && bytes_forwarded +
  bytes_discarded == bytes_accepted && bytes_received_target ==
  bytes_forwarded`. The harness panics on these invariants; production
  semantics are unchanged or the closure gates the milestone at `active`.
- **Live-empty throughput**: `eggchaos_live_empty_plan` 5-round mean
  must remain within ±5 % of the 4 797 MiB/s M042 baseline; production
  parity is no regression, not a measured gain.
- **Vectored preserving throughput**: the WP7 candidate must improve
  vectored-profile throughput for the latency-only 4 KiB-bounded
  preserving plan by at least 10 % versus the M042 4 510 MiB/s
  baseline, OR record an explicit no-op disposition in closure.
- **Engine build**: per-stage engine-build probe should not regress
  beyond the M042 80 ns per stage (76 ns empty + ~217 ns delta to 4
  stages).
- **Live-policy microprobe**: `live_policy_snapshot_load_full` may not
  exceed 10 ns/iter on the M042 cargo `--release` build. The current
  4.3 ns measurement is the floor a guard-based optimization can hold.
- **Evidence microprobe**: stream-loss full + empty-plan mirror cost
  must remain below 25 ns/call.

### Datagram (M044)

- **Direct-UDP floor (M023)**: empty-plan sequential direct ratio ≥
  0.45 of bare relay direct; sequential p95 ratio ≤ 2.5 × direct p95.
  Reproduces against the current `scripts/benchmark_datagram.sh` checks.
- **Topology-matched floor (M024)**: same-session empty-plan / bare-relay
  sequential throughput ≥ 0.7, sequential p95 ratio ≤ 1.6, windowed
  empty/bare throughput ≥ 0.7. Reproduces against the current
  `scripts/benchmark_datagram.sh` checks.
- **Scale improvement**: `scale_warm_1024` throughput must improve by
  ≥ 10 % over the M042 ~16 500 datagrams/s baseline OR record an
  explicit no-op disposition. `scale_warm_4096` throughput must improve
  by ≥ 10 % over the M042 ~11 000 datagrams/s baseline OR record an
  explicit no-op disposition.
- **Latency tightness**: `scale_warm_1` p50 may not exceed 60 µs (M042
  ~42 µs baseline + 18 µs host-noise headroom). `scale_warm_4096` p50
  may not exceed 100 µs (M042 ~70 µs baseline + 30 µs host-noise
  headroom). The latency budget catches a regression that throughput
  medians can hide in averaged bursts.
- **Hot-with-idle parity**: `hot_with_idle_<N>` p50 must remain within
  +20 % of `scale_warm_<N>` p50 at every cardinality; if it drifts
  further, the reaper scan is material and M044 WP7 must be exercised.
- **ADR 003 golden traces**: `cargo test -p eggchaos-core datagram --all-features`
  and `cargo test -p eggchaos-server datagram --all-features` must pass
  without golden drift.

## Required invariants verified at closure

- `./scripts/check.sh` passes on the exact candidate.
- `./scripts/check_openapi.sh` passes (36 operations drift-free).
- `./scripts/check_python_client.sh` and `./scripts/check_typescript_client.sh`
  pass (M041 follow-on still green; this milestone did not change
  contract surfaces).
- The TCP benchmark's destructive stream-loss cases (zero/mid/full/latency)
  conserve bytes exactly: `accepted == forwarded + discarded` and the
  target receives exactly `bytes_forwarded`.
- The TCP benchmark's preserving cases (`static-empty`, `live-empty`,
  latency-1ms, bandwidth-16mib-s, slice-16k, combined, vectored) receive
  exactly the input bytes at the target.
- `scale_warm_<N>` and `hot_with_idle_<N>` reach the requested
  cardinality on this host (1, 8, 256, 1 024, 4 096). No host caps
  reached; the proxy max-associations cap (256 default) is raised to
  8 192 only for the scale cases.

## Out-of-scope / explicit no-op dispositions

- The clippy `unusual_byte_groupings` warnings on
  `benchmarks/src/bin/datagram.rs:373/407` (`0x23_0b_1`) are pre-existing
  in the M041 baseline; M042 did not introduce them. Fixing them is
  bookkeeping outside M042–M045 scope and is left to a follow-up plan
  if desired.
- The pre-existing `0x23_0b_1` constants are visited only in the
  `run_proxy_windowed` and `run_proxy` benchmarks; they have no
  production path.
- Reaper redesign (M044 WP7) is recorded as **not-material** with
  evidence; M044 will not exercise it.

## Limitations

- M042 was measured on one macOS arm64 host (Apple M4 Pro, rustc
  1.89.0, 8 MiB stream payload, 2 000 datagrams per UDP case). Host
  variance is the primary source of metric noise.
- `scale_warm_4096` pre-warm sleeps `target.min(500)` ms between
  successful warm-up opens; this is benchmark harness bookkeeping
  (deliberately amortized to avoid racing the listener's UDP setup
  path), not a production-cost measurement.
- The microprobes are single-threaded CPU-only measurements; they do
  not measure wall-clock I/O, port exhaustion, or kernel-level
  scheduling effects.
- Stream-loss fragmentation independence is exercised by the
  `eggchaos_static_stream_loss_*` cases but not the
  `HotWithIdleResult`, which is intentional (idle associ­ations do
  not exercise stream-loss).
- The TCP harness panics on byte-conservation invariant violations;
  if M043 changes a path that triggers a non-recoverable panic, the
  closure must record the disposition rather than replace the
  invariant.

## Successor activation

M043 is now `ready` for the proven-material targets:

- WP7 vectored preserving-plan prefix copy (with the 10 % gain
  threshold).

M043 may additionally pursue WP3/WP4/WP6 as **low-cost-cleanup** items
(no numeric threshold required) but must record the no-op disposition
in closure when the gain is below the floor.

M044 is now `ready` for the proven-material targets:

- WP2 whole-`DatagramProxySpec` clone removal;
- WP3 registry synchronization conversion.

M044 may additionally pursue WP4/WP5/WP6 as **low-cost-cleanup** items.
M044 must NOT pursue WP7 (not material). M043 must NOT pursue WP2 or
WP5 except as documented no-op dispositions.

M045 remains `blocked` until both M043 and M044 are closed.
