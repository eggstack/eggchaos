# Performance qualification

Run `./scripts/benchmark.sh` on the release candidate. The harness measures a
64 KiB-buffered bare `eggress-relay`, an empty-plan eggchaos relay, latency,
bandwidth, slicing, a combined latency/slicing plan, and the Eggfetch adapter.
Set `EGGCHAOS_BENCH_BYTES` and `EGGCHAOS_BENCH_ROUNDS` to repeat a larger or
longer sample set.

Record the raw JSON together with CPU/model, OS, Rust version, release profile,
payload size, rounds, throughput, and any scheduler/power-management caveats.
The bare relay is the comparison baseline; deliberate fault delay is not
included in the no-fault overhead budget.

The sibling UDP harness is `benchmarks/src/bin/datagram.rs`; run it with
`./scripts/benchmark_datagram.sh`. It reports three baselines at 1200-byte
payloads — direct UDP echo, a benchmark-local bare fixed-target relay with
the same client -> proxy -> fixed target -> proxy -> client socket topology
but no `DatagramDirectionEngine`, and the eggchaos fixed-target runtime —
in two workload modes: sequential RTT (one datagram outstanding, for p50/p95
latency) and windowed throughput (32 outstanding sequence-tagged datagrams,
for sustained datagrams/s without serial RTT dominating). The bare relay is
benchmark-only and must not become a second production runtime. The harness
also emits core-only scheduler scaling probes (`scheduler` array: admit and
`next_deadline`/`take_ready` cost at ready-queue depths 1/32/256/1024, no
sockets involved) and measures delay/loss/duplicate/reorder/
corruption/bandwidth, a combined plan, and eight concurrent client
associations. The script records raw samples plus candidate SHA, CPU,
OS, architecture, and Rust version when `EGGCHAOS_DATAGRAM_BENCH_OUTPUT` is
set. `EGGCHAOS_DATAGRAM_BENCH_DATAGRAMS` and
`EGGCHAOS_DATAGRAM_BENCH_ROUNDS` adjust workload size.
`EGGCHAOS_DATAGRAM_BENCH_CASE` selects one case (e.g. `empty_plan_windowed`
or a `scheduler_depth_*` probe source) for focused runs.

M023's first direct-UDP baseline selected a same-session empty-plan budget of
at least 45% of direct datagrams/second median and empty-plan p95 latency no
more than 2.5 times direct p95 latency. The script checks both ratios. These
relative thresholds tolerate host speed changes while keeping a visible
regression limit; absolute results remain host-specific. Deliberate timer-based
fault cases are reported but excluded from the empty-plan budget.

Exact candidate `ae2ab733b2be199d7693e40cdc558df01ee9a9de` was measured on
2026-09-24 with rustc 1.89.0 on an Apple M4 Pro (macOS arm64), 1200-byte
payloads, 2000 logical datagrams per case, and three rounds. The direct UDP
echo median was 43,786.71 datagrams/s (p95 42 µs); the fixed-target empty-plan
median was 24,107.89 datagrams/s (p95 66 µs), a 0.5506 throughput ratio and
1.5714 p95 ratio. The checked raw 30-sample report is
`2026-09-24-macos-arm64-m023.json`. Fault cases and eight-client throughput
are in that report; scheduler timer granularity dominates the 20 µs delay and
reorder cases on this host.

M024 added the topology-matched bare-relay baseline and froze a second
budget from measured evidence: same-session empty-plan/bare-relay sequential
throughput ≥ 0.7, sequential p95 ratio ≤ 1.6, and windowed empty/bare
throughput ≥ 0.7. The pre-change production tree (`3c3a7ba`, measured with
the new harness before any hot-path change) showed matched medians of 0.9479
sequential throughput, 1.025 sequential p95, and 0.8559 windowed — the 0.7 /
1.6 thresholds sit well below observed behavior with headroom for host
variance, while the retained M023 direct floor still guards end-to-end
regressions. Pre-change scheduler probes confirmed the scaling cost being
removed: `next_deadline` peek 1 ns at depth 1 rising to 1374 ns at depth
1024 (full-queue scan), and not-ready `take_ready` 10 ns rising to 711 ns
(full-queue sort on every check). Raw reports are
`2026-09-24-macos-arm64-m024-before.json` and
`2026-09-24-macos-arm64-m024-after.json` (1200-byte payloads, 2000 datagrams
per case, 3 rounds, window 32).

The M024 candidate `ca46801bb5f12a9ecf9232f33d8840bd0c09afad` was measured
in the same host class (Apple M4 Pro, macOS arm64, rustc 1.89.0) with the
same workload. Before → after medians: direct sequential 36,831 → 39,149
datagrams/s (p95 50 → 43 µs); bare relay 21,108 → 21,494 (p95 80 → 71 µs);
empty-plan 20,007 → 20,566 (p95 82 → 79 µs); direct windowed 135,357 →
140,911; bare windowed 89,091 → 82,137; empty windowed 76,257 → 82,731.
Topology-matched ratios moved 0.9479 → 0.9568 (sequential throughput), 1.025
→ 1.1127 (sequential p95), and 0.8559 → 1.0072 (windowed throughput): a
repeatable windowed improvement with no material sequential change, and the
retained M023 floor passes (0.5253 ≥ 0.45, 1.8372 ≤ 2.5). Scheduler probes after the change are flat with depth:
`next_deadline` peek ~0.5 ns and not-ready `take_ready` ~2.7 ns at all
depths (was 1374 ns / 711 ns at depth 1024); per-item drain cost rose from
~5 ns to ~40 ns (heap pops instead of one memmove drain) and is negligible
next to socket I/O. A host sampling profiler was attempted (`xctrace`
requires full Xcode, absent; `sample` produced unsymbolized stacks), so
profiling evidence rests on these built-in timing probes plus the end-to-end
triad, which measure the exact suspected hot paths.
