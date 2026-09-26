# Tooling, release, and repo governance

This is a deep dive under [Eggchaos architecture overview](overview.md).
It covers the reproducible command surface (`scripts/`), CI/release
workflows (`.github/workflows/`), dependency/supply-chain policy
(`Cargo.toml`, `deny.toml`, `rust-toolchain.toml`), shipped artifacts
(`dist/`), and the `plans/` governance authority that gates any release.

Pre-release `0.1.0`. Milestones M000–M041 plus M008 are closed
(`plans/registry.md` last reconciled 2026-09-26, no successor). M019
closed at `ca527db` and remains the final pre-tag release-candidate
authority; later tranches do not rewrite it. M041 closed at
`724b967da04579282dd8bfc7a81dc4fe55d034a2` (hosted run `36219464594`,
13/13 jobs) and is the latest ADR 007 repository-level authority for
the narrow stream-loss metrics/tooling/closure corrective. Complete:
ADR 003 datagram tranche (M020–M023 plus M024 performance and M025
setup/closure hygiene), ADR 004 schedule tranche (M026–M028), ADR 005
integration-boundary tranche (M029–M031), ADR 006 cross-language
tranche (M032–M034 plus corrective M035), ADR 007 post-v2.12
tranche (M036–M039 plus correctives M040–M041). Strict Toxiproxy
v2.12 remains frozen/default. Tagging, crates.io publication, and
GitHub release creation remain explicit owner actions
(`plans/registry.md`, `plans/README.md`,
`plans/019-qualification-expansion-and-final-corrective-requalification.md`).

## 1. Scripts catalog (`scripts/`)

All scripts are POSIX `sh` with `set -eu` (except
`scripts/sync_sdk_contract.py`, which is `python3`). They are the
canonical commands; CI and the release workflow invoke them rather
than re-implementing their steps.

| Script | What it runs | When to run it |
| --- | --- | --- |
| `scripts/check.sh` | `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace --all-features`; `cargo doc --workspace --all-features --no-deps`. Note: no `audit`/`deny` here — those live in CI and `release-smoke.sh`. | Every local change before push; fastest full local gate. |
| `scripts/benchmark.sh` | `cargo fmt --manifest-path benchmarks/Cargo.toml -- --check`; `cargo run --manifest-path benchmarks/Cargo.toml --release --quiet --bin eggchaos-benchmarks`. | No-fault throughput/latency vs bare `eggress-relay` (see `benchmarks/`, `qualification/performance/`). Do not invent budgets before measurement (`plans/roadmap.md` §12). |
| `scripts/benchmark_datagram.sh` | Runs the direct UDP echo / benchmark-local bare fixed-target relay / fixed-target benchmark in release mode (`--bin datagram`), annotates the JSON report with `cpu_model`/`rustc`/`candidate_sha` (or writes to `${EGGCHAOS_DATAGRAM_BENCH_OUTPUT:-}` temp file), then checks the measured no-fault ratios. | Datagram performance qualification; the retained M023 budget is ≥45% of same-session direct datagrams/s and ≤2.5× direct p95 latency, plus the M024 topology-matched budget (empty/bare ≥0.7× sequential throughput, ≤1.6× sequential p95, ≥0.7× windowed throughput). Also runs in the release `qualify` lane. |
| `scripts/qualify_eggfetch.sh` | `cargo test -p eggchaos-eggfetch --all-features`; `cargo test -p eggchaos-server --all-features`. | After any `eggchaos-eggfetch` / server / `eggfetch-core` profile change; part of the release qualify lane. |
| `scripts/qualify_fuzz.sh` | Runs 9 targets — `plan_json`, `datagram_plan_json`, `datagram_transitions`, `native_config`, `native_control_json`, `fault_evidence_json`, `policy_transitions`, `toxiproxy_attributes`, `scenario_v2` — with `cargo fuzz --sanitizer none`, each for `${EGGCHAOS_FUZZ_RUNS:-10000}` runs; prints per-target `{"fuzz":"pass",...}` plus `{"fuzz":"pass","targets":9}`. | Bounded parser/state-machine soak before release. Release workflow pins 10,000 runs per target. |
| `scripts/fetch_toxiproxy_v2_12.sh` | Downloads the official v2.12.0 server asset for Linux/Darwin x86_64/ARM64, verifies the pinned SHA-256 from `toxiproxy_v2_12_expected_sha256.sh`, checks `-version` contains `version 2.12.0`, then prints the binary path (default under `${TMPDIR:-/tmp}/eggchaos-toxiproxy-v2.12.0/`). | Before strict differential qualification; release workflow provisions this explicitly and never trusts a `PATH` binary. |
| `scripts/toxiproxy_v2_12_expected_sha256.sh` | Prints the pinned per-host SHA-256 for the v2.12.0 oracle (Linux x86_64 `556d8911…48302fc`, Linux aarch64 `53e770c1…701cf077c`, Darwin ARM64 `aa299966…95d15`, Darwin x86_64 `9625bba4…1ef8afe`); exits 2 on unsupported hosts. | Single checksum authority consumed by the fetcher and both strict qualifiers. |
| `scripts/qualify_toxiproxy_v2_12.sh` | Always runs translation tests. Developer mode can report missing/unverified oracle as `incomplete`; `EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1` requires an explicit `TOXIPROXY_SERVER`, supported-host pinned checksum, version 2.12.0, and a clean `DIFFERENTIAL_SUMMARY` with `"failed":0`. | Toxiproxy parity work and mandatory release differential gate. The release workflow fetches the oracle then enables strict mode. Current strict corpus is 50/50 vs pinned v2.12.0 (M041). |
| `scripts/fetch_toxiproxy_post_v2_12.sh` | Pinned post-v2.12 oracle: fetches the exact upstream commit `40f7fd31bee529d824116bd2a11a9e3425e904ec` source archive (pinned SHA-256 `26351cc7…042b13d`), builds `cmd/server` with recorded Go toolchain `${EGGCHAOS_POST_V2_12_GO_TOOLCHAIN:-go1.23.0}` (asserts `GOTOOLCHAIN` identity and `toxiproxy-server version` output). M041 stdout contract: default (no flag) and `--path-only [DEST]` print exactly one executable path on stdout (shell substitution `TOXIPROXY_POST_V2_12_SERVER="$(...)"`); `--json [DEST]` prints one JSON metadata record (`requested_toolchain`, `resolved_go_version`, `resolved_gotoolchain`, `source_commit`, `source_sha256`, `oracle_path`, `oracle_version`); `--help` exits 0 with usage on stderr; unknown flags exit 2. Diagnostics always go to stderr; path and JSON are never mixed on one stdout line. | Before post-v2.12 snapshot qualification. The qualifier itself uses `--path-only` internally so the contract is unambiguous. |
| `scripts/qualify_toxiproxy_post_v2_12.sh` | Opt-in post-v2.12 gate: reaps stale snapshot oracles, runs `eggchaos-toxiproxy` unit tests, resolves the oracle via explicit `fetch_toxiproxy_post_v2_12.sh --path-only`, then runs `--test post_v212_differential` with `TOXIPROXY_POST_V2_12_SERVER`, `TOXIPROXY_POST_V2_12_COMMIT=40f7fd31…`, and `EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1`. Mandatory mode (`EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1`) fails closed on missing oracle; developer mode reports `differential:incomplete` (exit 0). | Post-v2.12 `packet_loss` profile work. Strict v2.12 stays on its own pinned oracle. |
| `scripts/check_openapi.sh` | `cargo test -p eggchaos-protocol --all-features` (OpenAPI drift + golden fixtures) + `cargo test -p eggchaos-server --all-features --test native_route_inventory` (live 36-operation inventory proof) + a YAML shape assertion (`openapi == 3.0.3`, 21 paths, 36 operations). | Native contract gate after any protocol/server/OpenAPI change; part of the release qualify lane via language-clients CI. |
| `scripts/sync_sdk_contract.py` | Derives SDK artifacts from `api/openapi/eggchaos-v1.yaml`: `bindings/_contract/operations.json` + `bindings/python-client/eggchaos_client/_generated.py` + `bindings/typescript-client/src/generated.ts` (deterministic sorted output; 36 operations + stream/datagram/scenario tag unions). Prints `{"sync":"pass","operations":36,...}`. | Before SDK checks; `check_python_client.sh` / `check_typescript_client.sh` rerun it and assert `git diff --exit-code` on all three generated artifacts. |
| `scripts/check_python_client.sh` | Regeneration drift (`sync_sdk_contract.py` + `git diff --exit-code` on operations snapshot + both generated tables) + Python unit tests (no server: `test_models.py`, `test_contract.py`, `test_cross_language.py`) + `Client`/`AsyncClient` import proof. | Python SDK changes. |
| `scripts/check_typescript_client.sh` | Same regeneration drift gate + `npm install` if needed + `tsc --noEmit` + `tsc` build + `node --test` contract/cross-language tests (no server). | TypeScript SDK changes. |
| `scripts/qualify_language_clients.sh` | Loopback servers (plain + auth-token configs): equivalent Python `pytest` (sync/async) and TS `npm test` flows with `EGGCHAOS_ADMIN_URL`/`EGGCHAOS_AUTH_URL`/`EGGCHAOS_BASE_URL`, then `python -m build` sdist/wheel and `npm pack` artifact builds. Uses status-preserving child cleanup (captured qualification status, guarded `wait` reaping, temp removal) so expected SIGTERM reaping never leaks exit 143. | Cross-language qualification. |
| `scripts/tests/test_cleanup_traps.sh` | Regression for the qualification cleanup pattern: static trap-shape checks on both server-spawning qualification scripts plus pass/fail fixtures proving exit preservation, child reaping, and temp cleanup. | Binding-qualification hygiene; runs in the `language-clients` CI job. |
| `scripts/tests/test_fetch_toxiproxy_post_v2_12_contract.sh` | M041 regression freezing the fetcher stdout contract: pattern checks (qualifier uses `--path-only`; fetcher emits `requested_toolchain`/`oracle_path`, documents `default (no flag)`, defaults to `mode="--path-only"`), `--help` (exit 0, usage on stderr) and `--bogus` (exit 2) arg parsing, plus end-to-end default/`--path-only`/`--json` single-line/executable/JSON-field/stderr-clean checks when a cached oracle exists (otherwise `partial:pattern-and-args-only`). | Fetcher-contract hygiene; runs in the `language-clients` CI job. |
| `scripts/check_python_native.sh` | `eggchaos-embed` + binding-crate tests, unsafe-boundary audit (`grep` forbids handwritten `unsafe` blocks/fns/impls; asserts `allow(unsafe_code)` in `lib.rs`), binding-crate audit (`cargo audit --file bindings/python-native/Cargo.lock`), abi3 wheel build via `maturin build`, abi3 `.so` zip check, per-wheel import smoke + server-independent Python tests. Target selection is host-aware (OS + architecture; Apple targets only on Darwin; `EGGCHAOS_NATIVE_TARGET` override for intentional cross builds). | Native binding changes. |
| `scripts/qualify_python_native.sh` | Remote/native conformance + control-overhead measurements against a loopback daemon. Same host-aware target selection and status-preserving server cleanup as above. | Binding qualification. |
| `scripts/build_python_native_artifacts.sh` | Host-native abi3 wheel + sdist + per-artifact import smoke (no publication). Apple cross-arch wheels are only produced on a Darwin host with the target installed; cross-built wheels are never import-smoked without a matching interpreter (selects the wheel matching `platform.machine()` for the smoke). | Wheel builds. |
| `scripts/release-smoke.sh` | Full pre-publish gate: fmt + clippy (`-D warnings`) + workspace tests + doc + `cargo build --workspace --release` + `cargo audit --deny warnings` + `cargo deny check advisories licenses bans sources` + `cargo package -p eggchaos-core --allow-dirty` + `cargo package --list --allow-dirty` for all workspace crates (core, experiment, protocol, server, eggfetch, toxiproxy, embed, cli) + `cargo build --release --locked --package eggchaos-cli` + `./scripts/release-artifact-smoke.sh` + an embedded Python `cargo metadata --locked` order-publishability proof asserting every intra-workspace path dependency requires exactly `^{workspace_version}` from the registry, documenting order `core -> experiment/eggfetch -> protocol -> server/toxiproxy/cli -> embed`. | Before any tag; first job step of the release `qualify` lane. |
| `scripts/release-artifact-smoke.sh` | Boots `target/release/eggchaos serve --config qualification/release/eggchaos.toml` (overridable as `$1`/`$2`), waits up to ~5 s for the `eggchaos listening; admin=` log line, then asserts: `/v1/health`; TCP `proxy list`; UDP `datagram proxy list`; `reset`; and `version`. Prints `{"artifact_smoke":"pass"}`; cleans up the child process and log on exit via trap. | Standalone after a release build; also called at the end of `release-smoke.sh` and as the last step of the release `qualify` job. |

Supporting evidence directories: `qualification/release/` (smoke TOML),
`qualification/toxiproxy-v2-12/` (pinned oracle baseline + Go/Python client
smokes), `qualification/performance/` (snapshots), `fuzz/` (plan, native
config/control/evidence, transition, compatibility, and `scenario_v2`
targets), `benchmarks/` (relay comparison harness).

## 2. CI (`.github/workflows/ci.yml`)

- Triggers: `push` and `pull_request`.
- Job `check`, `timeout-minutes: 25` (explicitly bounded: one stuck test
  once burned 4+ hours per platform), `fail-fast: true`,
  `RUST_TEST_THREADS: 4`, matrix
  `[ubuntu-latest, macos-latest, windows-latest]`.
- Toolchain: `dtolnay/rust-toolchain@stable` pinned to `1.89.0` with
  `rustfmt, clippy`; `Swatinem/rust-cache@v2`.
- Pinned scanners: `cargo-audit --locked --version 0.22.2`,
  `cargo-deny --locked --version 0.20.2` (installed from source each run).
- Steps in order: `cargo fmt --all -- --check`; `cargo clippy --workspace
  --all-targets --all-features -- -D warnings`; `cargo test --workspace
  --all-features`; `cargo test -p eggchaos-server --all-features
  runtime::datagram::tests::ipv6_loopback_works_when_host_capability_is_available
  -- --exact --nocapture` (IPv6 loopback capability reported visibly, not
  silently skipped); `cargo doc --workspace --all-features --no-deps`;
  `cargo audit --deny warnings`; `cargo deny check advisories licenses
  bans sources`.

Ordinary three-platform CI is necessary but not sufficient for release —
the dedicated release workflow must additionally pass on the exact
candidate (see §3).

- Job `language-clients` (`timeout-minutes: 25`, matrix
  `[ubuntu-latest, macos-latest] × python ["3.11", "3.12"] × node
  ["20", "22"]`, `actions/setup-python@v5` + `actions/setup-node@v4`,
  `pip install build pytest pyyaml`): `sh
  scripts/tests/test_cleanup_traps.sh`, `sh
  scripts/tests/test_fetch_toxiproxy_post_v2_12_contract.sh` (M041
  fetcher-contract regression), `./scripts/check_openapi.sh`,
  `./scripts/check_python_client.sh`,
  `./scripts/check_typescript_client.sh`,
  `./scripts/qualify_language_clients.sh`.
- Job `python-native` (`timeout-minutes: 25`, matrix
  `[ubuntu-latest, macos-latest] × python ["3.12"]`, pinned
  `maturin==1.9.5`, `pip install maturin==1.9.5 pytest build` plus
  `cargo-audit 0.22.2` for the binding lockfile):
  `./scripts/check_python_native.sh` then
  `./scripts/qualify_python_native.sh` on native-host wheels, kept
  separate so Rust and remote-SDK gates stay independent of Python
  packaging availability.

## 3. Release qualification (`.github/workflows/release.yml`)

- Triggers: `workflow_dispatch` (manual) and `push` tags `v*.*.*`.
- Job `qualify` (`ubuntu-latest`, `timeout-minutes: 60`,
  `RUST_TEST_THREADS: 4`): same pinned toolchain/scanners as CI, plus a
  documented workaround — `cargo-fuzz 0.13.2` is built with current
  `stable` (`rustup toolchain install stable --profile minimal` then
  `RUSTUP_TOOLCHAIN=stable cargo install cargo-fuzz --locked --version
  0.13.2`) because its transitive `cargo-platform@0.3.3` needs rustc
  1.91 while the MSRV toolchain is pinned 1.89.0; the fuzz target
  itself still builds/runs under 1.89.0 with `--sanitizer none`.
  Steps: `./scripts/release-smoke.sh`;
  `./scripts/benchmark_datagram.sh`;
  `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh`; acquire the
  checksum-pinned oracle via `echo
  "TOXIPROXY_SERVER=$(./scripts/fetch_toxiproxy_v2_12.sh)" >>
  "$GITHUB_ENV"`;
  `EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1
  ./scripts/qualify_toxiproxy_v2_12.sh`;
  `./scripts/qualify_eggfetch.sh`;
  `./scripts/release-artifact-smoke.sh`.
- Job `artifacts` (`timeout-minutes: 45`, `fail-fast: false`), 5-target
  matrix:
  - `ubuntu-22.04 / x86_64-unknown-linux-gnu`;
  - `ubuntu-22.04 / aarch64-unknown-linux-gnu` (installs
    `gcc-aarch64-linux-gnu`; sets
    `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc`
    because the host linker cannot link aarch64 objects);
  - `macos-14 / x86_64-apple-darwin` (cross from ARM64 host via Xcode
    clang; Intel `macos-13` runners are retired);
  - `macos-14 / aarch64-apple-darwin`;
  - `windows-2022 / x86_64-pc-windows-msvc`.
- Each artifact job: `cargo build --release --locked --package
  eggchaos-cli --target <triple>`; copies the binary to
  `eggchaos-v${GITHUB_REF_NAME#v}-<triple>[.exe]`; writes a `.sha256`
  sidecar via `sha256sum` (or `shasum -a 256` fallback); uploads via
  `actions/upload-artifact@v4` as `eggchaos-<triple>`.
- Local `dist/` snapshot at time of writing holds 5 binaries + 5
  `.sha256` files (`aarch64-apple-darwin`, `aarch64-unknown-linux-gnu`,
  `x86_64-apple-darwin`, `x86_64-pc-windows-gnu.exe`,
  `x86_64-unknown-linux-gnu`). The CI matrix is authoritative for the
  release; note the local snapshot's Windows triple (`-gnu`) differs
  from the CI matrix triple (`-msvc`) — verify against the release
  run before shipping.

### Exact-candidate context (M019 final, M041 latest)

- M008 closed first at `645a761`; post-candidate dependency/test/workflow
  fixes were reconciled by M014 and requalified by M015 at exact HEAD
  `cd88b22` (`plans/registry.md`, `plans/roadmap.md` §11A,
  `plans/008-qualification-release-and-distribution.md`,
  `plans/015-final-exact-head-release-requalification.md`,
  `plans/closure/M015-final-exact-head-release-requalification-closure.md`).
  M015 remains valid historical evidence for `cd88b22` but is no longer
  the final tag authority.
- M019 is the final pre-tag authority at exact candidate `ca527db`
  (`plans/closure/M019-qualification-expansion-and-final-corrective-requalification-closure.md`).
  M019 acceptance required on one SHA: green ordinary CI (3 OS) + green
  dedicated release workflow (qualify + 5/5 artifacts with checksums) +
  pinned-oracle Toxiproxy differential + Go/Python smokes +
  fuzz/security/package gates + perf within budget (empty-plan mean
  throughput ≥ 70% of same-session bare relay unless deliberately revised
  with evidence).
- M041 is the latest repository-level authority at exact candidate
  `724b967da04579282dd8bfc7a81dc4fe55d034a2` (hosted run `36219464594`,
  13/13 jobs; evidence in
  `plans/closure/M041-stream-loss-metrics-and-closure-hygiene-corrective-closure.md`):
  50/50 strict differential vs pinned v2.12.0 (oracle SHA-256
  `aa299966…95d15` per `plans/reference/toxiproxy-parity.md`),
  post-v2.12 qualifier pass against pinned `40f7fd31` (archive SHA-256
  `26351cc7…042b13d`, `go1.23.0`), per-proxy/direction stream-loss
  Prometheus samples emitted exactly once with real newlines, M041
  fetcher stdout contract enforced by
  `test_fetch_toxiproxy_post_v2_12_contract.sh`, plus
  `check_openapi.sh` (21 paths / 36 operations), `release-smoke.sh`,
  and `release-artifact-smoke.sh` on the candidate.
- If any fix lands after candidate selection, select a new candidate and
  rerun every affected gate. Planning-only closure-note commits may
  follow only if explicitly distinguished from the qualified code
  candidate.
- Even after clean qualification: **tag, crates.io publish (in verified order),
  and GitHub release creation remain explicit owner actions.** Do not
  automate them off `main` pushes.

## 4. Dependency and supply-chain discipline

Rule (from `AGENTS.md`, `plans/roadmap.md` §10,
`plans/000-architecture-and-scope-baseline.md`): prefer the smallest
direct Eggstack dependency that provides the needed primitive; never
copy sibling implementation code when a stable published crate surface
exists.

| Choice | Policy | Pinned surface (`Cargo.toml`) |
| --- | --- | --- |
| `eggress-relay` (not `eggress-embed`) | Bidirectional relay authority; eggchaos must not fork its half-close/copy semantics (`plans/adrs/001-stream-fault-engine-boundary.md`). | `eggress-relay 1.0.7` (package `eggress-relay`). |
| `eggserve-server` + `eggserve-primitives` (not `eggserve-core`, never `eggress-admin`) | Generic H1 admin substrate; `eggress-admin` owns Eggress-specific routing/UDP/metrics state. | `eggserve-server 0.2.0`, `eggserve-primitives 0.2.0`. |
| `eggfetch-core` minimal | CLI control client + public `Dialer` seam for in-process HTTP chaos; narrowest feature set that compiles. | `eggfetch-core 0.2.0`, `default-features = false`, features `native-http1 high-level-url json tls-rustls tls-native-roots`. |
| `eggress-outbound` | Optional/future only, behind an explicit optional feature, for chained upstream routes; not an MVP dependency. | Not in the current graph. |
| `eggress-testkit` | Reuse for echo / half-close / fragmentation / transport fixtures where practical (dev). | Per-crate dev-dependencies; add eggchaos-specific fixtures only when semantics differ. |

Additional invariants (all release-blocking):

- Workspace: edition 2021, `rust-version = "1.89"`, resolver v2
  (`Cargo.toml`, `rust-toolchain.toml` pins `channel = "1.89.0"`,
  `minimal` + `rustfmt, clippy`).
- Workspace lints: `[lints.rust] unsafe_code = "forbid"`,
  `missing_docs = "warn"`; `[lints.clippy]` `all` + `pedantic` warn
  with narrow `allow`s (`module_name_repetitions`,
  `must_use_candidate`, `missing_errors_doc`, `missing_panics_doc`).
  No `unsafe` without a separate explicit ADR + narrow audit.
- Release profile: `lto = "thin"`, `codegen-units = 1`,
  `strip = "symbols"` (`Cargo.toml`).
- `deny.toml`: advisories `unmaintained = workspace`, `yanked = deny`;
  bans `wildcards = deny`, `multiple-versions = warn`; licenses
  allow-list (`Apache-2.0`, `Apache-2.0 WITH LLVM-exception` for the
  M034 `target-lexicon` transitive build dep via `pyo3-build-config`,
  `BSD-3-Clause`, `CC0-1.0`, `CDLA-Permissive-2.0`, `ISC`, `MIT`,
  `MIT-0`, `Unicode-3.0`, threshold 0.8) with `[licenses.private]
  ignore = true`; sources restricted to crates.io
  (`unknown-registry/unknown-git = deny`, `allow-registry =
  ["https://github.com/rust-lang/crates.io-index"]`).
- Bounded-everything: queues, connection counts, body sizes (admin
  1 MiB), fault buffers, history/run retention (≤ 32 runs) are bounded;
  default overflow policy is backpressure, never silent unbounded
  allocation.
- Deterministic chaos: versioned SplitMix64-v1 sub-seeds from
  `(run_seed, proxy identity, connection key, direction, fault identity)`
  (`plans/adrs/002-determinism-and-live-mutation.md`); no
  process-global or scheduler-order RNG. Stream-chunk dropping is
  `stream-loss` in native APIs (only the Toxiproxy compat presentation
  may say `packet_loss`); it never reuses ADR 003 datagram-loss
  semantics.
- Loopback-by-default for native admin/compat listeners; non-loopback
  requires explicit opt-in + auth policy. Machine-readable JSON is a
  first-class CLI/control contract (`--json` emits one document; nonzero
  exit on failure).

## 5. Repo governance (`plans/` + `docs/`)

Canonical surface is `plans/` (`AGENTS.md`, `plans/README.md`):

| Path | Authority |
| --- | --- |
| `plans/roadmap.md` | Long-term architecture, sequencing, invariants, non-goals, release gates. Status line names M019 final pre-tag authority and M041 latest corrective authority. |
| `plans/registry.md` | Compact source of truth for milestone status, dependencies, activation, closure. Update it in the same change that activates/blocks/closes/supersedes a milestone. M041 row is `closed` at `724b967da04579282dd8bfc7a81dc4fe55d034a2` with hosted run `36219464594` (13/13). |
| `plans/000-architecture-and-scope-baseline.md` | Investigated baseline and boundaries. |
| `plans/001-*.md` … `plans/041-*.md` | Executable handoffs; filename prefix is the milestone sequence number and must not be reused. |
| `plans/adrs/` | Durable decisions (`001-stream-fault-engine-boundary.md`, `002-determinism-and-live-mutation.md`, `003-datagram-impairment-boundary-and-semantics.md`, `004-deterministic-scenario-schedules-and-replay-identity.md`, `005-cross-project-integration-boundary-and-experiment-identity.md`, `006-cross-language-control-contracts-and-native-binding-boundary.md`, `007-post-v2-12-toxiproxy-stream-loss-compatibility.md`); implementation must not silently change them. |
| `plans/reference/` | Parity/verification contracts (`toxiproxy-parity.md`, `verification-matrix.md`), not status. |
| `plans/closure/` | Independent closure evidence after implementation (candidate SHA, commands, oracle, artifacts, limitations, verdict). M041 evidence is `M041-stream-loss-metrics-and-closure-hygiene-corrective-closure.md`; M040 remains preserved historical evidence. |
| `plans/archive/` | Superseded material only; never delete history to look cleaner. |

Status vocabulary (only these): `ready` (dependencies satisfied,
handable to an implementer), `blocked` (must name the blocking
milestone/evidence gap), `active`, `implemented-awaiting-evidence`
(code looks complete, closure evidence incomplete), `closed` (acceptance
+ verification evidence satisfied, points at closure record/evidence),
`superseded` (names the successor plan). Registry closure from source
inspection alone is forbidden.

Every numbered plan must contain all 11 handoff items: (1) objective +
user-visible outcome; (2) baseline + dependencies; (3) scope + explicit
non-goals; (4) affected crates/modules or expected files; (5) ordered
work packages; (6) behavioral invariants + failure semantics; (7) test
and verification commands; (8) acceptance criteria; (9) rejection/stop
conditions; (10) evidence required for closure; (11) follow-on
activation rules.

Closure-evidence rule (`plans/registry.md`, `plans/README.md`): a
milestone becomes `closed` only when implementation is on the target
branch, the plan's tests/commands ran on the exact candidate where
practical, external/differential evidence is present rather than
inferred, docs + registry match implementation, no unresolved
medium-or-higher finding remains, and a `plans/closure/` note names the
candidate, evidence, limitations, and successor activation. Missing
execution (e.g. no pinned oracle run) is recorded as incomplete
evidence.

`docs/` vs `plans/reference/` split: `docs/*.md` (`architecture.md`,
`configuration.md`, `control-plane.md`, `toxiproxy.md`, `eggfetch.md`)
are user-facing contracts; `plans/reference/*.md` are qualification
contracts the release is measured against. Keep them consistent at
closure; neither may claim unsupported behavior.

## 6. Non-goals and future work (gates)

| Item | State | Gate to activate |
| --- | --- | --- |
| UDP / datagram (`ChaosDatagram`) impairment | completed / maintenance complete | ADR 003 and M020–M025 are closed; exact-candidate evidence is in `plans/registry.md` and `plans/closure/`. Follow-on datagram semantics require separate planning. |
| Optional `eggress-outbound` chained upstreams | future | Proven demand; optional feature only; must not turn eggchaos into a second proxy framework. |
| `eggreplay` timing/fault integration | future (downstream) | M031 closure + EggReplay-owned adoption plan; `.eggr`/semantic timing remain EggReplay authority. No `eggreplay-*` production dependency in eggchaos. |
| `eggprobe` controlled experiments | future (downstream) | M031 closure + EggProbe-owned adoption plan; route/probe/report semantics remain EggProbe authority. No `eggprobe-*` production dependency in eggchaos. |
| Python / TypeScript remote control SDKs + Python native embedding | implemented; correctively qualified via M035, contract held by M041 | ADR 006 chain M032–M034 closed; M035 reconciles hosted SDK/native-Python qualification, the shared datagram mutation authority, and planning closure (closed at `a710cd6`, hosted 13/13 green). A generic C ABI remains deferred pending a separate ADR and demonstrated multi-consumer demand (M034 decided no-go). |
| Richer schedulers / time-varying fault scripts | completed (bounded v2 model) | ADR 004 tranche M026–M028 closed and qualified at `ceb3bae`; ScenarioV1 remains a compatibility surface. Ramps, predicates, lifecycle actions, and cron require separate planning and must compile to or compose with the bounded model. |
| Post-v2.12 Toxiproxy (`packet_loss` etc.) | implemented / correctively qualified | ADR 007 chain M036–M039 plus correctives M040 (closed at `48fe0dd`, hosted `36214657871` green) and M041 (closed at `724b967`, hosted `36219464594` green) are closed. Strict v2.12 remains default/frozen; native name stays `stream-loss` (never ADR 003 datagram loss, never IP/TCP packet loss). Later upstream tags require separate reconciliation against pinned `40f7fd31`. |
| TLS interception / HTTP rewriting, forward/CONNECT/SOCKS proxying, QUIC/SSH protocols, plugin ABI, distributed coordination, DB persistence | non-goals (`plans/000-architecture-and-scope-baseline.md`) | Fresh planning pass; none may weaken the fixed-target, protocol-neutral core boundary (`plans/roadmap.md` §14). |
| SBC target-class benchmarking / service-manager integration | post-v1 follow-up | Target hardware + operational demand. |

## 7. Review checklist (release / governance reviewer)

1. Candidate SHA frozen; ordinary CI (Ubuntu/macOS/Windows) and the
   dedicated release workflow green on that exact SHA
   (`.github/workflows/ci.yml`, `.github/workflows/release.yml`,
   `plans/015-final-exact-head-release-requalification.md` WP1–WP3;
   M019 at `ca527db` is the final pre-tag authority, M041 at `724b967`
   the latest corrective authority).
2. `scripts/check.sh` clean; `scripts/release-smoke.sh` (incl.
   audit/deny/package-list/order-proof/artifact-smoke) clean, with the
   order-proof asserting `core -> experiment/eggfetch -> protocol ->
   server/toxiproxy/cli -> embed` via `cargo metadata --locked`.
3. `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh` executed and
   timed (9 targets incl. `scenario_v2`); `cargo audit --deny warnings`
   and `cargo deny check advisories licenses bans sources` clean
   (`deny.toml` policy holds, incl. LLVM-exception + private-ignore +
   crates.io-only sources).
4. `./scripts/qualify_toxiproxy_v2_12.sh` reports
   `differential:pass` against pinned oracle `2.12.0` (checksum
   `aa299966…95d15` per `plans/reference/toxiproxy-parity.md`); 50/50
   strict corpus + Go/Python smokes recorded (M041), or `incomplete`
   explicitly recorded with the milestone kept out of `closed`.
   `./scripts/qualify_toxiproxy_post_v2_12.sh` passes against pinned
   `40f7fd31` where the post-v2.12 profile is claimed, using the M041
   fetcher contract (default and `--path-only` print exactly one path;
   `--json` prints one metadata record); `sh
   scripts/tests/test_fetch_toxiproxy_post_v2_12_contract.sh` clean.
5. `./scripts/qualify_eggfetch.sh` passes on the candidate (H1,
   keep-alive/live mutation, HTTPS trust/rejection, H2 concurrency,
   blackhole/timeout, mid-response termination, shaping,
   reconnect/redial, error/redaction).
6. `./scripts/benchmark_datagram.sh` within the retained M023 + M024
   budgets; 5/5 artifact jobs succeeded; names/triples/`.sha256`/sizes
   recorded; runtime smoke claimed only for executed targets, build-only
   otherwise.
7. Perf within M008 budget (empty-plan mean ≥ 70% of same-session bare
   relay) or explained with evidence (`scripts/benchmark.sh`,
   `qualification/performance/`).
8. `unsafe_code = "forbid"` holds (`Cargo.toml`); any exception has an
   ADR + audit or blocks release. Loopback-by-default, auth opt-in,
   secret redaction, 1 MiB body cap, bounded buffers/history verified
   (`docs/control-plane.md`, `plans/reference/verification-matrix.md`
   §11).
9. `plans/registry.md` updated in the same change; closure note under
   `plans/closure/` names candidate, commands, platforms, oracle,
   artifacts, limitations, verdict, and successor; `docs/` and
   `plans/reference/` match implementation; version `0.1.0` census done.
   `plans/roadmap.md` status names M019 final and M041 latest; no
   `closed` from source inspection alone.
10. Tag / crates.io publish (order `core -> experiment/eggfetch -> protocol ->
    server/toxiproxy/cli -> embed` per `release-smoke.sh` order-proof) / GitHub
    release treated as separate owner decisions — never implied by a
    green workflow alone.
