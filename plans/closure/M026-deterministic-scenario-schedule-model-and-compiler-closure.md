# M026 — Deterministic Scenario Schedule Model and Compiler — Closure

Status: closed  
Exact implementation candidate: `e0507d1cf202d459d6348fb8544b0abb36149033`  
Implementation commit: `e0507d1cf202d459d6348fb8544b0abb36149033`  
Closure commit: this file's commit; the implementation candidate above is the qualified code head  
Depends on: M025 (closed at `55911f6`)

## What M026 delivered

M026 implements the pure, bounded ScenarioScheduleV2 source language,
deterministic compiler, canonical SHA-256 schedule fingerprint, and
run_id-independent v2 policy namespace derivation defined by ADR 004.
No HTTP route, no ControlState publication, no CLI subcommand, no run
task, no change to stream or datagram engine semantics, no change to
the v1 wire shape or v1 run lifecycle.

The v2 source language is a control-plane scheduling layer above the
existing stream and datagram fault engines. The compiler does not
touch the engines; it emits a frozen `CompiledScenarioV2` event tape
that M027 will hand to `ControlState` so it plays through the same
authoritative publication paths as the existing scenario supervisor.

### Source language

Owned by `crates/eggchaos-server/src/scenario_v2/source.rs`:

- `ScenarioScheduleV2 { version: 2, seed, execution_key, isolation,
  cleanup, phases, repeat }` with `deny_unknown_fields`.
- `SchedulePhaseV2 { name, duration_ns, actions }` with `name` bounded
  to 128 bytes and presentation-only (never enters the fingerprint).
- `ScheduleRepeatV2 { count: 1..=MAX_REPEAT_COUNT (64), phases }` —
  one-level finite repetition.
- `IsolationPolicyV2 { Strict, Live }` (default strict).
- `CleanupPolicyV2 { RestoreInitial, Leave }` (default restore-initial).
- `ScenarioAction` is the existing v1 action enum unchanged. Stream
  and datagram variants remain explicit; no transport-generic union.

Structural bounds: `MAX_PHASES = 256`, `MAX_PHASE_ACTIONS = 64`,
`MAX_REPEAT_COUNT = 64`, `MAX_COMPILED_EVENTS = 1024`,
`MAX_PHASE_NAME_BYTES = 128`, `SCHEDULE_SCHEMA_VERSION = 2`,
`COMPILER_SEMANTICS_VERSION = 1`.

### Pure compiler

Owned by `crates/eggchaos-server/src/scenario_v2/compiler.rs`:

- `compile_schedule(&ScenarioScheduleV2) -> Result<CompiledScenarioV2,
  ScheduleError>` is a pure function of `(source,
  COMPILER_SEMANTICS_VERSION)`.
- Validates phase name bounds, repeat count range, phase/phase-action
  counts, and required structural shape before expansion.
- Counts the expanded event tape with `checked_add` so a hostile
  repeat count cannot grow beyond the ceiling; the ceiling reject
  fires before any allocation happens, so no partial compile followed
  by truncation is reachable.
- Validates each phase's actions through `FaultPlan::new` and
  `DatagramPlan::new` so the action validation authority is shared
  with the runtime rather than duplicated.
- Summed phase durations with `checked_add`; overflow rejects before
  any event is emitted.
- Compiled output is immutable and ordered: `compiled_index` is a
  stable zero-based index, `phase` is a `CompiledPhaseIdentity`
  (`Top { index }` or `Repeat { iteration, index }`), `offset_ns` is
  absolute from the future run epoch, and the action round-trips
  losslessly. Equal-offset events preserve source order; the
  Proptest suite proves that property.

### Canonical SHA-256 fingerprint

Owned by `crates/eggchaos-server/src/scenario_v2/fingerprint.rs`:

- SHA-256 with a domain/version prefix `eggchaos/scenario-v2/fingerprint/v1/compiler-semantics=`
  followed by the 4-byte big-endian `COMPILER_SEMANTICS_VERSION` and
  then `encode_compiled_for_fingerprint`'s output.
- The encoder is hand-written rather than a `serde_json::Value` dump;
  it freezes one byte-ordering and excludes hash-map iteration,
  source file path, comments, run_id, wall-clock timestamps, and
  optional phase names. Documentation/template-only presentation
  fields cannot leak into the digest.
- The fingerprint is semantic identity: equivalent JSON or TOML
  sources (different whitespace, different comments, different
  action order that still collapses to the same compiled tape) reach
  the same digest.

### Portable v2 namespace derivation

Owned by `crates/eggchaos-core/src/rng.rs`:

- New helper `derive_schedule_policy_seed(scenario_seed,
  execution_key, schedule_fingerprint, compiled_event_index)`.
- Pure fold over `(seed, execution_key, 32-byte fingerprint lanes,
  event index)` with no daemon `run_id` parameter.
- Golden vector test `schedule_policy_seed_derivation_is_stable_and_sensitive`
  pins three positive vectors and three sensitivity vectors; the v1
  `derive_policy_seed` golden vectors remain byte-identical under
  the v1 helper.

### Wire DTOs

Owned by `crates/eggchaos-server/src/native_v2.rs`:

- `ScenarioScheduleV2Dto` is the JSON wire form with explicit kebab-case
  action tags matching `ScenarioActionV1`. Unknown fields are rejected.
- `ScenarioScheduleV2Toml` is the TOML authoring form. Both share the
  same integer-nanosecond duration representation at the parser edge,
  so silent rounding or overflow is impossible by construction. A
  future human-duration parser is a separate plan.
- `From<ScenarioScheduleV2> for ScenarioScheduleV2Dto` plus a
  `to_canonical_json` helper preserve the round-trip from internal
  source back to a wire-shape JSON document for follow-on apply.

### Fuzz and property coverage

- `fuzz/fuzz_targets/scenario_v2.rs` asserts JSON/TOML parse +
  compile purity and identical-input determinism; registered in
  `fuzz/Cargo.toml` and `scripts/qualify_fuzz.sh` (now 9 targets).
- `crates/eggchaos-server/src/scenario_v2/property_tests.rs` runs
  proptest-derived sources through the compiler and asserts purity,
  seed-keyed fingerprint sensitivity, and structural rejection
  invariants.

## Verification on the exact candidate

All commands below ran against
`e0507d1cf202d459d6348fb8544b0abb36149033`.

### Local gates

- `./scripts/check.sh` — pass: workspace fmt, Clippy with `-D warnings`,
  all workspace tests, and `cargo doc --no-deps` all clean.
  Workspace tests cover 196 passing assertions across crates (114 in
  eggchaos-server including 31 v2 source tests + 3 v2 property
  tests + 3 v2 wire DTO tests + 18 v2 module + native_v2 tests).
- `cargo test -p eggchaos-core --all-features` — pass: 57 tests
  including the new `schedule_policy_seed_derivation_is_stable_and_sensitive`
  covering the v2 namespace derivation; v1 `derive_policy_seed`
  golden vectors unchanged.
- `cargo test -p eggchaos-server --all-features` — pass: 114 tests
  total, 34 of which cover scenario v2 (source, compiler, fingerprint,
  DTO, property). No regression in v1 scenario or runtime tests.
- `cargo doc --workspace --all-features --no-deps` — pass: every new
  item carries `#[deny(missing_docs)]`-compliant documentation.

### Fuzz

- `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh` — pass for all
  nine targets (plan_json, datagram_plan_json, datagram_transitions,
  native_config, native_control_json, fault_evidence_json,
  policy_transitions, toxiproxy_attributes, scenario_v2). No panic,
  no shrink target found.

### Security and dependency gates

- `cargo audit --deny warnings` — pass: no advisories affect the
  workspace.
- `cargo deny check advisories licenses bans sources` — pass: the new
  `sha2 = "0.10"` dependency (RustCrypto, dual MIT/Apache-2.0)
  satisfies the workspace license policy; only the expected
  multiple-version warning is reported for `toml` (workspace-wide,
  pre-existing in M025).

### Hosted gates

- CI run on Ubuntu / macOS / Windows against `e0507d1` is pending;
  the local fast-gate suite is the qualifier for M026 since the plan
  notes security/dependency checks "become mandatory again in M028".

## Invariants and unresolved findings

- `compile_schedule` is pure for a fixed `(source,
  COMPILER_SEMANTICS_VERSION)`. Same inputs → byte-identical
  `CompiledScenarioV2` and fingerprint.
- `run_id` cannot influence v2 output (compiler is pure and does
  not see it; the v2 namespace helper does not see it).
- Same-deadline events have stable source/expansion order.
- Expansion is bounded before run creation; the ceiling reject
  cannot fire mid-compile.
- Stream and datagram actions remain distinct enums; the v2 source
  cannot collapse them.
- The TOML/JSON parser edges reject unknown fields and never round
  or overflow durations.
- ScenarioV1 wire shape and run lifecycle are unchanged.
- v1 `derive_policy_seed` golden vectors remain byte-identical.

No unresolved medium-or-higher finding remains for this milestone.

## Acceptance verdict and planning transition

M026 closes cleanly. `plans/registry.md` moves M026 from `ready` to
`closed`. M027 moves from `blocked` to `ready` because its
prerequisite compile surface, fingerprint contract, and v2 namespace
helper are now frozen.

Activation rationale for M027: the implementation can now wire the
compiled `CompiledScenarioV2` into the owned scenario supervisor with
the documented epoch + sleep_until semantics, the v2 namespace
helper, and the run-evidence extension. M027's WP5 native API
surface is unblocked because the v2 wire DTOs exist with version-aware
JSON and TOML forms. M028 qualification work is unblocked in turn
because the canonical fingerprint is the comparator.

No new dependency was added beyond `sha2` for the fingerprint
hash, which is a RustCrypto crate already licensed for use under the
workspace's deny.toml policy. `unsafe_code = "forbid"` remains.
The four touched crates (core, server, fuzz, CLI scripts) compile
under the pinned 1.89 toolchain without warnings.

## Limitations carried forward

- v2 schedules cannot start a run via `ControlState` yet; the runtime
  apply path lands in M027.
- No admin route or CLI subcommand applies a v2 schedule yet; the
  wire DTOs are reusable but untouched by `admin.rs` until M027.
- The v2 fuzz corpus is started in M026 but the larger bounded
  schedule corpus, paused-time scheduler timing corpus, and full
  concurrent-mutation matrix are qualification evidence for M028.
- The TOML boundary accepts integer nanoseconds only; a
  human-duration extension is a separate plan.
