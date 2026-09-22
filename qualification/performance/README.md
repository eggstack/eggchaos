# Performance qualification

Run `./scripts/benchmark.sh` on the release candidate. The harness measures a
64 KiB-buffered bare `eggress-relay`, an empty-plan eggchaos relay, and a
representative 1 ms latency plan. Set `EGGCHAOS_BENCH_BYTES` and
`EGGCHAOS_BENCH_ROUNDS` to repeat a larger or longer sample set.

Record the raw JSON together with CPU/model, OS, Rust version, release profile,
payload size, rounds, throughput, and any scheduler/power-management caveats.
The bare relay is the comparison baseline; deliberate fault delay is not
included in the no-fault overhead budget.
