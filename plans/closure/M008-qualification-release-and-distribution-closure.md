# M008 Closure — Qualification, Release, and Distribution

Candidate: `645a761` (code, tests, docs, perf record). Closure/registry
follow in the same change family; planning records only.
Status: `closed`. First release candidate: v0.1.0 artifacts qualified;
tag/crates.io publication/GitHub release remain explicit owner decisions
(see "Not done here").

## M000–M008 reconciliation

| Plan | Final state | SHA | Evidence path | Deferred work |
| --- | --- | --- | --- | --- |
| M000 architecture baseline | closed | historical | `plans/000-architecture-and-scope-baseline.md` | none |
| M001 workspace bootstrap | closed | `309aeff8da9b1d91b36aecd55c190e12d56e537d` | `plans/closure/M001-…` | none |
| M002 stream faults (historical) | closed/superseded by M009 | `d85d25402af4f7c63a45ebb4ebc73b8de727d2e5` | historical closure + `plans/closure/M009-…` | none |
| M003 runtime (historical) | closed/superseded by M010 | `5dd41027948e7413b595c77f11d7a9e0b31f3785` | historical closure + `plans/closure/M010-…` | none |
| M004 control/CLI (historical) | closed/superseded by M010 | `14791042aad47dc11d57f84677dcb69ef055d690` | historical closure + `plans/closure/M010-…` | none |
| M005 live/scenario (historical) | closed/superseded by M011 | `250494fc7d408c3f86933f44437d454864519984` | historical closure + `plans/closure/M011-…` | none |
| M006 toxiproxy (historical) | closed/superseded by M012 | `e3d1d1faaf390f7d2b8b134f8650dc5a385a0110` | historical closure + `plans/closure/M012-…` | none |
| M007 eggfetch | closed, requalified by M013 | `eb52ecd2a2a28e06171bbdf96c3ef4947b8d3eb8` | `crates/eggchaos-eggfetch/tests/regression.rs` (10 tests) | none |
| M008 release gate | closed | `645a761` | this record | tag/publish/release (owner); win-msvc artifact (CI); Linux/Windows test execution (CI) |
| M009 core corrective | closed | `8c1373e90129e68e96379fc3277fade8ce087abd` | `plans/closure/M009-…` | none |
| M010 runtime corrective | closed | `3961e968e98948cfab1d0c99d3503ba1624e2e6` | `plans/closure/M010-…` | none |
| M011 live corrective | closed | `acd0883` | `plans/closure/M011-…` | none |
| M012 toxiproxy corrective | closed | `a040ed7` | `plans/closure/M012-…` | bandwidth/slicer/slow_close data-plane timing differential (recorded incomplete, not claimed) |
| M013 requalification | closed (clean) | `9904490` | `plans/closure/M013-…` | none |

## Verification evidence (candidate `645a761`, 2026-09-22, darwin/arm64)

- `cargo fmt --all -- --check`, `cargo clippy --workspace
  --all-targets --all-features -- -D warnings`,
  `cargo test --workspace --all-features` (core 43, server 37,
  toxiproxy 10, cli e2e 1, eggfetch 3+10, translation 3),
  `cargo doc --workspace --all-features --no-deps` (0 warnings): PASS.
- `cargo build --workspace --release`: PASS. MSRV 1.89.0 toolchain.
- `cargo audit --deny warnings`: exit 0. `cargo deny check advisories
  licenses bans sources`: all ok.
- `TOXIPROXY_SERVER=/tmp/oracle/toxiproxy-server
  ./scripts/qualify_toxiproxy_v2_12.sh`: differential pass, 47/47,
  oracle 2.12.0 pinned + checksummed.
- Go client (`toxiproxy/v2@v2.12.0`, go1.27.1) 13/13; Python stdlib
  13/13; `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh` pass;
  `libfuzzer plan_json` 30.4M execs/0 crashes; `./scripts/qualify_eggfetch.sh` pass.
- Performance: `qualification/performance/2026-09-22-macos-arm64-m008.json`
  (Mac16,8 M4 Pro, 64 MiB/5 rounds); empty/bare ratio 0.982. Budget
  frozen: empty-plan mean ≥ 70% of same-session bare relay; fault delay
  excluded; numbers host-specific.
- Security review: loopback-by-default admin with explicit opt-in/auth
  (`docs/control-plane.md`); bounded bodies/buffers/connections/history;
  no shell execution, no open forward proxy, no TLS interception;
  hostile-input behavior covered by validation tests + fuzz; unsafe
  census: no `unsafe` blocks anywhere; `#![deny/forbid(unsafe_code)]`
  on all library crates + CLI.
- Public API: surfaces enumerated (core ~110, server re-exports,
  adapters); 0.1.0 makes no item-level semver commitment beyond
  documented wire/config surfaces; hardening deferred to pre-1.0 and
  stated in closure (no silent freeze).
- Packaging: `eggchaos-core-0.1.0.crate` builds; publish order is core
  → eggfetch; server/toxiproxy/cli blocked on upstream `eggserve-*`
  git deps reaching crates.io (external, recorded). External consumer
  fixture (`ChaosDialer`-free core surface via path dep): pass.
- Binaries (from candidate code): macOS x86_64/aarch64, Linux
  x86_64/aarch64 (zigbuild, glibc 2.17), Windows x86_64-gnu — all with
  SHA-256 (see `dist/*.sha256`, git-ignored; reproduced by
  `.github/workflows/release.yml`). Windows-MSVC: ring build-script
  vs local zig wrapper fails; built by CI on windows-2022 instead.
  macOS artifacts smoke-tested (`release-artifact-smoke.sh`: pass).
  Linux/Windows artifacts are build-verified (`file` magic) with
  execution smoke deferred to CI runners.

## Final limitations (release notes material)

- Stream slicing/loss are user-space byte operations, not IP/TCP packet behavior.
- `reset_peer` RST is platform-qualified; termination is guaranteed, the RST/FIN distinction is not asserted.
- Toxicity clamping, zero-numeric coalescing, lowercase stream echo, loopback-ephemeral listen, socket-addr upstream + charset name requirements: see parity matrix.
- Bandwidth/slicer/slow_close data-plane timing differential is incomplete (native-tested, tolerance-classified).
- No UDP, proxy chaining, language bindings, Toxiproxy-main features.

## Not done here (owner decisions)

No `v0.1.0` tag created, nothing published to crates.io, no GitHub
release drafted — the candidate is qualified and the order is validated
(core → eggfetch; server chain awaits upstream eggserve publication).
Say the word and I will tag/publish in the documented sequence.
