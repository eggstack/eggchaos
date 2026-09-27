# M049 — Core Deserialization and Plan-Invariant Hardening Corrective

Status: ready
Depends on: M048 closed
Role: first implementation milestone in the post-M048 maintenance/correctness tranche
Registration baseline: 4febd52e60f8098e9adfffa876052831fb2be56a

## Objective

Make eggchaos-core's documented stream-plan invariants true for every safe Rust construction path, including Serde deserialization, without changing the serialized representation, public symbol names, fault semantics, deterministic RNG contract, or any existing HTTP/config compatibility surface.

The audit baseline found a structural asymmetry:

- FaultId is described and constructed as a validated 1..=128-byte identity, but derived Deserialize can populate the private inner String without calling FaultId::new.
- Probability is described and constructed as finite and within [0, 1], but derived Deserialize can populate the private f64 without calling Probability::new.
- FaultPlan::new rejects duplicate IDs, but derived Deserialize can create a plan without passing through FaultPlan::new.
- FaultPlan::validate currently rechecks individual fault kinds but is not a complete reconstruction of all constructor invariants.
- LivePolicy publication calls plan validation, so validation must be an actual invariant authority rather than a partial second implementation.
- DatagramPlan already follows the stronger pattern: deserialized values are explicitly revalidated before publication.

M049 must make stream-plan behavior at least as strong as the existing datagram-plan behavior while preserving every valid serialized document byte-for-byte at the semantic level.

## Baseline and constraints

The registration baseline is 4febd52e60f8098e9adfffa876052831fb2be56a, after M048 closed the hosted performance-provenance line.

This is a correctness and invariant-boundary repair. It is not authorization to redesign the fault model.

The following are frozen:

- FaultKind variants and their meanings;
- STREAM_LOSS_GRAIN_BYTES and all stream-loss RNG/evidence semantics;
- SplitMix64-v1 and all seed derivation;
- FaultId and Probability public constructors/accessors;
- FaultPlan public constructors and mutation methods;
- LivePolicy public signatures, including infallible LivePolicy::new;
- native /v1 DTO spellings, OpenAPI shape, TOML configuration shape, CLI behavior, Toxiproxy mappings, SDK behavior, and Python-native behavior;
- the seven-slot legacy activation-array contract.

## Scope

### In scope

- custom or constructor-routing Deserialization for FaultId and Probability;
- one complete canonical FaultPlan invariant check covering duplicate IDs and every invariant reachable through safe deserialization;
- making FaultPlan::new, FaultPlan::validate, with_fault, replace_fault, and publication paths agree on the same invariant set;
- explicit regression tests for invalid core JSON/Serde construction;
- valid round-trip tests proving representation compatibility;
- focused property/fuzz coverage for malformed IDs, probabilities, duplicate IDs, and fault-kind bounds;
- comparison against DatagramPlan's revalidation model to avoid two different definitions of "validated plan";
- documentation corrections where current comments imply stronger guarantees than the implementation actually provided.

### Non-goals

- no new fault type;
- no fault-semantic change;
- no public-field removal or constructor signature change;
- no new wire field or stricter native HTTP/TOML schema beyond invariants already documented by the core types;
- no RNG or replay-identity change;
- no conversion of Duration or NonZeroU64 representation;
- no change to datagram semantics unless a directly analogous invariant bug is found while proving parity;
- no panic-based validation path;
- no performance optimization work.

## Affected surfaces

Expected primary files:

- crates/eggchaos-core/src/plan.rs;
- crates/eggchaos-core/src/policy.rs only if publication tests require a local assertion or comment clarification;
- crates/eggchaos-core/tests/ and/or plan-local tests;
- fuzz/fuzz_targets/plan_json.rs if the existing target does not already cover the invalid-construction cases;
- architecture/core-fault-engine.md;
- architecture/verification-qualification.md;
- plans/registry.md and closure evidence at completion.

Protocol, server, CLI, compatibility, SDK, and binding code should change only if qualification exposes an actual dependency on the invalid state.

## Ordered work packages

### WP1 — Freeze the valid representation contract

Before editing Serde behavior:

1. record representative valid serialized forms for FaultId, Probability, FaultSpec, every FaultKind, and FaultPlan;
2. verify round-trip equality on the existing baseline;
3. include empty and multi-fault plans, stream-loss, and boundary probability values 0 and 1;
4. confirm that no existing native/config DTO relies on directly deserializing invalid core values and repairing them later.

The implementation must reject invalid states without changing valid field names, enum tags, nesting, or units.

### WP2 — Route scalar deserialization through constructors

Implement invariant-preserving Deserialize behavior for FaultId and Probability.

Requirements:

- FaultId deserialization must call the same validation authority as FaultId::new;
- Probability deserialization must reject NaN, infinities, and values outside [0, 1] using the same invariant as Probability::new;
- Serialize output remains unchanged;
- error strings may become clearer but must remain bounded and must not expose unrelated input;
- no unsafe code.

### WP3 — Make FaultPlan validation complete

Create one complete plan-level validation path.

It must cover at least:

- duplicate fault IDs;
- every FaultSpec invariant;
- probability validity defensively, even if scalar deserialization now prevents invalid construction;
- every existing kind-specific bound;
- preservation of order.

FaultPlan::new and FaultPlan::validate must agree. Mutation helpers must not maintain separate partial rules.

If custom FaultPlan Deserialize is used, deserialize through a temporary representation and then pass through the canonical constructor/validator. If a Serde try_from pattern is cleaner, it must preserve the current wire representation.

### WP4 — Prove publication cannot accept an invalid safe-Rust plan

Exercise LivePolicy::new, publish, and publish_expected with the valid plan states that remain constructible through public safe APIs.

Do not change LivePolicy::new to return Result; that would regress the public API. The type invariant should make invalid safe construction impossible rather than moving a new fallible boundary into existing callers.

Add tests showing that invalid serialized plans fail before they can become a published policy.

### WP5 — Fuzz/property and cross-surface regression

Extend the existing plan-json fuzz/property corpus so malformed scalar values and duplicate IDs are explicit targets rather than incidental cases.

Then verify that native DTO conversion continues to accept every valid case it accepted before and reject the same invalid cases or earlier.

## Invariants and failure semantics

- A successfully created or deserialized FaultId is always 1..=128 bytes.
- A successfully created or deserialized Probability is finite and within [0, 1].
- A successfully created or deserialized FaultPlan contains unique IDs and valid fault specs.
- Validation failure is returned as an ordinary error; never panic.
- Valid serialized representations remain compatible.
- No publication path silently normalizes invalid data.
- No deterministic decision changes for valid plans.

## Required verification

At minimum:

    cargo test -p eggchaos-core --all-features
    cargo test -p eggchaos-protocol --all-features
    cargo test --workspace --all-features
    ./scripts/check_openapi.sh
    ./scripts/check.sh

Run the plan_json fuzz target for a bounded qualification interval on the implementation candidate. If the pinned cargo-fuzz tooling constraint applies, follow the existing AGENTS.md release guidance rather than changing the pinned workspace toolchain.

## Acceptance criteria

M049 may close only when:

1. invalid FaultId and Probability values cannot be created through Serde;
2. duplicate-ID FaultPlan documents are rejected at deserialization or canonical validation;
3. FaultPlan::new and FaultPlan::validate agree on the same invariant corpus;
4. valid pre-M049 serialization round trips unchanged in shape and meaning;
5. LivePolicy public signatures are unchanged;
6. all core/protocol/workspace/OpenAPI checks pass;
7. bounded fuzz evidence is green;
8. no public symbol, fault behavior, RNG identity, or compatibility surface is removed or renamed;
9. exact-candidate closure evidence is recorded.

## Rejection / stop conditions

Stop and re-plan if the repair requires:

- changing the public serialized shape of valid core types;
- changing a public function signature;
- changing fault or RNG semantics;
- accepting a wire/config compatibility regression;
- introducing a second plan-validation authority.

A newly discovered invalid state in DatagramPlan may be repaired in the same milestone only when the fix is directly analogous and semantics-preserving; otherwise register a separate corrective.

## Closure evidence

Create plans/closure/M049-core-deserialization-and-plan-invariant-hardening-corrective-closure.md containing:

- exact candidate SHA;
- tests/fuzz commands and results;
- before/after invalid-construction corpus;
- confirmation that valid serialization shapes are unchanged;
- explicit public-API and deterministic-semantics non-regression statement.

## Successor activation

Closing M049 activates M050. M050 must not begin from source inspection alone; use the exact M049 closure candidate as its implementation baseline.
