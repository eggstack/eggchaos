# M032 — Native Protocol Contract Extraction and OpenAPI Foundation

Status: ready  
Depends on: M031 (closed), ADR 006  
Role: cross-language contract foundation

## Objective

Extract the stable native `/v1` wire contract from `eggchaos-server` into a
narrow `eggchaos-protocol` crate, preserve existing Rust/server behavior
through compatibility re-exports and conversions, and add a mechanically
verified OpenAPI contract suitable for external SDK generation.

M032 does not add a foreign-language SDK or native FFI binding. Its purpose is
to make the contract those later surfaces consume explicit, publishable,
versioned, and regression-tested without coupling them to the listener/runtime
implementation.

## Baseline and dependencies

M017 made the native control plane explicit by introducing dedicated wire DTOs
instead of exposing internal Rust Serde layout. Those DTOs currently live in
`crates/eggchaos-server/src/native.rs` and `native_v2.rs`.

M026–M030 moved Scenario V2 semantic/compiler authority into
`eggchaos-experiment` while the server retained native DTO adapters and
compatibility re-exports.

M031 closed the integration-boundary tranche and qualified the current public
seams on exact candidate `fa189b9`.

ADR 006 now establishes that:

- `/v1` JSON is the primary cross-language contract;
- a narrow `eggchaos-protocol` crate should own native wire types;
- OpenAPI is generated from or mechanically checked against that authority;
- SDKs are downstream of this milestone;
- no generic C ABI is part of M032.

## Scope

### In scope

- Add publishable workspace crate `crates/eggchaos-protocol`.
- Move or reconstruct the explicit native V1 and Scenario V2 wire DTOs there.
- Preserve native JSON spellings, defaults, units, bounds, discriminators, and
  unknown-field rejection.
- Preserve source compatibility through `eggchaos-server` re-exports where
  practical.
- Keep runtime-only conversions and state authority in `eggchaos-server`.
- Allow protocol-to-core/experiment conversion helpers only where they preserve
  one semantic authority and do not pull in server/runtime dependencies.
- Freeze a stable native error-envelope model if it is not already represented
  as a reusable DTO.
- Add native route/operation metadata needed to describe the public API without
  importing the server runtime.
- Add `api/openapi/eggchaos-v1.yaml` as the checked-in native contract artifact.
- Cover all native stream, datagram, Scenario V1/V2, service, history, and
  metrics routes.
- Model bearer authentication and content types accurately.
- Add machine checks so protocol DTOs/server routes/OpenAPI cannot silently
  drift.
- Add JSON golden/round-trip fixtures for all discriminated request/response
  families.
- Update package/release-order proof for the new crate.
- Update architecture/control-plane docs to identify the new ownership
  boundary.

### Non-goals

- No Python/TypeScript SDK yet.
- No PyO3, maturin, Node-API, UniFFI, C ABI, JNI, P/Invoke, cgo, or WASM.
- No change to native route semantics for generator convenience.
- No new fault kind or scenario language feature.
- No control-plane version bump merely because DTO code moved.
- No removal of existing `eggchaos-server` public imports without a separately
  justified compatibility decision.
- No second server/router implementation inside `eggchaos-protocol`.
- No process/service-management API.

## Affected surfaces

Expected surfaces:

- workspace `Cargo.toml`;
- new `crates/eggchaos-protocol/`;
- `crates/eggchaos-server/src/native.rs`;
- `crates/eggchaos-server/src/native_v2.rs`;
- `crates/eggchaos-server/src/lib.rs`;
- `crates/eggchaos-server/src/admin.rs` for route-contract linkage only;
- `crates/eggchaos-cli` only if imports move;
- `api/openapi/eggchaos-v1.yaml`;
- contract fixtures under `qualification/` or a narrowly named test directory;
- release/package scripts;
- `docs/control-plane.md`;
- `docs/architecture.md`;
- `architecture/control-plane-cli.md`;
- `architecture/tooling-distribution.md`;
- `architecture/overview.md`;
- `AGENTS.md` and planning/closure records.

Do not move runtime models such as `ControlState`, `ProxySpec`,
`ConnectionSnapshot`, or listener ownership into the protocol crate.

## Required protocol crate boundary

`eggchaos-protocol` must be usable by a Rust control client or schema generator
without linking the server runtime.

Preferred public categories:

- common service/version/error DTOs;
- stream proxy/fault/connection request/response DTOs;
- datagram proxy/fault/association request/response DTOs;
- Scenario V1 DTOs;
- Scenario V2 source/compile/validate/run DTOs;
- route/operation identifiers and stable media-type/version constants;
- conversion helpers into `eggchaos-core` or `eggchaos-experiment` semantic
  types where ownership is naturally below the server.

The crate must not:

- open sockets;
- own Tokio runtimes/tasks;
- depend on EggServe;
- hold live policy/state;
- know CLI presentation;
- contain Toxiproxy DTOs;
- depend on EggReplay/EggProbe.

If a type is only meaningful as an internal server runtime view and is not part
of the documented JSON contract, leave it in the server.

## OpenAPI contract

Create `api/openapi/eggchaos-v1.yaml` using a broadly supported OpenAPI version
appropriate to the selected generator toolchain. Prefer compatibility with
mature Python and TypeScript generators over using a newer spec revision for
its own sake.

Required route families include the exact implemented native inventory:

- `/v1/health` and `/v1/version`;
- stream proxy CRUD;
- stream fault CRUD;
- connection list/get/terminate;
- history;
- reset;
- Scenario V1/V2 validate/compile/apply/get/cancel as implemented;
- datagram proxy CRUD;
- datagram fault CRUD;
- datagram association list/get/terminate;
- `/metrics` with its non-JSON Prometheus response.

The document must correctly express:

- bearer auth requirements and unauthenticated exceptions;
- JSON request/response media types;
- Prometheus text for metrics;
- fault discriminators and variant-specific fields;
- required versus defaulted fields;
- integer duration units;
- nullable/optional fields;
- bounded error body shape;
- path parameter escaping expectations at the semantic level.

Do not expose internal Rust enum representation names when the native wire
spelling differs.

## Single-authority / drift rule

M032 must select and document one mechanical direction:

1. protocol metadata generates the checked-in OpenAPI document; or
2. the checked-in OpenAPI document and protocol DTOs are both generated from a
   smaller common schema authority; or
3. executable contract tests prove every checked-in OpenAPI operation/schema
   against protocol DTO serialization and the real server route inventory.

A purely manual convention such as “remember to update both files” is not
sufficient.

The selected mechanism must fail CI when:

- a native route is added/removed without contract reconciliation;
- a fault discriminator changes;
- a required field/default/unit changes;
- an error envelope changes incompatibly;
- the checked-in generated artifact is stale.

## Compatibility requirements

Before moving each DTO family, capture current JSON fixtures from the exact
baseline and preserve them byte/semantic equivalent after extraction.

At minimum freeze examples for:

- every stream fault variant;
- every datagram fault variant;
- proxy create/patch/view;
- datagram proxy create/patch/view;
- connection and datagram-association views;
- Scenario V1 action variants;
- Scenario V2 source schedule, repeat, compile, validate, run, event, and
  cleanup output;
- health/version/reset;
- representative validation/error responses.

Unknown-property rejection and malformed discriminator behavior must remain
equivalent.

Existing Rust users importing DTOs from `eggchaos_server::*` should continue
to compile through re-exports unless an item is demonstrably private/internal.

## Ordered work packages

### WP1 — Inventory and freeze current contract

Record every native route, method, request/response DTO, status code, auth rule,
content type, error envelope, default, bound, and stable spelling from the
current exact baseline. Commit golden fixtures before moving types.

### WP2 — Create `eggchaos-protocol`

Add the crate with minimal dependencies and move the wire DTO families in
cohesive groups. Keep protocol-only validation/conversion below the server where
appropriate.

### WP3 — Server compatibility adapters

Update `eggchaos-server` to consume the protocol crate. Retain server re-exports
and keep runtime conversion/state mutation in the server. Prove no alternate
state store appears.

### WP4 — Route/operation metadata

Represent the native operation inventory in a form that can be checked against
the real admin router without teaching the protocol crate how to serve HTTP.

### WP5 — OpenAPI generation/check

Produce `api/openapi/eggchaos-v1.yaml` and the deterministic command/script that
regenerates or validates it. Pin any external generator/validator version used
for qualification.

### WP6 — Golden and negative contract tests

Exercise all discriminated unions, defaults, unknown fields, malformed input,
error envelopes, and route coverage through both protocol DTOs and the real
server.

### WP7 — Package/release integration

Add the new crate to release/package smoke, cargo metadata checks, dependency
policy, docs generation, and publish-order proof. Document the resulting order.

### WP8 — Documentation/planning reconciliation

Update control-plane/architecture/tooling docs, README crate inventory where
appropriate, `AGENTS.md`, registry state, and create exact-candidate closure
evidence.

## Invariants and failure semantics

- Native `/v1` behavior does not change merely because DTO ownership moves.
- The protocol crate has no server/listener/state authority.
- One semantic validation authority remains for core faults and Scenario V2.
- Server runtime errors remain bounded and map to the same native envelope.
- OpenAPI describes implemented behavior; implementation is never changed only
  to satisfy a generator limitation.
- Toxiproxy compatibility remains separate from native protocol DTOs.
- Existing deterministic RNG/schedule fingerprints are untouched.
- No new unbounded strings/collections enter DTO parsing.
- No unsafe code is introduced.

## Required tests

At minimum:

- pre/post JSON golden corpus equality;
- round trips for every stream fault variant;
- round trips for every datagram fault variant;
- Scenario V1 and V2 JSON/TOML semantic-equivalence fixtures;
- unknown-field rejection fixtures;
- validation-boundary fixtures;
- native error-envelope fixtures;
- route inventory versus OpenAPI operation inventory equality/check;
- server admin integration tests using protocol DTOs;
- Rust source-compat smoke through `eggchaos-server` re-exports;
- OpenAPI parse/validate/generation-drift test;
- package-list and publish-order proof for `eggchaos-protocol`;
- existing stream/datagram/Scenario V2 golden traces unchanged.

## Verification

Minimum local gate:

```sh
./scripts/check.sh
cargo test -p eggchaos-protocol --all-features
cargo test -p eggchaos-server --all-features
cargo test -p eggchaos-cli --all-features
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
./scripts/release-smoke.sh
```

Also run the pinned Toxiproxy and EggFetch regressions because server/package
topology changes can accidentally affect those adapters:

```sh
TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" \
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 \
  ./scripts/qualify_toxiproxy_v2_12.sh
./scripts/qualify_eggfetch.sh
```

Run the deterministic OpenAPI regeneration/check command introduced by M032 on
the exact candidate.

## Acceptance criteria

M032 closes only when:

- `eggchaos-protocol` exists as a narrow publishable crate;
- all intended native wire DTOs have one clear owner;
- `eggchaos-server` uses that owner without duplicating DTO definitions;
- existing server DTO imports remain compatibility-reexported where practical;
- native JSON behavior, status/error semantics, defaults, units, and bounds are
  unchanged;
- OpenAPI covers the complete implemented native surface;
- OpenAPI drift is mechanically detectable in CI;
- protocol/server route inventory is reconciled;
- package/release smoke includes the new crate and valid publish order;
- existing Toxiproxy, EggFetch, stream, datagram, and Scenario V2 regressions
  remain green;
- no foreign-language SDK or native FFI code is introduced prematurely;
- closure evidence records the exact candidate and contract inventory.

Create
`plans/closure/M032-native-protocol-contract-extraction-and-openapi-foundation-closure.md`.

## Stop/rejection conditions

Do not close if:

- OpenAPI is maintained only by convention with no drift check;
- the protocol crate depends on `eggchaos-server`, EggServe, CLI, or a
  downstream product;
- runtime/state authority migrates into the protocol crate;
- server and protocol keep duplicate native DTO implementations;
- JSON spellings/defaults/bounds change without an explicit versioned contract
  decision;
- existing Rust DTO users are broken without a justified compatibility plan;
- generator limitations cause native API semantics to be weakened;
- package/publish order becomes unresolved;
- any existing deterministic golden corpus changes unexpectedly.

## Follow-on activation

A clean M032 closure makes M033 ready.

M033 must consume the exact M032 OpenAPI/protocol authority. It must not
reconstruct schemas independently in Python or TypeScript.
