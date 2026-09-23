# M017 Closure — Native Control Contract and Operator-Surface Consolidation

Verdict: **closed**

Implementation candidate: `58c43455c5a2a229b6e2414a816bf90a94194e86` (M017 implementation; planning closure recorded in the following commit).

## Delivered

- Added versioned native DTOs for proxy/fault mutations and views, scenarios, and runtime configuration. Native fault kinds use explicit kebab-case tags and integer nanosecond durations; exact fixtures cover every fault kind and create/patch/view consistency.
- Routed TOML compilation and CLI fault requests through the same typed native fault vocabulary. Added bounded runtime settings and omitted-value tests proving prior defaults remain intact.
- Normalized proxy timeout naming to `connect_timeout_ms`, added CLI scenario apply/get/cancel, history, and metrics commands, and bounded scenario file input to 1 MiB.
- Documented the native API/config migration and operator commands. `eggchaos-core` remained unchanged in responsibility; mutations still go through `ControlState` over HTTP from the CLI.

Deliberately retained config parser aliases: `limit_data`/`limit-data`, `slow_close`/`slow-close`, `slicer`/`slice`, `timeout`, `reset_peer`, and canonical spellings. They are parser-edge aliases only; native HTTP DTO tags remain canonical. See `native.rs` exact fixtures and config tests for the contract/default evidence.

## Verification

On macOS 26 / Darwin 25.6 ARM64 with the repository-pinned Rust 1.89.0 toolchain:

- `cargo fmt --all` — passed.
- `RUST_TEST_THREADS=1 ./scripts/check.sh` — passed: format check, workspace all-target/all-feature Clippy with warnings denied, workspace all-feature tests, and docs.
- The gate covered 43 core tests, 3 Eggfetch unit tests plus 10 regressions, 49 server tests, 10 Toxiproxy unit tests, the CLI E2E tests, and doc tests.
- `./scripts/qualify_toxiproxy_v2_12.sh` — translation tests passed, but reported `oracle: unavailable` and `differential: incomplete`. This is not treated as a differential pass. The plan's local acceptance excludes requiring a provisioned external oracle; M019 explicitly owns mandatory strict oracle acquisition and exact candidate differential qualification.

## Limitations and successor

No unresolved M017 implementation issue was found. The pinned Toxiproxy v2.12.0 differential remains incomplete evidence and is carried as a release-blocking M019 obligation, not silently waived. M018 is unblocked and activated to modularize the runtime and audit dependencies. M019 remains blocked on M018 and must complete the oracle-backed requalification before any tag or release.
