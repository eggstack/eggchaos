# Verification and qualification

Part of [Eggchaos architecture overview](overview.md) (item 7). Evidence-first
review handoff for how eggchaos proves correctness. Canonical contracts live in
`plans/reference/verification-matrix.md` and
`qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md`; canonical status
lives in `plans/registry.md` and `plans/closure/`. Do not assert by inspection:
if a gate was not run, record it as incomplete (see §8).

## 1. Test layers (what runs where)

### Unit: per-fault state machines

- `crates/eggchaos-core/src/engine.rs` (`mod tests`): per-type activation
  counting (`activations_count_per_type_deterministically`), probability
  0/1 boundaries (`probability_zero_never_activates_and_one_always_does`),
  token-bucket burst/cap/cumulative behavior, zero-delay disconnect at first
  boundary, finite-blackhole prefix + termination, deterministic-state replay
  (`identical_inputs_give_identical_deterministic_state`), namespace-varying
  decisions with replay.
- `crates/eggchaos-core/src/stream.rs` (`mod tests`, all
  `#[tokio::test(start_paused = true)]`): latency bounded + flushable,
  queue backpressure at capacity + wake, flush/shutdown while queued.
- `crates/eggchaos-core/src/plan.rs`, `policy.rs`, `rng.rs`: validation,
  generation publication, seed derivation (see §2).
- `crates/eggchaos-server/src/runtime.rs`, `admin.rs`, `scenario.rs`,
  `config.rs`: relay embedding, admission limits, control authority,
  scenario driver, schema-v1 TOML compile-through-validation
  (`config.rs:132`).
- `crates/eggchaos-toxiproxy/src/lib.rs` (translation unit tests, 10 in
  M008/M015 census) and `crates/eggchaos-eggfetch/src/lib.rs` (adapter lib
  tests, 3): translator-only behavior; every view derives from
  `ControlState`.

### Property: proptest byte conservation

- `crates/eggchaos-core/src/engine.rs:1349-1369`
  (`preserving_accept_conserves_bytes`, `ProptestConfig::with_cases(64)`):
  random fragments through a preserving combination
  (latency + bandwidth + slice) must conserve bytes;
  `bytes_accepted` equals the fragment total.
- Rule from `plans/reference/verification-matrix.md` §9: property tests
  emphasize byte conservation for preserving fault combinations. Non-preserving
  faults (blackhole discard accounting, limit-data prefix, disconnect
  termination) assert their own exact boundaries instead.

### Half-close / shutdown

- Core: `stream.rs` flush-as-delivery-barrier, preserving buffers resolve
  before close, slow-close delay honored, runtime cancellation can still
  terminate (`verification-matrix.md` §1).
- Runtime: `crates/eggchaos-server/src/runtime.rs` half-close policy
  (`HalfClosePolicy::Drain`, `half_close` field ~`runtime.rs:769-783,2157-2194`),
  `eggress-testkit` fixtures where suitable (echo, request half-close then
  response, target half-close, fragmented writes, slow reader/writer —
  `verification-matrix.md` §3). `eggress-relay` remains the relay authority;
  eggchaos never forks its copy/half-close semantics.
- Eggfetch: `server_closed_idle_connection_redials`,
  `disconnect_fault_terminates_http_stream` in
  `crates/eggchaos-eggfetch/tests/regression.rs`.

### Bounded-buffer / backpressure

- `stream.rs:1022-1032`
  (`latency_queue_backpressures_at_capacity_and_wakes`): bounded queue reaches
  cap, returns pending, wakes, resumes without duplication/loss.
- `engine.rs` capacity surface (`capacity_bytes`, `bytes currently owned in
  the bounded queue`, `engine.rs:139,556-566,663-669`).
- Matrix (`verification-matrix.md` §1-§2): every fault with a queue proves
  cap → pending → wake → resume; flush delivers all preserving accepted bytes
  before success; shutdown drains preserving bytes first.

### RNG golden vectors

- `crates/eggchaos-core/src/rng.rs:106-124`:
  `golden_vectors_are_stable` (`DeterministicRng::new(42)` →
  `2949826092126892291`, `5139283748462763858`; `derive_seed(42,"proxy",7,…)` →
  `11882912530514077282`) and
  `policy_seed_derivation_is_stable_and_sensitive`
  (`derive_policy_seed(7,1,0/1)`, `(8,1,0)` vectors + sensitivity).
- `engine.rs:1118-1141`, `1244-1280`: namespace-varying probabilistic
  decisions reproduce on replay; identical inputs give identical state.
- `verification-matrix.md` §7: commit goldens for seed derivation, Bernoulli
  decisions, jitter values, slice sizes, scenario ordering, evidence
  serialization; rerun with scheduling noise and verify decisions identical.
  No process-global or scheduler-order RNG.

### JSON / config round-trips

- Core plan: `fuzz/fuzz_targets/plan_json.rs:11-15` asserts
  validated-plan serialize → parse round-trip equality; unit tests cover
  ordered-fault order preservation through round trip
  (`verification-matrix.md` §1).
- Control/API (`verification-matrix.md` §4): exact JSON success schemas,
  stable error envelope, invalid JSON / body-too-large (1 MiB cap per
  `docs/control-plane.md`), invalid duration/rate/buffer/probability,
  unknown proxy/fault/connection, duplicate create, generation-conflict CAS,
  concurrent reads during mutation, reset, health/readiness, auth, loopback
  default + non-loopback opt-in.
- CLI (`crates/eggchaos-cli/tests/cli_e2e.rs`, 1 test
  `cli_json_create_fault_kill_reset_end_to_end`): `--json` valid JSON on
  success and failure, nonzero exits, no ANSI in JSON, config parse errors,
  admin-unavailable, auth redaction.
- Config: `qualification/release/eggchaos.toml` (schema-v1 seed + admin bind
  + `smoke` proxy) is the release-smoke fixture consumed by
  `scripts/release-artifact-smoke.sh`.

### Differential vs Toxiproxy v2.12.0 (47/47)

- Oracle pinned: `toxiproxy-server version 2.12.0`, SHA-256
  `aa299966b52f16a8594f1cd0d1e9049dc2e8fe2c04a90c19860e2719b2b95d15`
  (M015 closure), download URL + route/toxic baseline in
  `qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md`.
- Corpus: `crates/eggchaos-toxiproxy/tests/differential.rs`
  (`toxiproxy_v212_differential`): same API + data-plane sequences against
  oracle (`TOXIPROXY_SERVER`, port `18747`) and in-process compat server.
  47 passed / 0 failed on the M015 candidate `cd88b22` with 4 declared
  normalizations only: disjoint-bind `listen` replacement (concrete ports
  asserted per server), f64 number canonicalization (Go `1` vs `1.0`),
  toxicity clamp into `[0,1]`, degenerate-zero `rate`/`average_size`/`bytes`
  coalesced to 1. Status codes, toxic defaults, direction, byte boundaries
  are never normalized.
- Case input: `qualification/toxiproxy-v2-12/cases/default-latency.json`;
  corpus README: `qualification/toxiproxy-v2-12/README.md` (seven toxics
  only, tolerance windows for timing, ephemeral-port/timestamp normalization
  only).
- Gate: `scripts/qualify_toxiproxy_v2_12.sh` runs the translation suite
  always; runs the differential only with a pinned `2.12.0` binary, else
  prints `{"translation":"pass","oracle":"unavailable","differential":"incomplete"}`
  and exits 0 — never treats inspection as differential proof.

### Go + Python client smokes

- Files (`qualification/toxiproxy-v2-12/client-smoke/`):
  `go/main.go`, `go/go.mod` (`github.com/Shopify/toxiproxy/v2 v2.12.0`),
  `go/go.sum`, `go/go_results.json`, `py_smoke.py` (stdlib `urllib` only),
  `py/py_results.json` (`py_results.json` at the directory root).
- Coverage: version, create, populate, add toxic (incl. auto-name
  `<type>_<stream>`), update via POST and PATCH, list, remove, disable/enable,
  reset, delete.
- M015 exact-candidate evidence: Go 13 steps / 0 failed
  (`/tmp/go-m015c.json`), Python 12 steps / 0 failed (`/tmp/py-m015c.json`);
  historical shorthand "13/13" covered the same 12-step Python script
  (M015 closure § "Client smokes"). Stored snapshots `go_results.json` /
  `py_results.json` show the same all-pass shape.

### CLI e2e

- `crates/eggchaos-cli/tests/cli_e2e.rs` (sole integration test file for the
  CLI crate): boots `NativeAdmin` + echo origin, drives the real `eggchaos`
  binary (`CARGO_BIN_EXE_eggchaos`) through proxy add → fault add (latency
  50 ms downstream) → live-connection kill → reset → unknown-name nonzero
  exit, all over `--json` with parsed-body assertions.

### Eggfetch regression (H1/H2)

- `crates/eggchaos-eggfetch/tests/regression.rs` (10 tests, M013–M015):
  H1 keep-alive with live policy update, HTTPS trust (added CA) and rejection
  (self-signed, no key-material leak), H2 multiplexing over one fault-wrapped
  connection (`http2` feature), blackhole non-hang, mid-response truncation-as-error,
  downstream bandwidth pacing, idle-close redial, refused-dial shaped error,
  disconnect termination-as-error.
- `crates/eggchaos-eggfetch/src/tests.rs` (20 tests, M029): arbitrary-inner
  dialer composition (target forwarding, single attempt, routed dialer,
  all five `DialErrorKind`s preserved), deterministic connection-key
  providers (inputs, stability, explicit collision, failure/panic
  behavior), bounded `RecordingObserver` (exactly-once, eviction),
  bidirectional evidence (generations, bytes, termination, post-drop
  reads), H1 keep-alive reuse vs forced separate connections, live
  policy engagement, and the H1/H2 client regressions.
- `crates/eggchaos-eggfetch/tests/correlation.rs` (M031): cross-layer
  fixture correlating schedule fingerprint/seed/execution key, event
  generations, physical connection key, active fault evidence, and the
  workload-observed outcome through EggFetch + route dialer + chaos
  adapter + prepared experiment on one shared epoch.
- `crates/eggchaos-eggfetch/tests/adapter_overhead.rs` (M031):
  empty-plan throughput vs bare duplex (functional ratio bounds),
  wrap latency with observer disabled/enabled, and
  compile/prepare/start overhead for small and 1024-event schedules.
- Gate: `scripts/qualify_eggfetch.sh` runs
  `cargo test -p eggchaos-eggfetch --all-features` plus
  `cargo test -p eggchaos-server --all-features` (M015: adapter 3 +
  regression 10 + server 37 + toxiproxy 10, all pass).

### Integration-boundary and experiment harness (M029–M031)

- `crates/eggchaos-experiment` (23 tests, M030): compiler determinism
  and seed sensitivity, prepare-time capability/missing-resource
  rejection with no publication, epoch-gate sharing (exactly-once
  capture, late waiters), no-event-before-release, paused-time
  drift-free deadlines and compiled-order preservation, cancel
  before/while/after publication, strict conflict without overwrite,
  live-mode current-state behavior, restore/leave cleanup, set/remove
  flows, and target bounds.
- `crates/eggchaos-server/src/scenario_v2/conformance_tests.rs`
  (M030): server `ControlState` adapter vs in-process stream target
  produce equivalent event/generation outcomes for the stream subset;
  datagram actions fail explicitly on the stream-only target while the
  server applies them.
- Golden Scenario V2 corpus (`schedule_corpus`, M026/M028) runs
  unchanged against the extracted `eggchaos-experiment` authority via
  server re-exports; fingerprints and namespace vectors are frozen.
- Dependency direction is verified structurally (`cargo tree -p
  eggchaos-experiment` shows only core + serde/sha2/thiserror/
  tokio/tokio-util) and by the release order proof
  (`core->experiment/eggfetch->server/toxiproxy/cli` in
  `scripts/release-smoke.sh`); no `eggreplay-*`/`eggprobe-*`
  dependency exists anywhere in the workspace.

### Cross-language SDKs and native embedding (M032–M035)

- Contract authority: `scripts/check_openapi.sh` runs
  `cargo test -p eggchaos-protocol --all-features` (OpenAPI drift +
  golden fixtures) + `cargo test -p eggchaos-server --all-features
  --test native_route_inventory` (live 36-operation inventory proof) +
  a YAML shape assertion.
- Remote SDKs: `scripts/check_python_client.sh` (regeneration drift +
  Python unit tests, no server), `scripts/check_typescript_client.sh`
  (typecheck + build + contract/cross-language tests, no server),
  `scripts/qualify_language_clients.sh` (loopback servers, equivalent
  sync/async/TS flows, sdist/wheel + tarball builds). Hosted matrix:
  `[ubuntu-latest, macos-latest] × python 3.11/3.12 × node 20/22`.
- Cleanup hygiene (M035): both server-spawning qualification scripts
  use status-preserving child cleanup (captured status, `set +e` in
  cleanup, guarded `wait` reaping so expected SIGTERM never leaks exit
  143, temp removal, exit with the original status).
  `scripts/tests/test_cleanup_traps.sh` pins the trap shape plus
  pass/fail exit-preservation, reaping, and temp-cleanup fixtures.
- Shared datagram mutation authority (M035): `ControlState` owns
  `add/get/list/update/remove_datagram_fault` (duplicate detection,
  cross-direction ID uniqueness, patch non-emptiness, plan
  reconstruction, generation-guarded publication) in core/runtime
  types; `NativeAdmin` and `eggchaos-embed` only convert DTOs and map
  errors. `crates/eggchaos-embed/tests/facade.rs`
  (`datagram_facade_and_http_admin_agree_on_mutation_and_conflicts`)
  proves HTTP/embed equivalence across create/get/list/patch/delete,
  same-direction and cross-direction conflicts, empty-patch rejection,
  and post-delete not-found, alongside the retained stream
  conformance test.
- Native binding: `scripts/check_python_native.sh`
  (`eggchaos-embed` tests, binding-crate test/audit, handwritten-
  `unsafe` audit, abi3 wheel inspection, import/runtime smoke) and
  `scripts/qualify_python_native.sh` (remote/native conformance +
  control-overhead measurements). Target selection is host-aware
  (Apple targets only on Darwin; `EGGCHAOS_NATIVE_TARGET` override for
  intentional cross builds, never runtime-qualified without a matching
  import). Hosted gate: dedicated `python-native` CI job on
  `[ubuntu-latest, macos-latest] × python 3.12` with pinned
  `maturin==1.9.5`. Platform support claims live in
  `bindings/python-native/README.md` (hosted runtime-qualified vs
  built-only vs unqualified).

### No-fault throughput/latency regression vs bare `eggress-relay`

- Harness: `benchmarks/src/main.rs` (7 cases: `bare_eggress_relay`,
  `eggchaos_empty_plan`, `latency_1ms`, `bandwidth_16mib_s`, `slice_16k`,
  `combined_latency_slice`, `eggfetch_adapter_empty_policy`; 64 KiB buffers,
  `EGGCHAOS_BENCH_BYTES`/`EGGCHAOS_BENCH_ROUNDS` knobs),
  `benchmarks/Cargo.toml` (excluded workspace, pinned `eggress-relay 1.0.7`,
  `eggfetch-core`, `tokio`), runner `scripts/benchmark.sh`
  (fmt-check + `cargo run --release --quiet`).
- Snapshots: `qualification/performance/README.md` (method + caveats),
  `qualification/performance/2026-09-22-macos-arm64.json` (provisional,
  candidate `5f46825`) and
  `qualification/performance/2026-09-22-macos-arm64-m008.json` (candidate
  `645a761`, Mac16,8 M4 Pro, 64 MiB/5 rounds, host block + per-case
  mean/throughput/samples + frozen budget).
- Budget (frozen M008, rechecked M015): empty-plan mean throughput ≥ 70% of
  same-session bare relay; deliberate-fault delay excluded; absolute numbers
  host-specific, ratio is the gate (M008 0.982; M015 sanity 0.821 ≥ 0.70).

## 2. Deterministic Tokio-time policy + wall-clock tolerance rule

- Deterministic first: use `#[tokio::test(start_paused = true)]` +
  `tokio::time::advance(...)` wherever the fault owns the clock
  (`stream.rs` latency/backpressure/flush/shutdown tests; bandwidth
  paused-time tests per `verification-matrix.md` §2).
- Wall-clock assertions require a justified tolerance window and are never
  the sole evidence. Examples of the allowed pattern:
  - differential data-plane: latency byte preservation is exact; delay is
    `elapsed >= 150 ms` for a 200 ms fault (`differential.rs:data_plane`);
  - eggfetch regression: live-latency `>= 100 ms` for 150 ms fault,
    shaping `>= 1200 ms`, keep-alive bodies byte-exact alongside;
  - throughput comparators are tolerance-based
    (`verification-matrix.md` §5).
- `docs/control-plane.md` carries no separate verification section; its
  contracts (route inventory, generation CAS, seed-namespace derivation,
  loopback default, 1 MiB body cap, bearer redaction, JSON-first CLI) are the
  assertions the control/API/CLI layers above verify.

## 3. Fuzz

- Target: `fuzz/fuzz_targets/plan_json.rs` — arbitrary bytes →
  `serde_json::from_slice::<FaultPlan>` → `validate()` → serialize →
  re-parse → `assert_eq!`. Catches parser/validator/round-trip panics and
  divergence.
- Manifest: `fuzz/Cargo.toml` (`eggchaos-fuzz`, `cargo-fuzz = true`,
  deps `eggchaos-core`, `libfuzzer-sys 0.4`, `serde_json 1`, standalone
  `[workspace]`).
- Corpus: `fuzz/corpus/plan_json/seed.json` (`{"faults":[]}`) is the only
  tracked seed; run-generated byproducts are removed. Artifacts:
  `fuzz/artifacts/` must be empty on a clean gate (M015: empty, 0 crashes).
- Gate: `scripts/qualify_fuzz.sh`
  (`EGGCHAOS_FUZZ_RUNS` default 10000;
  `cargo fuzz run plan_json --sanitizer none -- -runs=…`; prints
  `{"fuzz":"pass","target":"plan_json"}`). Release workflow installs
  `cargo-fuzz 0.13.2` under current stable (1.89.0 cannot compile its
  transitive `cargo-platform@0.3.3`) but drives the target under pinned
  1.89.0 — see `.github/workflows/release.yml`.
- Datagram fuzz targets cover plan JSON, transition/accounting bounds, datagram
  DTOs, schema-v1 TOML compilation, and association evidence round-trips.
  `datagram_transitions` reconciles emitted, queued, configured loss, explicit
  overflow, oversize, and duplicated candidates.

## 4. Benchmarks + qualification snapshots

| What | Where | Notes |
| --- | --- | --- |
| Harness + cases | `benchmarks/src/main.rs`, `benchmarks/src/bin/datagram.rs`, `benchmarks/Cargo.toml`, `scripts/benchmark.sh`, `scripts/benchmark_datagram.sh` | Stream cases plus direct UDP, benchmark-local bare fixed-target relay, fixed-target empty plan, sequential-RTT and windowed-throughput modes, core-only scheduler depth probes, individual/combined datagram faults, and multi-client workload |
| Method README | `qualification/performance/README.md` | Topology-matched bare relay isolates proxy-hop cost from engine overhead; sequential RTT and windowed throughput are separate measurements; fault delay excluded from overhead |
| Snapshots | `qualification/performance/2026-09-22-macos-arm64.json`, `...-m008.json`, `2026-09-24-macos-arm64-m023.json`, `2026-09-24-macos-arm64-m024-before.json`, `2026-09-24-macos-arm64-m024-after.json` | Host block (OS/model/CPU/Rust/profile), candidate SHA, method, results, and budget |
| Budget | `...-m008.json:budget`; `2026-09-24-macos-arm64-m023.json`; M024 matched budget in `scripts/benchmark_datagram.sh` | Stream `empty_plan ≥ 70% of same-session bare relay`; datagram `≥45%` of direct UDP throughput and `≤2.5×` direct p95 latency (retained M023 floor); M024 topology-matched empty/bare `≥0.7×` sequential throughput, `≤1.6×` sequential p95, `≥0.7×` windowed throughput |
| Release TOML | `qualification/release/eggchaos.toml` | Artifact-smoke fixture (seed 7, loopback admin, TCP `smoke` and UDP `udp-smoke` proxies) |
| Oracle baseline | `qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md` | Live-captured 2026-09-22: identity, routes, reset/populate, 7 toxic defaults, non-API 404s |
| Client smokes | `qualification/toxiproxy-v2-12/client-smoke/{go/,py_smoke.py,*_results.json}` | Pinned Go client + stdlib Python; rerun fresh per qualification |

## 5. CI matrix + release workflow evidence (M015 exact-HEAD)

- Ordinary CI (`.github/workflows/ci.yml`): `ubuntu-latest`,
  `macos-latest`, `windows-latest`; `timeout-minutes: 25`; toolchain
  `1.89.0` + `rustfmt,clippy`; `cargo fmt --all -- --check`, `cargo clippy
  --workspace --all-targets --all-features -- -D warnings`,
  `cargo test --workspace --all-features`,
  `cargo doc --workspace --all-features --no-deps`,
  `cargo audit --deny warnings`,
  `cargo deny check advisories licenses bans sources`.
  M015: run `35810065730` on `cd88b22`, all three platforms green.
- Hosted SDK/native gates (`.github/workflows/ci.yml`): job
  `language-clients` (`[ubuntu-latest, macos-latest] × python
  3.11/3.12 × node 20/22`: cleanup-trap regression, OpenAPI drift,
  Python/TS checks, live cross-language qualification) and job
  `python-native` (M035; `[ubuntu-latest, macos-latest] × python
  3.12` with `maturin==1.9.5`: embed/binding checks plus
  remote/native conformance on native-host wheels). M035 closure
  records the exact-candidate run IDs/URLs for the full matrix.
- Release qualification (`.github/workflows/release.yml`, `workflow_dispatch`
  + `v*.*.*` tags): `qualify` job on ubuntu (`timeout-minutes: 60`:
  `release-smoke.sh` → fuzz 10k → toxiproxy qualify → eggfetch qualify →
  artifact smoke) plus `artifacts` matrix (`timeout-minutes: 45`) over 5
  targets: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`
  (cross-linker `aarch64-linux-gnu-gcc`), `x86_64-apple-darwin` (cross-built
  on `macos-14`; `macos-13` Intel retired), `aarch64-apple-darwin`,
  `x86_64-pc-windows-msvc`, each with checksummed upload.
  M015: run `35810310455` on `cd88b22`, qualify + 5/5 artifacts green
  (see `plans/closure/M015-final-exact-head-release-requalification-closure.md`
  for run/job IDs, SHAs, sizes).
- Platform reset semantics: library correctness must not assume Unix-only
  sockets; hard reset gets a capability result + per-platform evidence
  (`verification-matrix.md` §2/§10; `docs/architecture.md` hard-reset split).
  `reset_peer` termination is guaranteed; the RST/FIN distinction is not
  asserted (M008 limitations). CI covers Linux/macOS/Windows; IPv4/IPv6 and
  SBC class are qualified where runners permit, otherwise recorded incomplete.

## 6. Canonical verification commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
cargo audit --deny warnings
cargo deny check advisories licenses bans sources
./scripts/benchmark.sh
EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh
./scripts/qualify_toxiproxy_v2_12.sh
TOXIPROXY_SERVER=/path/to/pinned/v2.12.0 ./scripts/qualify_toxiproxy_v2_12.sh
./scripts/qualify_eggfetch.sh
./scripts/release-smoke.sh
./scripts/release-artifact-smoke.sh
sh scripts/tests/test_cleanup_traps.sh
./scripts/check_openapi.sh
./scripts/check_python_client.sh
./scripts/check_typescript_client.sh
./scripts/qualify_language_clients.sh
./scripts/check_python_native.sh
./scripts/qualify_python_native.sh
./scripts/benchmark_datagram.sh
```

Integration-test files (via `crates/*/tests/*` glob): exactly

- `crates/eggchaos-cli/tests/cli_e2e.rs`,
- `crates/eggchaos-toxiproxy/tests/differential.rs`,
- `crates/eggchaos-eggfetch/tests/regression.rs`.

Everything else is unit tests inside `crates/*/src/` plus the external
`fuzz/`, `benchmarks/`, `qualification/`, `scripts/` gates above.

## 7. Incomplete-evidence rule

From `AGENTS.md` and `plans/reference/verification-matrix.md` §12: when a
platform or external oracle cannot run, record it as incomplete evidence —
never replace missing execution with "the code looks correct." A blocked
milestone names the dependency; a closed milestone points at the closure
record or exact verification evidence.

Concrete applications: developer mode of the Toxiproxy qualify script and
`differential.rs` report `incomplete` without a verified pinned `2.12.0`
binary. Release mode is strict and fails unless the architecture-specific
SHA-256 and version pass and a clean differential summary is emitted.
Bandwidth/slicer/slow_close now have oracle-backed byte and tolerance-based
timing cases; the recorded measurements and intentional timing limits are in
`plans/reference/toxiproxy-parity.md`. Foreign-arch artifact execution smoke
is build-verified + checksummed only (M008/M015);
closure notes use the fixed format (milestone, candidate + implementation
commits, commands, platforms, oracle/version, artifacts, limitations, verdict,
registry transition, next activation).

## 8. Review checklist for a verifier

1. `cargo fmt`, `clippy -D warnings`, full workspace tests, `cargo doc`
   green on the exact candidate? (`scripts/check.sh` is the short form.)
2. Audit + deny green? Any new `git` source in `Cargo.lock`?
3. Differential: pinned `2.12.0` SHA matches baseline? 47/47 with only the 4
   declared normalizations? Transcript (`/tmp/qualify_m012.log`) attached?
4. Client smokes rerun fresh against the candidate compat server (Go 13,
   Python 12 steps, 0 failed)? Transcripts kept?
5. Fuzz 10k pass, `fuzz/artifacts/` empty, only tracked corpus retained?
6. Eggfetch qualify pass (adapter 3 + regression 10 + server suites)?
7. Benchmark ratio ≥ 0.70 same-session (fault delay excluded)? Host block +
   raw JSON recorded under `qualification/performance/`?
8. Every wall-clock assertion paired with an exact assertion (bytes/status/
   schema) plus a justified window — no timing-only proof?
9. Golden RNG vectors unchanged; any seed-derivation change accompanied by
   updated vectors + replay evidence?
10. Bounded-everywhere census: new queue/count/body/buffer has a cap, a
    backpressure test, and no payload capture in evidence/metrics/history?
11. Loopback default + auth/redaction + platform reset-capability behavior
    covered and docs (`docs/control-plane.md`, `docs/toxiproxy.md`,
    `docs/architecture.md`) match implementation with no packet-loss or
    "fully compatible" overclaim?
12. Closure record names the exact candidate, commands, platforms, oracle,
    artifacts, limitations, verdict, and registry transition — and any gap is
    labeled `incomplete`, not closed?
