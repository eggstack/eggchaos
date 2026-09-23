# M018 — Runtime Modularization and Dependency Hygiene

Status: closed
Depends on: M017
Role: maintenance/structural hardening

## Objective

Reduce maintenance concentration in `eggchaos-server` without changing runtime authority or observable behavior.

At the audit baseline, `crates/eggchaos-server/src/runtime.rs` is approximately 4,758 lines / 184 KiB and owns domain views, metrics, connection registries, listener supervision, proxy CRUD, admission, relay execution, termination mapping, history, and tests. The single `RuntimeInner` / `ControlState` authority is correct; the source-file concentration is not required to preserve that invariant.

This milestone decomposes the implementation along cohesive internal boundaries and removes direct dependencies that are no longer used after EggServe encapsulated the HTTP stack.

## User-visible outcome

There should be no intentional user-visible semantic change. The benefit is maintainability:

- smaller reviewable modules;
- clearer ownership of lifecycle, connection evidence, metrics, and transport termination;
- fewer direct dependencies;
- preserved test and release behavior.

## Scope

### In scope

- Split `runtime.rs` into cohesive private/public modules while preserving the exported API.
- Keep one `RuntimeInner` and one `ControlState` mutation authority.
- Move tests with the implementation area they verify where practical.
- Remove unused normal dependencies or move test-only dependencies to `dev-dependencies`.
- Update architecture documentation/module maps.

### Non-goals

- No API schema change beyond mechanical paths caused by M017.
- No new features.
- No fault algorithm refactor.
- No replacement of `eggress-relay`, EggServe, or Eggfetch.
- No metrics semantic redesign.
- No switch to a new async runtime.
- No generic framework extraction solely to reduce line count.

## Target module boundaries

Exact filenames may vary, but the decomposition should approximately isolate:

- `runtime/control.rs` — `ControlState` proxy/fault mutation and publication orchestration;
- `runtime/connection.rs` — connection snapshots, registries, close history, evidence merge/finalization;
- `runtime/supervisor.rs` — proxy listener lifecycle, admission, task ownership, shutdown/join;
- `runtime/transport.rs` — `ResettableTcpStream`, concrete hard-reset capability, relay termination mapping;
- `runtime/metrics.rs` — counters/tables and Prometheus rendering;
- `runtime/model.rs` — shared runtime-facing structs/enums if this reduces cyclic imports;
- a thin `runtime/mod.rs` — `RuntimeInner` composition and public re-exports.

Do not force a split that creates circular abstractions or excessive visibility. Cohesion and authority are the goal, not a specific file count.

## Ordered work packages

### WP1 — Freeze behavior and public surface

Before moving code, capture:

- public exports from `eggchaos-server/src/lib.rs`;
- native API fixtures from M017;
- connection outcome/metrics tests;
- listener lifecycle/restart tests;
- hard-reset/graceful termination tests.

No behavior change should be mixed into the initial move unless a test exposes an existing defect, in which case stop and register that defect explicitly.

### WP2 — Extract pure/domain and metrics pieces

Move low-coupling data structures and metric tables/rendering first. Keep visibility as narrow as possible.

Metric names, labels, cardinality bounds, and values must remain byte/semantically compatible with M017 fixtures except for ordering where the contract already permits it.

### WP3 — Extract transport and connection lifecycle

Move reset-capability wrappers, termination resolution, connection evidence merge, and connection finalization into focused modules.

Preserve:

- level-triggered termination;
- connection outcome classification;
- cancellation behavior during upstream connect;
- exact accepted/forwarded/discarded accounting;
- platform-qualified hard-reset reporting.

### WP4 — Extract supervisor/control implementation

Move listener/task ownership and `ControlState` mutation methods last, after the lower-level modules are stable.

Preserve:

- bind-before-visible-success;
- replacement pre-bind behavior;
- no detached unfinished supervisor;
- authoritative live-policy snapshots;
- generation/CAS behavior;
- disable/delete shutdown semantics;
- bounded histories and scenario ownership.

### WP5 — Dependency hygiene

Run a direct-use census against `eggchaos-server/Cargo.toml`.

At the audit baseline, normal dependencies such as `hyper`, `hyper-util`, `http-body-util`, `http`, `bytes`, and `prometheus-client` appeared unused by production server sources after the EggServe migration/manual metrics renderer. Remove truly unused dependencies, or move them to `dev-dependencies` only if a test directly requires them.

Do not remove a dependency merely because use is hidden behind a feature/module without verifying all-target/all-feature builds.

### WP6 — Documentation/module index

Update:

- `architecture/server-runtime.md`;
- `architecture/overview.md`;
- `architecture/tooling-distribution.md` if dependency inventory changes;
- `AGENTS.md` layout hints if module paths materially change.

## Behavioral invariants

- Exactly one runtime mutation/state authority remains.
- No public listener/connection task is detached.
- No change to deterministic seed derivation or fault semantics.
- No new lock-order inversion or lock held across network I/O.
- No reduction in bounds.
- No change to admin bind/auth behavior.
- No change to Toxiproxy compatibility or Eggfetch integration.
- `unsafe_code = "forbid"` remains true.

## Verification

Minimum:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
cargo audit --deny warnings
cargo deny check advisories licenses bans sources
./scripts/qualify_eggfetch.sh
```

Also run focused server lifecycle/control/metrics tests after each extraction rather than waiting until the end.

Use `cargo tree --locked` before/after and record removed direct dependencies in the closure note.

## Acceptance criteria

M018 closes when `runtime.rs` is no longer the monolithic implementation surface, the intended internal boundaries are represented by cohesive modules, public behavior/API fixtures remain unchanged from M017, unused direct dependencies are removed or justified, and the full workspace gate is green.

The closure note must include a before/after module/dependency census and identify any files intentionally left large because further splitting would reduce cohesion.

## Stop/rejection conditions

Do not close if:

- state is duplicated across new modules;
- public API paths change unnecessarily;
- test coverage is deleted to simplify moves;
- lifecycle behavior changes without an explicit corrective plan;
- new module boundaries require broad `pub` exposure of formerly internal details;
- the dependency cleanup breaks supported feature profiles.

## Follow-on activation

On clean closure: M019 becomes `ready`.
