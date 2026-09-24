# M026 — Deterministic Scenario Schedule Model and Compiler

Status: ready
Depends on: M025, ADR 004
Role: post-release scenario-v2 semantic/compiler foundation

## Objective

Implement the pure, bounded ScenarioScheduleV2 model and deterministic compiler defined by ADR 004. The result is an inspectable CompiledScenarioV2 event tape with stable replay identity, but no new HTTP route or live schedule execution path yet.

M026 must make the higher-level language deterministic before M027 wires it to ControlState. This milestone is primarily about source semantics, expansion, identity, validation, and golden fixtures.

## Baseline and dependencies

M011 corrected the existing ScenarioV1 driver, generation publication, run supervision, and evidence. M022 added transport-explicit datagram scenario actions. M025 leaves the repository with no active successor and does not alter scenario semantics.

Current v1 strengths to reuse:

- bounded 1024-event documents;
- explicit stream versus datagram actions;
- FaultPlan and DatagramPlan validation authorities;
- versioned deterministic seed helpers and golden-vector discipline;
- one ControlState authority for policy publication;
- no payload capture.

Current v1 limitations motivating this milestone:

- no named phases or finite-repeat source language;
- no canonical compile artifact/fingerprint;
- v1 seed namespaces include runtime run_id;
- no source-level isolation/cleanup policy;
- no compile-time expansion guard because no expansion exists.

ADR 004 is the semantic authority for v2. ADR 002 and ADR 003 remain unchanged for stream/datagram data-plane behavior.

## Scope

### In scope

- ScenarioScheduleV2 source model with version, seed, execution_key, isolation, cleanup, and bounded phase/step structure.
- Named sequential phases with explicit duration and ordered existing scenario actions at phase boundaries.
- Finite repetition of bounded phase groups.
- Deterministic expansion to CompiledScenarioV2 with absolute monotonic offsets and stable same-offset order.
- Maximum 1024 compiled events and explicit bounded nesting/repetition validation.
- Checked duration/offset arithmetic.
- Canonical schedule semantics representation and SHA-256 fingerprint.
- V2 schedule-specific policy namespace derivation independent of runtime run_id.
- Golden vectors for fingerprint and v2 namespace derivation.
- Pure JSON/TOML DTO parsing/round-trip fixtures where the source format is public.
- Fuzz/property coverage for expansion, arithmetic, malformed input, and determinism.
- Documentation of source and compiled semantics sufficient for M027 handoff.

### Non-goals

- No ControlState publication from v2 schedules.
- No new admin routes or CLI commands.
- No changes to ScenarioV1 wire shape.
- No proxy enable/disable, connection/association kill, shell, callback, predicate, HTTP hook, cron, or arbitrary-code actions.
- No continuous fault interpolation inside stream/datagram engines.
- No new stream or datagram fault kind.
- No change to ADR 002 stream mutation or ADR 003 datagram admission semantics.
- No unbounded or data-dependent loop construct.

## Affected surfaces

- crates/eggchaos-server/src/scenario.rs, preferably decomposed into a scenario module if needed for cohesion.
- crates/eggchaos-server/src/native.rs for DTO definitions/conversion only; route wiring waits for M027.
- crates/eggchaos-core/src/rng.rs only if the stable v2 namespace helper belongs beside existing derivation helpers; existing golden vectors must remain unchanged.
- fuzz/ for schedule parsing/expansion targets.
- architecture/scenario-observability.md and docs/control-plane.md for explicitly planned/implemented compiler semantics.
- plans/adrs/004-deterministic-scenario-schedules-and-replay-identity.md is the authority.

## Required model

Exact Rust names may vary, but the implementation must preserve concepts equivalent to:

    ScenarioScheduleV2
      version = 2
      seed: u64
      execution_key: u64
      isolation: strict | live
      cleanup: restore-initial | leave
      steps/phases: bounded sequence

    SchedulePhaseV2
      optional bounded name
      duration
      ordered actions

    CompiledScenarioV2
      compiler_semantics_version
      schedule_fingerprint
      seed
      execution_key
      isolation
      cleanup
      ordered CompiledEventV2[]

    CompiledEventV2
      compiled_index
      optional phase identity
      offset
      transport-explicit scenario action

The compiler output is immutable after validation.

## Source semantics

A phase applies its actions at the phase start in source order, then advances the schedule cursor by its duration. Equal-offset events therefore have deterministic order by compiled_index.

Finite repetition expands structurally in source order. Expansion must stop with a validation error before allocating/executing beyond the compiled-event ceiling. The implementation must not partially compile and then truncate.

Durations use one exact internal representation. Prefer nanosecond Duration-compatible integers in the explicit native DTO and human duration strings only at the TOML parser edge. Overflow when summing durations is an error.

Empty schedules are valid only if explicitly useful and documented; otherwise reject them consistently. Zero-duration phases are permitted only if their action order remains deterministic and bounds prevent degenerate expansion.

## Canonical fingerprint

M026 must freeze one canonical fingerprint contract. Required properties:

- SHA-256 with an explicit domain/version prefix;
- semantic, not source-text, identity: irrelevant whitespace/TOML formatting cannot change the digest;
- includes compiler semantics version, expanded event order/offset/action data, isolation/cleanup semantics, and any field that changes execution;
- excludes daemon run_id, wall-clock timestamps, source file path, comments, and presentation-only phase labels unless labels are declared execution evidence;
- no hash-map iteration order;
- committed golden fixtures.

If canonical serialization uses Serde, it must serialize a frozen map-free typed fingerprint representation with stable field ordering; do not hash arbitrary serde_json::Value maps.

## V2 namespace derivation

Add a domain-separated helper equivalent to:

    derive_schedule_policy_seed(
        scenario_seed,
        execution_key,
        schedule_fingerprint_identity,
        compiled_event_index
    )

The exact byte/folding contract is versioned and golden-tested. Runtime run_id must not participate. Existing derive_policy_seed v1 golden vectors and all stream/datagram fault-local RNG vectors must remain byte-for-byte unchanged.

## Ordered work packages

### WP1 — Freeze v2 DTO and bounds

Define source structs/enums, duration representation, phase/repeat structure, string/name bounds, nesting limit, compiled-event ceiling, default strict isolation, and default restore-initial cleanup.

### WP2 — Pure compiler

Implement validation plus deterministic flattening to absolute offsets and stable compiled indices. Compiler code must not access ControlState, Tokio wall time, sockets, or runtime run IDs.

### WP3 — Canonical fingerprint

Implement versioned canonical semantic encoding and SHA-256 fingerprinting. Add exact digest fixtures covering equivalent JSON/TOML source forms and semantically different schedules.

### WP4 — Portable seed namespace

Implement the v2 schedule namespace helper with a distinct domain/version and golden vectors. Prove runtime run_id independence and preserve all existing RNG vectors.

### WP5 — Source parsing and round trips

Define explicit JSON v2 DTOs and TOML authoring representation. TOML may use human duration strings, but both forms must compile to identical semantic IR/fingerprint.

### WP6 — Fuzz/property tests

Cover repeat/nesting expansion, duration overflow, duplicate/equal deadlines, transport-specific action validation, malformed fault IDs/probabilities, and deterministic compile output.

### WP7 — Architecture/docs handoff

Document v2 source versus compiled model, portable replay identity, bounds, and explicit non-goals. Do not describe runtime application as implemented until M027.

## Invariants and failure semantics

- Compilation is pure for a given source document and compiler semantics version.
- Same semantic source -> byte-identical canonical compiled representation and fingerprint.
- run_id can never change v2 compile output or v2 namespace vectors.
- Same-deadline ordering is source/expansion order, never hash-map order.
- Expansion is bounded before execution and fails atomically.
- Stream and datagram action types remain distinct.
- No source parser silently rounds/overflows durations.
- ScenarioV1 behavior and wire fixtures remain unchanged.

## Required tests

At minimum:

- JSON and TOML equivalents compile identically;
- same source compiled repeatedly produces the same event tape and digest;
- changing seed/execution_key behavior matches the documented fingerprint/namespace split;
- daemon/run ID is absent from v2 derivation and golden vectors;
- equal-offset actions preserve source order;
- nested finite repeats produce exact expected offsets/indices;
- expansion to exactly 1024 events succeeds and 1025 fails before run creation;
- duration addition overflow fails;
- semantically different action/offset/isolation/cleanup values change the fingerprint;
- existing v1 DTO fixtures and derive_policy_seed vectors remain unchanged;
- stream/datagram FaultPlan validation is reused rather than duplicated.

## Verification

Minimum:

    cargo fmt --all -- --check
    cargo clippy -p eggchaos-core -p eggchaos-server --all-targets --all-features -- -D warnings
    cargo test -p eggchaos-core --all-features
    cargo test -p eggchaos-server --all-features
    cargo test --workspace --all-features
    cargo doc --workspace --all-features --no-deps

Run focused fuzz/property targets added by this milestone. Security/dependency checks become mandatory again in M028, but any new hashing dependency must satisfy existing cargo-deny policy before M026 closure.

## Acceptance criteria

M026 closes only when:

- ADR 004 source/compiler concepts exist as bounded typed code;
- deterministic expansion produces a stable inspectable compiled tape;
- a versioned canonical SHA-256 schedule fingerprint is frozen with golden fixtures;
- v2 namespace derivation is independent of run_id and golden-tested;
- JSON/TOML source forms converge on the same compiler semantics;
- compiled expansion is capped at 1024 events with checked arithmetic;
- stream/datagram action separation is preserved;
- ScenarioV1 and all existing RNG vectors remain unchanged;
- required tests pass on one exact candidate;
- docs accurately distinguish implemented compiler support from M027 runtime work;
- closure evidence identifies the exact candidate and limitations.

Create plans/closure/M026-deterministic-scenario-schedule-model-and-compiler-closure.md.

## Stop/rejection conditions

Do not close if:

- compiler output depends on runtime run_id, Tokio scheduling, wall clock, source file path, or hash-map order;
- repeat expansion can allocate beyond a fixed bound;
- schedule compilation reaches into ControlState;
- v2 introduces a transport-generic fault representation that obscures stream/datagram differences;
- fingerprint changes under semantically equivalent JSON/TOML formatting;
- existing v1 wire/golden behavior is silently changed;
- canonical fingerprint/namespace rules are not frozen by executable fixtures.

## Follow-on activation

On clean M026 closure, M027 becomes ready. If implementation discovers that portable identity requires changing existing fault-local RNG semantics rather than only the published seed namespace, stop and amend ADR 004/ADR 002 before proceeding.