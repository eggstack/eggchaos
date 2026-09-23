# M017 — Native Control Contract and Operator-Surface Consolidation

Status: blocked
Depends on: M016
Role: native API/config/CLI hardening

## Objective

Remove duplicated native fault/proxy wire representations and bring the schema-v1 configuration and official CLI into deliberate alignment with the runtime/control surface.

The primary maintenance problem is not alternate runtime state; `ControlState` is already authoritative. The problem is that native TOML, native HTTP, and CLI currently encode parts of the same model independently. In particular the CLI hand-builds Serde JSON matching the internal `FaultKind` enum representation, while native create/update proxy fields already use inconsistent timeout spellings.

M017 creates an explicit versioned native control DTO boundary and makes configuration/CLI translators target that boundary instead of depending on incidental internal Serde layout.

## User-visible outcome

After M017:

- native `/v1` request/response schemas are explicit types rather than accidental serialization of runtime structs;
- proxy create/update timeout naming and fault-kind wire shapes are internally consistent and documented;
- CLI/config use shared semantic conversion rules for native fault input;
- the CLI exposes the already-supported scenario/history/metrics operations;
- schema-v1 TOML can configure the important existing runtime bounds that were previously hard-coded or unreachable;
- `eggchaos-core` remains protocol-neutral and unchanged in responsibility.

## Baseline

M016 must be closed first so this plan starts from panic-free, authenticated, secret-redacted, identifier-safe control plumbing.

Current duplication to eliminate:

- `config.rs` string type/field matching -> `FaultKind`;
- `eggchaos-cli/src/main.rs::build_kind` -> manually constructed `serde_json::Value` matching internal enum tags;
- native admin routes deserialize runtime types such as `ProxySpec`/`FaultUpsert` directly;
- runtime fields such as global connection/history limits, relay buffer, termination grace, proxy connect timeout/seed, and latency buffer capacity are only partly represented in TOML/CLI.

Toxiproxy remains a separate compatibility vocabulary and may keep its dedicated translation layer.

## Scope

### In scope

- Explicit native API DTOs for proxy/fault create/update and scenario/control responses where needed.
- A stable documented fault wire representation that does not depend on Rust enum variant serialization.
- One semantic native fault-input compiler/conversion path reused by TOML and HTTP/CLI-facing code where practical.
- Schema-v1 TOML exposure for currently implemented runtime tuning that is safe and useful.
- CLI coverage for existing native scenario/history/metrics routes.
- Exact JSON/TOML round-trip and compatibility tests.

### Non-goals

- No new runtime state authority.
- No new transport protocols or fault semantics.
- No arbitrary plugin system.
- No Toxiproxy v2.12 behavior changes except mechanical adaptation to native DTO internals.
- No runtime.rs decomposition in this milestone; M018 owns that source-organization work.
- No Eggbench/EggReplay/Eggprobe feature absorption.

## Affected surfaces

Expected files/modules:

- `crates/eggchaos-server/src/admin.rs`
- `crates/eggchaos-server/src/config.rs`
- a new focused native DTO/conversion module if useful
- `crates/eggchaos-server/src/runtime.rs` only for narrow constructors/adapters
- `crates/eggchaos-cli/src/main.rs` and CLI E2E tests
- `docs/configuration.md`
- `docs/control-plane.md`
- `README.md` examples if syntax changes
- Toxiproxy adapter tests only as regression evidence

## Contract decisions

The implementation must make these choices explicit in code/docs rather than leaving them to derived Serde defaults:

1. Native request DTOs are versioned by the `/v1` route family and may evolve independently from internal Rust structs.
2. Fault wire type names use stable lowercase/kebab spellings (for example `latency`, `bandwidth`, `blackhole`, `limit-data`, `slow-close`, `slice`, `disconnect`) with explicit typed attributes.
3. Durations on the native HTTP JSON surface use one documented unit/shape. Avoid a mixture of Rust `Duration` object serialization and ad-hoc `*_ms` fields.
4. Create and patch operations should use the same canonical field vocabulary wherever semantics are the same.
5. Internal `FaultKind` remains the typed execution model; DTO conversion validates before publication.
6. Toxiproxy's oracle-specific shape stays isolated in `eggchaos-toxiproxy`.

Because no `v0.1.0` has been tagged, a deliberate pre-release native schema cleanup is allowed. Any intentional compatibility alias retained for the current unpublished shape must be documented and tested rather than accidental.

## Ordered work packages

### WP1 — Introduce explicit native DTOs

Define request/response types for native proxy and fault mutation instead of deserializing runtime structs directly.

At minimum cover:

- proxy create;
- proxy patch;
- fault create;
- fault patch;
- stable error envelope remains unchanged;
- scenario apply/get/cancel response types where current anonymous JSON obscures the contract.

Keep DTOs free of listener handles, `LivePolicy`, or other runtime-only fields.

### WP2 — Consolidate native fault conversion

Replace independent CLI JSON construction and TOML kind matching with one shared semantic vocabulary/conversion layer.

Requirements:

- each release-baseline fault has one native attribute schema;
- validation errors name the offending field;
- aliases such as `slice`/`slicer` or `limit-data`/`limit_data` are either deliberately accepted at the configuration parser edge or removed/documented;
- defaults are defined in one place per native schema;
- Toxiproxy conversion remains separate because its defaults/units intentionally differ.

### WP3 — Normalize proxy/control fields

Resolve current create-vs-patch asymmetries such as `connect_timeout` versus `connect_timeout_ms`.

Choose one documented native API vocabulary and apply it consistently to:

- create;
- patch;
- response/view where practical;
- CLI payload generation;
- docs/tests.

Do not silently reinterpret units.

### WP4 — Expose existing runtime bounds in schema-v1 TOML

Add configuration fields for the useful implemented runtime controls that are currently inaccessible, with bounded validation.

Target set:

- global active connection limit;
- retained history limit;
- relay buffer size;
- graceful termination drain timeout;
- per-proxy connect timeout;
- per-proxy deterministic seed;
- latency max-buffer bytes.

Half-close policy may be exposed if its existing enum has a stable user-facing contract; otherwise explicitly defer it rather than creating a poorly specified spelling.

All defaults must preserve current runtime defaults when fields are omitted.

### WP5 — Complete the official CLI over existing native routes

Add thin commands for already-existing server capabilities:

- scenario apply;
- scenario get;
- scenario cancel;
- history;
- metrics.

The CLI must remain an HTTP adapter; no direct `ControlState` mutation path may be introduced.

Machine mode continues to emit exactly one JSON document for JSON routes. For Prometheus metrics, either provide raw text in human mode plus an explicit machine wrapper, or document a separate raw-output contract; do not pretend Prometheus text is JSON.

### WP6 — Contract documentation and migration notes

Document the exact native v1 JSON schemas and configuration field units. If an unpublished earlier field spelling is removed, note it in a pre-release migration section so repository examples/tests cannot drift.

## Behavioral invariants

- `eggchaos-core` has no HTTP/TOML/CLI knowledge.
- `ControlState` remains the sole mutation authority.
- DTO conversion validates before runtime publication.
- Existing fault execution/determinism semantics do not change.
- Omitted new TOML fields preserve current defaults.
- Machine output is stable and bounded.
- Secrets remain redacted under M016 rules.
- No CLI command maintains local shadow state.

## Verification

Minimum:

```sh
cargo test -p eggchaos-server --all-features
cargo test -p eggchaos-cli --all-features
cargo test -p eggchaos-toxiproxy --all-features
cargo test -p eggchaos-eggfetch --all-features
./scripts/check.sh
```

Required contract tests:

- exact JSON fixtures for each native fault DTO;
- create and patch use the same canonical timeout/unit vocabulary;
- TOML -> typed runtime model covers every newly exposed bound;
- omitted TOML values reproduce pre-M017 defaults;
- CLI scenario apply/get/cancel E2E;
- CLI history E2E;
- metrics command behavior is explicit and tested;
- old internal Serde layout changes cannot silently change the native HTTP contract.

## Acceptance criteria

M017 closes when the native wire contract is explicit, CLI/config no longer independently mirror internal `FaultKind` Serde layout, the selected runtime controls are configurable, existing server capabilities have official CLI coverage, all relevant docs/fixtures are aligned, and full checks pass.

Closure evidence must identify any deliberately retained compatibility aliases and the exact JSON/TOML fixtures used as the v1 contract.

## Stop/rejection conditions

Do not close if:

- the native API still directly accepts runtime-only structs as its public mutation schema;
- CLI fault payloads still depend on internal Rust enum serialization;
- configuration and API use undocumented conflicting units;
- adding operator fields changes defaults for existing configs;
- Toxiproxy oracle behavior regresses;
- the change moves protocol concerns into `eggchaos-core`.

If the desired native schema cannot be changed without a meaningful compatibility decision, write an ADR and keep M017 blocked until that decision is explicit.

## Follow-on activation

On clean closure: M018 becomes `ready`.
