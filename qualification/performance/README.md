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
`./scripts/benchmark_datagram.sh`. It compares direct UDP echo with the fixed-
target runtime at 1200-byte payloads, measures delay/loss/duplicate/reorder/
corruption/bandwidth and a combined plan, and exercises eight concurrent
client associations. The script records raw samples plus candidate SHA, CPU,
OS, architecture, and Rust version when `EGGCHAOS_DATAGRAM_BENCH_OUTPUT` is
set. `EGGCHAOS_DATAGRAM_BENCH_DATAGRAMS` and
`EGGCHAOS_DATAGRAM_BENCH_ROUNDS` adjust workload size.

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
