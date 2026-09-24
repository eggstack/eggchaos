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
more than 2.5 times the direct median. The script checks both ratios. These
relative thresholds tolerate host speed changes while keeping a visible
regression limit; absolute results remain host-specific. Deliberate timer-based
fault cases are reported but excluded from the empty-plan budget.
