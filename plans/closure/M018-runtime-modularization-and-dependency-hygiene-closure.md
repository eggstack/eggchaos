# M018 Closure — Runtime Modularization and Dependency Hygiene

Verdict: **closed**

Candidate: `7e33d03e6adf9d13a4407bcb2b5a4e241dd38d09`.

## Delivered and census

The former 4,768-line `runtime.rs` is now `runtime/` with 551 lines in the composition/re-export module and cohesive implementation modules: `control.rs` (1,177 lines), `connection.rs` (546), `supervisor.rs` (145), `transport.rs` (162), `metrics.rs` (126), and `model.rs` (327). The 1,762-line runtime regression suite moved to `tests.rs`. `RuntimeInner` remains a single store, and one `ControlState` authority remains. `lib.rs` root exports and public API remain unchanged.

Normal direct dependencies before: 18 entries, including `bytes`, `http`, `http-body-util`, `hyper`, `hyper-util`, and `prometheus-client`. After: 12 entries. Removed those six after source and all-target census found no direct use. `cargo tree -p eggchaos-server --locked --edges normal --depth 1` confirms the new direct list; HTTP crates still appear transitively through `eggserve-server` or Eggfetch where required. No dependency was moved to dev dependencies.

## Verification on exact candidate

Host: macOS 26 / Darwin 25.6 ARM64; pinned Rust 1.89.0.

- `RUST_TEST_THREADS=1 ./scripts/check.sh` — passed on this candidate: format, workspace all-target/all-feature Clippy (`-D warnings`), all-feature workspace tests, and docs. Includes 49 server tests and 43 core tests.
- `cargo audit --deny warnings` — passed.
- `cargo deny check advisories licenses bans sources` — passed; only existing duplicate-version warnings were emitted.
- `RUST_TEST_THREADS=1 ./scripts/qualify_eggfetch.sh` — passed, including 3 Eggfetch unit tests, 10 regressions, and 49 server tests.
- The initial default-parallel `qualify_eggfetch.sh` run exceeded the host open-file limit in two existing listener tests; the serial rerun passed. The full gate was also run serially for the same constrained host.

No cross-OS hosted CI was run as part of M018; M019 owns exact-candidate ordinary CI and release requalification.

## Successor

No unresolved structural or dependency issue blocks the successor. M019 is unblocked and active. Toxiproxy's pinned oracle remains unavailable locally and is an explicit M019 release-blocking obligation; it is not represented as passed here.
