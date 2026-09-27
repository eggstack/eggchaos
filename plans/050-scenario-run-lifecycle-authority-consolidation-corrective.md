# M050 — Scenario Run Lifecycle Authority Consolidation Corrective

Status: blocked
Depends on: M049 closed
Role: scenario bookkeeping consolidation without Scenario V1/V2 semantic convergence
Activation baseline: exact M049 closure candidate

## Objective

Replace duplicated Scenario V1 and Scenario V2 run-registry/token/admission/pruning bookkeeping with one internal lifecycle authority while preserving both scenario semantic models, all existing public methods, run-ID behavior, shutdown ownership, and the current effective per-family capacity.

The audit baseline shows that V1 and V2 already share:

- one global next_run_id allocator;
- one supervised scenario JoinSet;
- the service shutdown token lineage.

They nevertheless maintain separate run maps, token maps, and nearly identical active-count/pruning/cancel/update logic.

This is maintenance overlap, not justification to merge the two scenario languages.

## Compatibility boundary

Scenario V1 and Scenario V2 remain semantically distinct.

M050 must not:

- compile V1 documents into V2 schedules;
- change V1 namespace derivation from (scenario seed, run id, event index);
- change V2 namespace derivation from (scenario seed, execution key, fingerprint, compiled event index);
- alter V2 strict/live isolation or cleanup semantics;
- alter V1 event timing semantics;
- change the JSON/TOML DTOs;
- change run record shapes returned by existing methods/routes;
- add or remove a native route.

The current runtime can retain/admit up to 32 V1 records and 32 V2 records independently. Consolidation must preserve that effective capability. Do not accidentally reinterpret MAX_SCENARIO_RUNS as 32 total.

## Scope

### In scope

- one private scenario lifecycle registry keyed by the existing globally unique run ID;
- a family-tagged internal entry enum or equivalent;
- one token registry keyed by run ID;
- shared insertion/admission/pruning helpers;
- shared cancellation-token removal;
- preservation of the single JoinSet supervisor;
- existing V1 and V2 public get/cancel/update methods retained as wrappers over the unified authority;
- explicit mixed-family capacity/pruning tests;
- shutdown/join regression coverage.

### Non-goals

- no Scenario V3;
- no scenario enumeration/list API;
- no wire DTO change;
- no new metrics;
- no scheduler/compiler change;
- no run-ID reset or renumbering;
- no global 32-run cap;
- no removal of Scenario V1 compatibility;
- no cross-family record coercion in public responses.

## Affected surfaces

Expected:

- crates/eggchaos-server/src/runtime/mod.rs;
- crates/eggchaos-server/src/runtime/control.rs;
- a new private runtime/scenario_registry.rs or equivalent;
- crates/eggchaos-server/src/scenario.rs only if a narrow helper is needed;
- crates/eggchaos-server/src/scenario_v2/runtime.rs only if a narrow helper is needed;
- runtime/scenario tests;
- architecture/scenario-observability.md;
- architecture/server-runtime.md.

No protocol/OpenAPI/SDK changes should be required.

## Ordered work packages

### WP1 — Characterize the existing lifecycle contract

Freeze tests for:

- shared monotonic run IDs across interleaved V1/V2 starts;
- 32 active V1 runs accepted and the 33rd rejected;
- 32 active V2 runs accepted and the 33rd rejected;
- simultaneous presence of 32 V1 plus 32 V2 runs;
- pruning only finished records in the relevant family when that family reaches retention capacity;
- get/cancel behavior for known, finished, active, and unknown IDs;
- token removal after completion;
- service shutdown cancelling and joining both families.

These tests are the non-regression oracle for the refactor.

### WP2 — Introduce one internal registry

Create a private registry abstraction that owns:

- family-tagged run records;
- cancellation tokens;
- family-specific retained/active counts;
- insertion;
- family-preserving oldest-finished pruning;
- lookup and mutation by run ID;
- token removal.

The registry may store heterogeneous record shapes through an internal enum. Public record types stay unchanged.

### WP3 — Migrate V1 lifecycle calls

Move V1 start/get/cancel/update/token-removal bookkeeping to the registry while leaving drive_scenario_run unchanged except for narrow registry callbacks.

Prove V1 behavior against the WP1 baseline before migrating V2.

### WP4 — Migrate V2 lifecycle calls

Move V2 start/get/cancel/update/token-removal bookkeeping to the same registry.

Keep the existing V2 compiler, fingerprinting, epoch gate, isolation, cleanup, event accounting, and driver semantics untouched.

### WP5 — Remove duplicate lifecycle stores

After both families use the shared registry:

- remove the duplicate maps/token maps;
- keep next_run_id and scenario_tasks as the existing single authorities;
- ensure no hidden second store remains in admin/embed layers;
- update comments/docs that currently describe separate maps.

## Capacity and pruning semantics

The consolidated implementation must preserve:

- maximum 32 retained/active V1 entries under the existing rule;
- maximum 32 retained/active V2 entries under the existing rule;
- ability to hold both families concurrently;
- family-local pruning order;
- global uniqueness/monotonicity of run IDs.

If a future milestone wants a genuinely global scenario quota, that is a separate behavior change and must be planned explicitly.

## Required verification

At minimum:

    cargo test -p eggchaos-server --all-features
    cargo test -p eggchaos-experiment --all-features
    cargo test -p eggchaos-protocol --all-features
    cargo test --workspace --all-features
    ./scripts/check_openapi.sh
    ./scripts/check.sh

Use paused Tokio time where lifecycle tests depend on schedule timing. Avoid wall-clock sleeps as sole evidence.

## Acceptance criteria

M050 may close only when:

1. one internal scenario lifecycle registry owns V1/V2 records and tokens;
2. the single next_run_id allocator and JoinSet remain the only ID/task authorities;
3. existing public V1/V2 methods retain their signatures and record shapes;
4. mixed-family 32+32 effective capacity is preserved;
5. family-local pruning and active admission behavior are proven;
6. shutdown joins/cancels both families with no detached task;
7. Scenario V1 and V2 deterministic semantics are unchanged;
8. OpenAPI and all existing client surfaces remain unchanged;
9. exact-candidate closure evidence is recorded.

## Rejection / stop conditions

Stop and re-plan if consolidation requires:

- translating V1 into V2;
- reducing current capacity;
- changing existing run response shapes;
- changing deterministic identity;
- adding a new external API solely to make the refactor easier.

## Closure evidence

Create plans/closure/M050-scenario-run-lifecycle-authority-consolidation-corrective-closure.md with exact candidate, mixed-family capacity evidence, lifecycle race/shutdown test results, and an explicit statement that V1/V2 semantic authorities remain separate.

## Successor activation

Closing M050 activates M051.
