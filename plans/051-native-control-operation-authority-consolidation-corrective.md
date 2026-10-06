# M051 — Native Control Operation Authority Consolidation Corrective

Status: closed (header reconciled to the registry status by M061; as written at registration: `blocked`; evidence in `plans/closure/M051-native-control-operation-authority-consolidation-corrective-closure.md`)
Depends on: M050 closed
Role: collapse duplicated HTTP/embed application-operation plumbing while preserving transport presentations
Activation baseline: exact M050 closure candidate

## Objective

Create one typed native application-operation authority between versioned protocol DTOs and ControlState so the native HTTP admin surface and eggchaos-embed do not independently reimplement validation/conversion/state-call/error-classification sequences.

The existing architecture already has good lower-level authorities:

- eggchaos-protocol owns /v1 wire DTOs and NATIVE_OPERATIONS;
- ControlState and DatagramRuntime own mutable runtime state;
- eggchaos-experiment owns Scenario V2 semantics;
- admin.rs owns HTTP/auth/path/status presentation;
- eggchaos-embed owns blocking lifecycle presentation.

M051 should make the middle operation layer equally explicit without moving transport concerns into it.

## Compatibility boundary

This is an internal/additive consolidation.

Frozen:

- all 36 existing NATIVE_OPERATIONS;
- every HTTP method/path/status and JSON response shape;
- metrics text behavior;
- authentication and loopback/public-admin policy;
- EmbeddedService public method names, signatures, blocking behavior, and error categories;
- protocol DTO shapes/defaults/unknown-field behavior;
- CLI, Python client, TypeScript client, Python-native behavior;
- Toxiproxy routes and error compatibility;
- ControlState mutation semantics;
- Scenario V1/V2 deterministic behavior.

No existing public item may be removed or renamed. If a cross-crate support type must be public so eggchaos-embed can consume it, keep it deliberately narrow and additive; prefer extending the existing native adapter seam over creating a broad second API.

## Scope

### In scope

- a typed native operation facade/helper layer in eggchaos-server;
- central conversion/validation for proxy CRUD;
- central conversion/validation for stream fault CRUD;
- central conversion/validation for datagram proxy/fault CRUD;
- central handling of connection/history/reset operations where there is actual duplicated application logic;
- central Scenario V1/V2 get/cancel family dispatch after M050's unified registry;
- central Scenario V2 validate/compile/apply conversion where HTTP and embed currently duplicate it;
- shared mapping from ControlError/runtime failures to stable operation-level categories;
- refactoring admin.rs and eggchaos-embed to delegate to this authority;
- exact response/error parity tests.

### Non-goals

- no generic RPC framework;
- no replacement of NATIVE_OPERATIONS/OpenAPI;
- no route generation framework;
- no Toxiproxy migration onto native DTOs;
- no async HTTP client changes;
- no SDK generation change unless parity testing exposes a pre-existing mismatch;
- no scenario-list route;
- no new public feature.

## Design constraints

The authority should operate on typed values, not HTTP requests.

A preferred shape is a narrow wrapper around ControlState with methods such as create_proxy, patch_proxy, add_fault, apply_schedule_v2, and cancel_scenario that accept protocol DTOs and return typed protocol/runtime result values.

HTTP-specific responsibilities remain outside:

- bearer authentication;
- path decoding;
- method routing;
- request-body size enforcement;
- Content-Type;
- HTTP status selection;
- JSON serialization;
- metrics text headers.

Embed-specific responsibilities remain outside:

- private Tokio runtime ownership;
- blocking the caller;
- start/shutdown lifecycle;
- mapping operation errors into the existing EmbedError variants where necessary for source compatibility.

## Affected surfaces

Expected:

- crates/eggchaos-server/src/native.rs and/or a new native_control.rs module;
- crates/eggchaos-server/src/admin.rs;
- crates/eggchaos-server/src/lib.rs for additive re-export only if cross-crate access requires it;
- crates/eggchaos-embed/src/lib.rs;
- server/admin tests;
- embed tests;
- bindings/python-native conformance tests;
- architecture/control-plane-cli.md;
- architecture/embedding-native.md;
- architecture/protocol-contract.md if authority wording changes.

## Ordered work packages

### WP1 — Build a parity matrix

For each of the 36 native operations, classify:

- HTTP-only presentation;
- shared typed operation;
- embed-exposed operation;
- transport-only concern.

Record current HTTP status/JSON behavior and current embed return/error behavior for the shared set.

The implementation must not use this milestone to "clean up" established external differences.

### WP2 — Introduce the typed operation seam

Implement the smallest shared operation abstraction that removes duplicated semantic work.

Requirements:

- accepts already parsed protocol DTOs or typed identifiers;
- delegates all mutable state to ControlState/DatagramRuntime;
- reuses existing native conversion helpers rather than cloning them;
- returns typed data and a stable operation-level error;
- owns no listener/runtime state;
- contains no HTTP or PyO3 types.

### WP3 — Migrate HTTP admin

Replace duplicated conversion/state logic in admin.rs with calls to the shared operation seam.

Keep route matching, status codes, envelopes, headers, auth, and request parsing unchanged.

Add golden/parity tests around representative success and failure cases before and after migration.

### WP4 — Migrate eggchaos-embed

Replace duplicated conversion/state logic in EmbeddedService with the same operation seam.

Preserve:

- every public EmbeddedService signature;
- EmbedError categories;
- blocking/lifecycle contract;
- control_state advanced escape hatch.

### WP5 — Remove redundant helpers only when source compatibility allows

Existing exported native conversion functions and compatibility re-exports must remain available if they are public today. They may become thin wrappers around the new authority.

Do not delete a helper merely because internal call sites disappear.

## Failure semantics

One invalid request should be classified once at the typed operation boundary and then presented by HTTP/embed according to their existing external contracts.

No transport layer may silently broaden or narrow accepted values relative to another transport after M051.

## Required verification

At minimum:

    cargo test -p eggchaos-server --all-features
    cargo test -p eggchaos-embed --all-features
    cargo test -p eggchaos-protocol --all-features
    ./scripts/check_openapi.sh
    ./scripts/check_python_client.sh
    ./scripts/check_typescript_client.sh
    ./scripts/qualify_language_clients.sh
    ./scripts/qualify_python_native.sh
    cargo test --workspace --all-features
    ./scripts/check.sh

Run representative live HTTP-vs-embed conformance cases for proxy/fault/datagram/scenario operations, including invalid patches, conflicts, not-found cases, and Scenario V1/V2 get/cancel dispatch.

## Acceptance criteria

M051 may close only when:

1. shared HTTP/embed operations use one typed semantic path;
2. admin.rs is reduced to HTTP/control presentation for those operations;
3. eggchaos-embed is reduced to lifecycle/blocking presentation for those operations;
4. all 36 existing native routes remain unchanged;
5. EmbeddedService signatures and error categories remain unchanged;
6. no existing public server helper is removed or renamed;
7. OpenAPI, SDK, and Python-native conformance remain green;
8. no Toxiproxy behavior changes;
9. exact-candidate closure evidence is recorded.

## Rejection / stop conditions

Stop and re-plan if the proposed seam:

- becomes a second mutable state store;
- introduces HTTP concepts into protocol/core;
- requires route/schema changes;
- requires removal of a public helper;
- pushes Toxiproxy through native semantics in a way that changes parity.

## Closure evidence

Create plans/closure/M051-native-control-operation-authority-consolidation-corrective-closure.md with the operation parity matrix, exact candidate, cross-surface conformance results, and public-surface diff statement.

## Successor activation

Closing M051 activates M052.
