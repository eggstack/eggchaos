# M023 closure — Datagram Qualification, Performance, and Release Hardening

Date: 2026-09-24  
Verdict: **clean**  
Exact implementation candidate: `ae2ab733b2be199d7693e40cdc558df01ee9a9de`  
Candidate branch: `main`

## Scope and outcome

M023 qualified M020–M022 as the post-release UDP/datagram tranche without changing M019's historical v0.1.0 qualification authority. The candidate adds a frozen 14-case exact datagram trace fixture, state/bounds fuzz reconciliation, cross-platform IPv6 and UDP runtime evidence, a measured direct-UDP versus empty-plan fixed-target budget, and release/security/distribution gates. Documentation and planning state were reconciled as part of this closure.

No unresolved medium-or-higher correctness, security, or release findings remain. No successor plan is registered or automatically activated. Further datagram features require separate planning and an ADR where semantics change.

## Exact-candidate evidence

All implementation evidence below is for candidate `ae2ab733b2be199d7693e40cdc558df01ee9a9de`.

- `RUST_TEST_THREADS=4 ./scripts/check.sh` passed locally: formatting, workspace Clippy, all-feature workspace tests, and docs. The four-thread limit is required on this macOS host because its descriptor limit is 256; unconstrained test concurrency can hit `EMFILE` in socket-heavy tests.
- `RUST_TEST_THREADS=4 ./scripts/release-smoke.sh` passed locally, including release checks, dependency/security checks, package verification, publish-order proof, and the artifact smoke with UDP proxy listing.
- `RUST_TEST_THREADS=4 ./scripts/qualify_eggfetch.sh` passed locally.
- Strict pinned Toxiproxy qualification passed locally with `TOXIPROXY_SERVER` set to checksum-verified Toxiproxy v2.12.0 and `EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1`; differential result: 50 passed, 0 failed.
- `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh` passed locally for all eight targets, including datagram transitions, datagram plans, datagram native configuration/control DTOs, and datagram evidence. No crashes were found.
- `./scripts/benchmark_datagram.sh` passed its measured budget. The frozen same-session criteria are empty-plan throughput at least 45% of direct UDP median and empty-plan p95 latency no more than 2.5 times direct UDP p95. The raw 30-sample macOS arm64 report is [qualification/performance/2026-09-24-macos-arm64-m023.json](../../qualification/performance/2026-09-24-macos-arm64-m023.json). On Apple M4 Pro / macOS / rustc 1.89.0, direct UDP median was 43,786.71 datagrams/s with p95 42 µs; fixed-target empty-plan median was 24,107.89 datagrams/s with p95 66 µs. Ratios were 0.5506 and 1.5714, respectively.
- GitHub [CI run 36020857078](https://github.com/eggstack/eggchaos/actions/runs/36020857078) completed successfully on Ubuntu, macOS, and Windows. Each host ran the full workspace test suite, including UDP runtime tests; each IPv6 UDP loopback test printed `PASS` and passed. Clippy, formatting, docs, audit, and deny checks passed on all three hosts.
- GitHub [release qualification run 36020888407](https://github.com/eggstack/eggchaos/actions/runs/36020888407) completed successfully. It passed release-smoke, the datagram benchmark budget, 10,000-run fuzz qualification, strict pinned Toxiproxy v2.12.0 differential (50/50), Eggfetch qualification, and release artifact smoke. All five release build targets passed: Linux x86_64/aarch64, macOS x86_64/aarch64, and Windows x86_64.
- The frozen fixture `crates/eggchaos-core/tests/fixtures/datagram_golden_traces.json` is exercised by `crates/eggchaos-core/tests/datagram_golden.rs`; it records deterministic loss, copy, operation ordering, corruption, delay/reorder, generation coexistence, bandwidth, equal-deadline, and overflow outcomes.

## Limitations and evidence boundaries

- The throughput/latency figures are a host-specific baseline; the enforced budget uses same-run relative ratios. Timer-based fault cases are reported separately and are not included in the no-fault threshold.
- IPv6 UDP loopback was available and passed on all three CI hosts; no capability-based skip was needed.
- Toxiproxy v2.12 remains a TCP/stream compatibility target. Its differential run validates that the UDP tranche did not regress the existing adapter.
- This closure does not amend M019, the historical v0.1.0 candidate, or its release evidence.

## Successor activation

M020, M021, M022, and M023 are closed. No M024 is registered. Any future datagram work must add a complete numbered plan to the registry before implementation; semantics-changing work must also update or add an ADR.
