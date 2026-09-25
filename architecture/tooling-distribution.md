# Tooling, release, and repo governance

This is a deep dive under [Eggchaos architecture overview](overview.md).
It covers the reproducible command surface (`scripts/`), CI/release
workflows (`.github/workflows/`), dependency/supply-chain policy
(`Cargo.toml`, `deny.toml`, `rust-toolchain.toml`), shipped artifacts
(`dist/`), and the `plans/` governance authority that gates any release.

Pre-release `0.1.0`. Milestones M000–M025 plus M008 are closed. M019
passed its final corrective qualification on `ca527db`; M025 closed the
post-M024 datagram association setup/closure hygiene pass on `55911f6`.
The ADR 004 scenario-schedule tranche (M026–M028), the ADR 005
integration-boundary tranche (M029–M031), and the ADR 006
cross-language tranche (M032–M034) are closed. M035 is the sole ready
corrective successor for hosted SDK/native-Python qualification,
shared datagram mutation authority, and planning closure.
Tagging, crates.io publication, and GitHub release creation remain explicit
owner actions (`plans/registry.md`, `plans/README.md`,
`plans/019-qualification-expansion-and-final-corrective-requalification.md`).

## 1. Scripts catalog (`scripts/`)

All scripts are POSIX `sh` with `set -eu`. They are the canonical
commands; CI and the release workflow invoke them rather than
re-implementing their steps.

| Script | What it runs | When to run it |
| --- | --- | --- |
| `scripts/check.sh` | `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace --all-features`; `cargo doc --workspace --all-features --no-deps`. Note: no `audit`/`deny` here — those live in CI and `release-smoke.sh`. | Every local change before push; fastest full local gate. |
| `scripts/benchmark.sh` | `cargo fmt --manifest-path benchmarks/Cargo.toml -- --check`; `cargo run --manifest-path benchmarks/Cargo.toml --release --quiet`. | No-fault throughput/latency vs bare `eggress-relay` (see `benchmarks/`, `qualification/performance/`). Do not invent budgets before measurement (`plans/roadmap.md` §12). |
| `scripts/benchmark_datagram.sh` | Runs the direct UDP echo / benchmark-local bare fixed-target relay / fixed-target benchmark in release mode (sequential-RTT plus windowed-throughput modes with core-only scheduler depth probes), saves raw candidate/host/sample JSON when requested, then checks the measured no-fault ratios. | Datagram performance qualification; the retained M023 budget is ≥45% of same-session direct datagrams/s and ≤2.5× direct p95 latency, plus the M024 topology-matched budget (empty/bare ≥0.7× sequential throughput, ≤1.6× sequential p95, ≥0.7× windowed throughput). |
| `scripts/qualify_eggfetch.sh` | `cargo test -p eggchaos-eggfetch --all-features`; `cargo test -p eggchaos-server --all-features`. | After any `eggchaos-eggfetch` / server / `eggfetch-core` profile change; part of the release qualify lane. |
| `scripts/qualify_fuzz.sh` | Runs `plan_json`, `datagram_plan_json`, `datagram_transitions`, `native_config`, `native_control_json`, `fault_evidence_json`, `policy_transitions`, and `toxiproxy_attributes` with `cargo fuzz --sanitizer none`, each for `${EGGCHAOS_FUZZ_RUNS:-10000}` runs. | Bounded parser/state-machine soak before release. Release workflow pins 10,000 runs per target. |
| `scripts/fetch_toxiproxy_v2_12.sh` | Downloads the official v2.12.0 server asset for Linux/Darwin x86_64/ARM64, verifies the pinned SHA-256, checks `-version`, then prints the binary path. | Before strict differential qualification; release workflow provisions this explicitly and never trusts a `PATH` binary. |
| `scripts/qualify_toxiproxy_v2_12.sh` | Always runs translation tests. Developer mode can report missing/unverified oracle as `incomplete`; `EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1` requires an explicit `TOXIPROXY_SERVER`, supported-host pinned checksum, version 2.12.0, and a clean `DIFFERENTIAL_SUMMARY`. | Toxiproxy parity work and mandatory release differential gate. The release workflow fetches the oracle then enables strict mode. |
| `scripts/check_openapi.sh` | `cargo test -p eggchaos-protocol --all-features` (OpenAPI drift + golden fixtures) + `cargo test -p eggchaos-server --all-features --test native_route_inventory` (live 36-operation inventory proof) + a YAML shape assertion. | Native contract gate after any protocol/server/OpenAPI change; part of the release qualify lane. |
| `scripts/check_python_client.sh` | Regeneration drift + Python unit tests (no server). | Python SDK changes. |
| `scripts/check_typescript_client.sh` | Typecheck + build + contract/cross-language tests (no server). | TypeScript SDK changes. |
| `scripts/qualify_language_clients.sh` | Loopback servers: equivalent sync/async/TS flows + sdist/wheel and tarball builds. Uses status-preserving child cleanup (captured qualification status, guarded `wait` reaping, temp removal) so expected SIGTERM reaping never leaks exit 143. | Cross-language qualification. |
| `scripts/tests/test_cleanup_traps.sh` | Regression for the qualification cleanup pattern: static trap-shape checks on both server-spawning qualification scripts plus pass/fail fixtures proving exit preservation, child reaping, and temp cleanup. | Binding-qualification hygiene; runs in the `language-clients` CI job. |
| `scripts/check_python_native.sh` | `eggchaos-embed` + binding-crate tests, unsafe-boundary audit, binding-crate audit, abi3 wheel build, server-independent Python tests. Target selection is host-aware (OS + architecture; Apple targets only on Darwin; `EGGCHAOS_NATIVE_TARGET` override for intentional cross builds). | Native binding changes. |
| `scripts/qualify_python_native.sh` | Remote/native conformance + control-overhead measurements against a loopback daemon. Same host-aware target selection and status-preserving server cleanup as above. | Binding qualification. |
| `scripts/build_python_native_artifacts.sh` | Host-native abi3 wheel + sdist + per-artifact import smoke (no publication). Apple cross-arch wheels are only produced on a Darwin host with the target installed; cross-built wheels are never import-smoked without a matching interpreter. | Wheel builds. |
| `scripts/release-smoke.sh` | Full pre-publish gate: fmt + clippy (`-D warnings`) + workspace tests + doc + `cargo build --workspace --release` + `cargo audit --deny warnings` + `cargo deny check advisories licenses bans sources` + `cargo package -p eggchaos-core --allow-dirty` + `cargo package --list --allow-dirty` for all workspace crates + `cargo build --release --locked --package eggchaos-cli` + `./scripts/release-artifact-smoke.sh` + an embedded Python `cargo metadata --locked` order-publishability proof asserting every intra-workspace path dependency requires exactly `^{workspace_version}` from the registry, documenting order `core -> experiment/eggfetch -> protocol -> server/toxiproxy/cli -> embed`. | Before any tag; first job step of the release `qualify` lane. |
| `scripts/release-artifact-smoke.sh` | Boots `target/release/eggchaos serve --config qualification/release/eggchaos.toml` (overridable as `$1`/`$2`), waits up to ~5 s for the `eggchaos listening; admin=` log line, then asserts: `/v1/health`; TCP `proxy list`; UDP `datagram proxy list`; `reset`; and `version`. Prints `{"artifact_smoke":"pass"}`; cleans up the child process and log on exit via trap. | Standalone after a release build; also called at the end of `release-smoke.sh` and as the last step of the release `qualify` job. |

Supporting evidence directories: `qualification/release/` (smoke TOML),
`qualification/toxiproxy-v2-12/` (pinned oracle baseline + Go/Python client
smokes), `qualification/performance/` (snapshots), `fuzz/` (plan, native
config/control/evidence, transition, and compatibility targets), `benchmarks/`
(relay comparison harness).

## 2. CI (`.github/workflows/ci.yml`)

- Triggers: `push` and `pull_request`.
- Job `check`, `timeout-minutes: 25` (explicitly bounded: one stuck test
  once burned 4+ hours per platform), `fail-fast: true`, matrix
  `[ubuntu-latest, macos-latest, windows-latest]`.
- Toolchain: `dtolnay/rust-toolchain@stable` pinned to `1.89.0` with
  `rustfmt, clippy`; `Swatinem/rust-cache@v2`.
- Pinned scanners: `cargo-audit --locked --version 0.22.2`,
  `cargo-deny --locked --version 0.20.2` (installed from source each run).
- Steps in order: `cargo fmt --all -- --check`; `cargo clippy --workspace
  --all-targets --all-features -- -D warnings`; `cargo test --workspace
  --all-features`; `cargo doc --workspace --all-features --no-deps`;
  `cargo audit --deny warnings`; `cargo deny check advisories licenses
  bans sources`.

Ordinary three-platform CI is necessary but not sufficient for release —
M015 additionally requires the dedicated release workflow on the exact
candidate (see §3).

- Job `language-clients` (`timeout-minutes: 25`, matrix
  `[ubuntu-latest, macos-latest] × python ["3.11", "3.12"] × node
  ["20", "22"]`): `sh scripts/tests/test_cleanup_traps.sh`,
  `./scripts/check_openapi.sh`, `./scripts/check_python_client.sh`,
  `./scripts/check_typescript_client.sh`,
  `./scripts/qualify_language_clients.sh`.
- Job `python-native` (M035; `timeout-minutes: 25`, matrix
  `[ubuntu-latest, macos-latest] × python ["3.12"]`, pinned
  `maturin==1.9.5`): `./scripts/check_python_native.sh` then
  `./scripts/qualify_python_native.sh` on native-host wheels, kept
  separate so Rust and remote-SDK gates stay independent of Python
  packaging availability.

## 3. Release qualification (`.github/workflows/release.yml`)

- Triggers: `workflow_dispatch` (manual) and `push` tags `v*.*.*`.
- Job `qualify` (`ubuntu-latest`, `timeout-minutes: 60`): same pinned
  toolchain/scanners as CI, plus a documented workaround — `cargo-fuzz
  0.13.2` is built with current `stable` because its transitive
  `cargo-platform@0.3.3` needs rustc 1.91 while the MSRV toolchain is
  pinned 1.89.0; the fuzz target itself still builds/runs under 1.89.0
  with `--sanitizer none`. Steps: `./scripts/release-smoke.sh`;
  `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh`;
  `./scripts/qualify_toxiproxy_v2_12.sh`;
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
  from the CI matrix triple (`-msvc`) — verify against the M015 release
  run before shipping.

### M015 exact-HEAD context

- M008 closed first at `645a761`; post-candidate dependency/test/workflow
  fixes were reconciled by M014 and requalified by M015 at exact HEAD
  `cd88b22` (`plans/registry.md`, `plans/roadmap.md` §11A,
  `plans/008-qualification-release-and-distribution.md`,
  `plans/015-final-exact-head-release-requalification.md`,
  `plans/closure/M015-final-exact-head-release-requalification-closure.md`).
- M015 acceptance required on one SHA: green ordinary CI (3 OS) + green
  dedicated release workflow (qualify + 5/5 artifacts with checksums) +
  47/47 Toxiproxy differential vs pinned v2.12.0 + Go/Python smokes +
  fuzz/security/package gates + perf within budget (empty-plan mean
  throughput ≥ 70% of same-session bare relay unless deliberately revised
  with evidence).
- If any fix lands after candidate selection, select a new candidate and
  rerun every affected gate. Planning-only closure-note commits may
  follow only if explicitly distinguished from the qualified code
  candidate.
- Even after clean M015: **tag, crates.io publish (in verified order),
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
  allow-list (`Apache-2.0`, `BSD-3-Clause`, `CC0-1.0`,
  `CDLA-Permissive-2.0`, `ISC`, `MIT`, `MIT-0`, `Unicode-3.0`,
  threshold 0.8); sources restricted to crates.io
  (`unknown-registry/git = deny`).
- Bounded-everything: queues, connection counts, body sizes (admin
  1 MiB), fault buffers, history/run retention (≤ 32 runs) are bounded;
  default overflow policy is backpressure, never silent unbounded
  allocation.
- Deterministic chaos: versioned SplitMix64-v1 sub-seeds from
  `(run_seed, proxy identity, connection key, direction, fault identity)`
  (`plans/adrs/002-determinism-and-live-mutation.md`); no
  process-global or scheduler-order RNG. Stream-chunk dropping is never
  described as IP/TCP packet loss in native APIs.
- Loopback-by-default for native admin/compat listeners; non-loopback
  requires explicit opt-in + auth policy. Machine-readable JSON is a
  first-class CLI/control contract (`--json` emits one document; nonzero
  exit on failure).

## 5. Repo governance (`plans/` + `docs/`)

Canonical surface is `plans/` (`AGENTS.md`, `plans/README.md`):

| Path | Authority |
| --- | --- |
| `plans/roadmap.md` | Long-term architecture, sequencing, invariants, non-goals, release gates. |
| `plans/registry.md` | Compact source of truth for milestone status, dependencies, activation, closure. Update it in the same change that activates/blocks/closes/supersedes a milestone. |
| `plans/000-architecture-and-scope-baseline.md` | Investigated baseline and boundaries. |
| `plans/001-*.md` … `plans/025-*.md` | Executable handoffs; filename prefix is the milestone sequence number and must not be reused. |
| `plans/adrs/` | Durable decisions (`001-stream-fault-engine-boundary.md`, `002-determinism-and-live-mutation.md`); implementation must not silently change them. |
| `plans/reference/` | Parity/verification contracts (`toxiproxy-parity.md`, `verification-matrix.md`), not status. |
| `plans/closure/` | Independent closure evidence after implementation (candidate SHA, commands, oracle, artifacts, limitations, verdict). |
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
| `eggreplay` timing/fault integration | future | Stable eggreplay flow model + M008. |
| `eggprobe` controlled experiments | future | Stable eggprobe diagnostics contract + M008. |
| Python / TypeScript remote control SDKs + Python native embedding | implemented; corrective qualification via M035 | ADR 006 chain M032–M034 closed; M035 reconciles hosted SDK/native-Python qualification, the shared datagram mutation authority, and planning closure. A generic C ABI remains deferred pending a separate ADR and demonstrated multi-consumer demand. |
| Richer schedulers / time-varying fault scripts | future | M011 corrected scenario model proven + M008. |
| Post-v2.12 Toxiproxy (`packet_loss` etc.) | future | M012 v2.12 parity requalified + M008; native name must be stream-chunk loss, not packet loss. |
| TLS interception / HTTP rewriting, forward/CONNECT/SOCKS proxying, QUIC/SSH protocols, plugin ABI, distributed coordination, DB persistence | non-goals (`plans/000-architecture-and-scope-baseline.md`) | Fresh planning pass; none may weaken the fixed-target, protocol-neutral core boundary (`plans/roadmap.md` §14). |
| SBC target-class benchmarking / service-manager integration | post-v1 follow-up | Target hardware + operational demand. |

## 7. Review checklist (release / governance reviewer)

1. Candidate SHA frozen; ordinary CI (Ubuntu/macOS/Windows) and the
   dedicated release workflow green on that exact SHA
   (`.github/workflows/ci.yml`, `.github/workflows/release.yml`,
   `plans/015-final-exact-head-release-requalification.md` WP1–WP3).
2. `scripts/check.sh` clean; `scripts/release-smoke.sh` (incl.
   audit/deny/package-list/order-proof/artifact-smoke) clean.
3. `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh` executed and
   timed; `cargo audit --deny warnings` and `cargo deny check
   advisories licenses bans sources` clean (`deny.toml` policy holds).
4. `./scripts/qualify_toxiproxy_v2_12.sh` reports
   `differential:pass` against pinned oracle `2.12.0` (checksum
   `aa299966…95d15` per `plans/reference/toxiproxy-parity.md`); 47/47
   corpus + Go/Python smokes recorded, or `incomplete` explicitly
   recorded with the milestone kept out of `closed`.
5. `./scripts/qualify_eggfetch.sh` passes on the candidate (H1,
   keep-alive/live mutation, HTTPS trust/rejection, H2 concurrency,
   blackhole/timeout, mid-response termination, shaping,
   reconnect/redial, error/redaction).
6. 5/5 artifact jobs succeeded; names/triples/`.sha256`/sizes recorded;
   runtime smoke claimed only for executed targets, build-only otherwise.
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
10. Tag / crates.io publish (order `core -> experiment/eggfetch -> protocol ->
    server/toxiproxy/cli -> embed` per `release-smoke.sh` order-proof) / GitHub
    release treated as separate owner decisions — never implied by a
    green workflow alone.
