# M028 — Deterministic Schedule Qualification and Hardening — Closure

Status: closed  
Exact implementation candidate: `ceb3bae5b45391240d0e3f27f93b260575f9540a`  
Implementation commit: `ceb3bae5b45391240d0e3f27f93b260575f9540a`  
Closure commit: this file's commit; the implementation candidate above is the qualified code head  
Depends on: M027 (closed at `5d85d15`), M026 (closed at `e0507d1`)

## Objective verdict

M028 qualifies the M026/M027 scenario-v2 compiler and runtime as one
reproducible post-release feature tranche on the exact candidate
above. Deterministic compilation/replay identity, drift-free deadline
scheduling, safe concurrent mutation/cleanup, bounded hostile-input
behavior, and no regression to stream, datagram, Toxiproxy, Eggfetch,
security, package, or performance contracts are all evidenced below.
No new schedule language feature was added; one narrow hardening fix
(`checked_add` on the epoch/deadline sum) was required and is
qualified as part of the candidate.

## WP1 — Candidate and matrix

Candidate SHA: `ceb3bae5b45391240d0e3f27f93b260575f9540a` (clean tree;
`git status` empty at gate time). Toolchain: pinned rustc/cargo
1.89.0 per `rust-toolchain.toml`. Host: macOS arm64 (Apple M4 Pro)
for local gates; Ubuntu/macOS/Windows CI is the release workflow's
hosted gate and remains an owner release-step action. Pinned oracle:
`toxiproxy-server 2.12.0` via checksum-verified
`scripts/fetch_toxiproxy_v2_12.sh`. Fuzz: `cargo-fuzz` with the
documented stable-toolchain workaround, `--sanitizer none`,
10,000 runs per target.

## WP2 — Golden compiler/replay corpus

Committed under `crates/eggchaos-server/tests/schedule_corpus/` and
verified by `tests/schedule_corpus.rs`:

- 9 JSON fixtures: minimal one-phase stream, minimal one-phase
  datagram, mixed stream/datagram phases, equal-deadline multiple
  actions, finite repeat expansion, defaults-equal-explicit,
  seed/execution-key/offset sensitivity.
- 1 TOML fixture converging on the minimal-stream fingerprint.
- `expected.json` freezing, per case: SHA-256 fingerprint, event
  count, compiled offsets, phase identities, and v2 policy namespace
  vectors. Spot values: minimal-stream
  `cba7c6ae8ca198a4e1708d999f44e0b09c880cc880bb3660e04bf05c5e79cda8`
  (matches the M026 unit golden), repeat offsets
  `[0,100,300,600,800,1100,1300]` with phases
  `[top/0,repeat/1/0,repeat/1/1,repeat/2/0,repeat/2/1,repeat/3/0,repeat/3/1]`.
- Relations pinned: omitted isolation/cleanup default without
  identity change; seed, execution key, offset, action/transport
  each change the fingerprint; namespace vectors differ with seed.
- Bound/rejection cases (in-test, not committed bulk): exactly 1024
  events compiles; 1025 fails; `u64::MAX` duration-sum overflow
  fails; malformed/unknown-version/unknown-field/empty-action
  bodies fail bounded without panic.
- Fixture changes require a compiler-semantics version bump plus a
  documented `EGGCHAOS_CORPUS_DUMP=1` regen; the test fails loudly
  otherwise.

## WP3 — Paused-time scheduler qualification

Tokio paused time is the timing authority (`#[tokio::test(start_paused
= true)]`):

- events at 1s/2s/3s fire at exact epoch offsets with unrelated
  control work between them (`late_by_ns == 0` throughout);
- sparse 1h-gap events stay epoch-anchored;
- multiple equal-offset events execute in compiled order;
- deadline-passed events apply immediately with deterministic
  nonnegative lateness (400ms/300ms case);
- cancellation just before a deadline applies nothing further and
  restores;
- unrepresentable deadline near the `u64` limit waits pending until
  cancellation with no panic (covers the `checked_add` hardening);
- zero-offset first events throughout the corpus and runtime suites.
- Wall-clock integration tests use bounded timeouts only for
  liveness polling, never as sole correctness evidence.

## WP4 — Isolation/cleanup race qualification

Exercised for stream and datagram directions:

- no external mutation → Completed + restore;
- manual mutation before the next strict event → fail-fast Failed +
  cleanup Conflict, external state preserved (stream and datagram);
- manual mutation racing as another strict scenario → deterministic
  loser Failed with per-resource Conflict/Restored split, winner
  plan intact;
- live mode absorbs a completed manual update (Completed);
- cancellation while sleeping → prompt Cancelled + restore;
- failure after several events → Failed + restore;
- leave → NotRequested, state kept;
- two-resource cleanup with one conflict restores the other;
- target deletion mid-run → Failed naming the proxy, no panic;
- service shutdown cancels/joins v2 tasks (shared JoinSet);
- scheduled stream publication beside live echo traffic (relay
  keeps working) and scheduled datagram publication beside a live
  association (association survives).
- Stream drain-barrier and datagram admission-snapshot semantics are
  engine-level, unchanged, and covered by their existing suites.

## WP5 — Fuzz/security/bounds

- `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh` — pass, all 9
  targets (2000-run pass also recorded mid-tranche). The
  `native_control_json` target now round-trips
  `ScenarioScheduleV2Dto` and compiles parsed schedules; no panic or
  unbounded allocation from hostile counts/durations/names.
- `cargo audit --deny warnings` — pass. `cargo deny check advisories
  licenses bans sources` — pass (only dependency added in the
  tranche is MIT/Apache-2.0 `sha2`, M026).
- Control-plane bounds verified: 1 MiB body cap applies to the new
  routes (shared admin path); malformed version/format/unknown
  fields return bounded JSON errors; non-loopback auth path
  untouched (existing e2e); TOML/JSON errors carry messages, never
  secrets or source dumps; phase names bounded at 128 bytes;
  server accepts schedule content only, never a filesystem path
  (CLI reads the explicit local file); run/history retention
  bounded (32-run maps); no payload in evidence (assertion scans
  run JSON for `payload`); no shell/callback/plugin surface
  (route inventory reviewed: validate/compile/apply/get/delete
  only).

## WP6 — API/CLI compatibility

- V1 JSON fixture/apply/get/cancel behavior green (existing
  `scenario_v1_*` unit tests plus CLI e2e v1 block, extended to
  assert no `schedule_fingerprint`/`execution_key`/`cleanup` keys
  leak into v1 records).
- `ScheduleValidateV2`/`ScheduleCompileV2`/`ScheduleRunV2` shapes
  frozen by construction tests and the CLI e2e (fingerprint length,
  `top/0` phase spelling, `offset_ns`, cleanup outcome spellings).
- TOML and JSON v2 apply converge on one fingerprint (unit, corpus,
  and live-server e2e).
- CLI emits one JSON document per operation and exits nonzero on
  failure (e2e asserts parseable single docs; `clap` help lists
  `apply|validate|compile|get|cancel`). No `examples/help` path
  exists in the repo; the generated `--help` text was verified to
  list the new subcommands.

## WP7 — Product regressions/performance

On the exact candidate:

- `./scripts/check.sh` — pass (fmt, `clippy -D warnings`,
  workspace tests, doc). Workspace total: 226 passed, 0 failed.
- `./scripts/release-smoke.sh` — pass (release build, audit,
  deny, all five `cargo package --list`, locked CLI release build,
  artifact smoke, publish-order proof
  `core->eggfetch->server/toxiproxy/cli`).
- `./scripts/qualify_eggfetch.sh` — pass.
- Strict pinned Toxiproxy qualification — 50/50 differential pass
  vs checksum-verified `toxiproxy-server 2.12.0` oracle.
- `./scripts/benchmark_datagram.sh` — `datagram_budget: pass`,
  `matched_budget: pass` (empty/bare sequential throughput 0.974 ≥
  0.70, windowed 0.746 ≥ 0.70, empty/direct 0.5345 ≥ 0.45, matched
  p95 1.125× ≤ 1.6, direct p95 1.62× ≤ 2.5). M024 budgets green.
- Stream no-fault bench (`benchmarks --bin eggchaos-benchmarks`,
  release): bare 5003 vs empty-plan 5502 MiB/s — scheduler support
  adds no per-byte work when no schedule runs (no engine file is
  touched by the tranche; v2 code executes only inside run tasks
  and on v2 control paths).
- Existing stream/datagram deterministic golden traces green
  (workspace suite).
- Compiler/dispatcher measurements (Apple M4 Pro, debug build,
  control-plane only): small-schedule compile+fingerprint ~15µs;
  1024-event compile+fingerprint ~7.3ms; 1024 same-deadline
  publications plus restore cleanup ~20ms (~20µs/event). No
  regression threshold is frozen: single-host debug numbers do not
  justify one, and the feature adds no hot-path work. Any future
  threshold must be evidence-based per this note.

## WP8 — Documentation/planning reconciliation

Updated in the tranche: `plans/registry.md`, `plans/roadmap.md`
(status + §11F), `plans/README.md`, `AGENTS.md` (determinism line +
ADR 004 chain marked complete), `plans/reference/verification-matrix.md`
(golden-fixture list, control-matrix v2 routes, determinism-matrix
replay-identity rule), `architecture/scenario-observability.md`
(§1.8.4 resolved + §1.9 runtime), `architecture/control-plane-cli.md`
(route + CLI tables), `docs/control-plane.md` (§Scenarios, CLI
inventory, metrics). `docs/architecture.md` carries no scenario
section (nothing to reconcile). Recorded limitation everywhere it
matters: policy/event replay is deterministic; live
connection/datagram arrival timing is not.

## Acceptance verdict

M028 closes cleanly: M026 and M027 are closed; one exact candidate
(`ceb3bae5b45391240d0e3f27f93b260575f9540a`) carries the declared
qualification verdict; golden fingerprints/namespaces pass unchanged;
replay identity is proven independent of daemon run_id; paused-time
tests prove one-epoch deadlines with no cumulative drift;
strict/live conflict semantics and restore/leave cleanup are
race-tested for stream and datagram resources; cancellation/shutdown
leave no untracked task or silent restoration; 1024-event and hostile
expansion bounds hold without panic/unbounded allocation;
JSON/TOML/API/CLI fixtures pass with ScenarioV1 compatible;
security/dependency gates are clean; Eggfetch and strict pinned
Toxiproxy regressions pass; deterministic traces and M024 budgets
remain green; compiler/dispatch performance is measured with no
invented threshold; docs state the traffic-timing replay limit; no
unresolved medium-or-higher finding remains.

## Follow-on activation

A clean M028 closes the first richer deterministic-scenario /
time-varying schedule tranche. Later work such as smooth ramp sugar,
predicates/branches, lifecycle actions, replay integration with
eggreplay, diagnostic orchestration with eggprobe, or persisted cron
scheduling requires separate planning and must compile to or compose
with this bounded model rather than bypass it.

## Limitations and incomplete evidence

- Hosted CI (Ubuntu/macOS/Windows) and multi-platform artifact jobs
  remain owner release-step actions; local gates are the
  qualification basis recorded here.
- `scripts/benchmark.sh` (stream bench entry) cannot run as written
  because the benchmarks workspace now has two binaries
  (`eggchaos-benchmarks`, `datagram`) and cargo cannot infer
  `default-run` — pre-existing, unrelated to this tranche (M024
  added the datagram binary). The stream bench was run explicitly
  with `--bin eggchaos-benchmarks` for this evidence.
- Fuzz corpus churn under `fuzz/corpus/` is gitignored working
  state, not evidence; only the committed seed files plus the
  schedule golden corpus are records.
