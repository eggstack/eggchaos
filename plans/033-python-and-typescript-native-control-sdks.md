# M033 — Python and TypeScript Native Control SDKs

Status: blocked  
Depends on: M032 (must close), ADR 006  
Role: first cross-language native-control clients

## Objective

Build and qualify Python and TypeScript/Node SDKs over the M032 native
`/v1` OpenAPI/protocol contract so non-Rust test harnesses can drive the full
eggchaos-native surface without depending on Toxiproxy compatibility or
embedding Rust.

The SDKs must cover stream and datagram resources, Scenario V1/V2,
connection/association evidence, history, reset, metrics, authentication, and
bounded native errors. They are control clients only: no native extension, no
FFI, no alternate server, and no automatic binary/process management.

## Baseline and dependencies

M032 is expected to close with:

- a publishable `eggchaos-protocol` crate owning native wire DTOs;
- a checked-in, mechanically verified
  `api/openapi/eggchaos-v1.yaml`;
- route/schema drift detection;
- compatibility-preserving server re-exports;
- exact-candidate native contract fixtures.

ADR 006 selects Python and TypeScript as the first control SDK targets because
they are common integration-test environments and can consume HTTP directly.
It also requires all SDKs to derive from the same contract rather than
reimplementing eggchaos semantics.

M033 must not begin implementation until M032's closure record identifies the
exact OpenAPI generation/check mechanism and contract candidate.

## Scope

### In scope

- Python control SDK under a dedicated repository path such as
  `bindings/python-client/`.
- TypeScript/Node control SDK under a dedicated path such as
  `bindings/typescript-client/`.
- Reproducible generation or mechanical derivation from
  `api/openapi/eggchaos-v1.yaml`.
- A thin handwritten idiomatic facade where generated APIs are awkward.
- Typed models for stream faults, datagram faults, proxy resources, scenario
  inputs/results, connection/association views, and native errors.
- Bearer-token authentication.
- Base URL and bounded request timeout configuration.
- Python synchronous client support.
- Python asynchronous client support when it can be provided without
  duplicating models/semantics.
- Promise-based TypeScript API using the selected supported Node/browser-fetch
  transport boundary.
- Complete native route coverage.
- Explicit metrics text handling rather than pretending it is JSON.
- Typed failure surfaces preserving HTTP status plus bounded native error
  category/detail.
- Contract-version compatibility checks where useful.
- Integration tests against a real loopback eggchaos native server.
- Cross-language contract/golden fixtures shared with M032.
- Reproducible package builds.
- CI matrix for supported Python and Node versions on the repository's normal
  host OSes where practical.
- Package/readme examples for pytest/unittest and Node test harness use.
- Release qualification that builds packages without requiring publication.

### Non-goals

- No PyO3/maturin/native Python module; that is M034.
- No Node-API/napi-rs addon.
- No generic C ABI, JNI, P/Invoke, cgo, UniFFI, or WASM.
- No Go/Java/.NET SDK in this milestone.
- No automatic daemon installation, download, discovery, spawn, or service
  management.
- No Docker/Testcontainers wrapper.
- No Toxiproxy client compatibility wrapper.
- No new server route merely because a generator prefers a different shape.
- No SDK-owned retry policy for mutating operations unless the native contract
  explicitly makes the operation idempotent and the retry behavior is
  separately justified.
- No client-side recreation of Scenario V2 compilation/fingerprinting when the
  server endpoint is authoritative.

## Package and API shape

Exact registry package names are owner release decisions and must be checked for
availability before publication. Repository/import boundaries should be stable
independent of registry naming.

Preferred Python import namespace:

```python
from eggchaos_client import Client, AsyncClient
```

Preferred TypeScript source API:

```ts
import { EggchaosClient } from "@eggstack/eggchaos-client";
```

If registry naming differs, do not change the underlying generated namespace
without a compatibility reason.

Both SDKs should separate:

1. generated/mechanically derived models and endpoint bindings;
2. a small stable facade for configuration, auth, errors, and common grouped
   resources.

Do not hand-copy every model into a second facade layer.

## Required native coverage

The SDK surface must cover every M032 OpenAPI operation, including:

- health and version;
- reset;
- stream proxy list/create/get/patch/delete;
- stream fault list/create/get/patch/delete;
- connection list/get/terminate;
- history;
- datagram proxy list/create/get/patch/delete;
- datagram fault list/create/get/patch/delete;
- datagram association list/get/terminate;
- Scenario V1/V2 validate/compile/apply/get/cancel as exposed by the server;
- metrics as Prometheus text.

A coverage test must compare generated client operations with M032's operation
inventory so a newly added native route cannot silently disappear from one SDK.

## Generation strategy

Pin the generator/toolchain versions used for both clients.

Prefer generated code where it faithfully preserves:

- discriminated fault unions;
- required/defaulted/nullable distinctions;
- integer width and duration units;
- error response models;
- authentication configuration.

If the selected generator emits pathological dependency weight, loses
discriminator safety, or cannot represent an existing valid contract, the
implementation may keep generated models/schema metadata while using a small
handwritten HTTP transport layer. That decision must be recorded in the M033
closure with evidence that the OpenAPI document remains the semantic source.

Never fix generator shortcomings by changing valid server behavior.

Generated output must be reproducible. CI should fail when regeneration changes
committed generated artifacts unexpectedly.

## Python client requirements

At minimum provide:

- `Client(base_url=..., token=..., timeout=...)`;
- `AsyncClient` if selected implementation can share the same models cleanly;
- context-manager cleanup for owned HTTP transport resources;
- typed resource methods or grouped subclients;
- explicit native error class carrying status/code/detail without leaking
  bearer tokens;
- percent-safe fault/resource identifiers through proper URL encoding;
- datagram and Scenario V2 model support;
- raw Prometheus text retrieval for metrics.

Do not make synchronous methods secretly depend on a global event loop.

Do not retain response bodies beyond what the caller/result object owns.

## TypeScript client requirements

At minimum provide:

- `new EggchaosClient({ baseUrl, token, timeoutMs, fetch? })` or equivalent;
- Promise-returning operations;
- typed discriminated unions for stream/datagram fault kinds;
- injectable `fetch` or equivalent transport for testability where practical;
- AbortSignal/cancellation propagation;
- bounded native error object with status/code/detail;
- no Node-only primitive in the model layer unless required;
- raw Prometheus text retrieval.

If the client targets Node only initially, document that clearly rather than
claiming browser compatibility that has not been qualified.

## Authentication and secret handling

Bearer tokens must:

- be sent only in the Authorization header;
- never appear in repr/debug/error output;
- never be serialized into request/response models;
- never be included in generated exception messages by default;
- be redacted from logging examples.

Base URLs containing userinfo should be rejected or normalized according to a
documented rule rather than accidentally echoing credentials.

## Error and retry semantics

SDKs must preserve the native server's distinction between:

- validation/client errors;
- missing resources;
- generation/conflict failures;
- authentication/authorization failures;
- server/internal failures;
- transport/connect/timeout failures.

Transport failures and HTTP-native errors must be distinct exception/error
families.

No implicit retry of mutation requests by default. Read-only retry support, if
added, must be opt-in and separately tested.

## Ordered work packages

### WP1 — Freeze generator/toolchain choices

Against the closed M032 contract, evaluate generator targets for discriminator
quality, dependency weight, async/sync support, and deterministic output. Pin
versions and document the regeneration commands.

### WP2 — Generate low-level clients/models

Generate or mechanically derive Python and TypeScript low-level surfaces from
the exact OpenAPI document. Commit only deterministic output that is practical
to review and reproduce.

### WP3 — Add thin idiomatic facades

Add client configuration, auth, grouped operations, native error mapping,
metrics text handling, and language-idiomatic resource entry points without
duplicating model semantics.

### WP4 — Contract coverage fixtures

Create shared fixtures for every fault variant, proxy resource, scenario form,
and representative error. Decode actual server responses through both SDKs.

### WP5 — Real-server integration matrix

Launch the normal eggchaos server on loopback for tests and execute equivalent
Python and TypeScript flows covering stream, datagram, Scenario V2, connection
or association evidence, auth failures, and reset.

### WP6 — Cancellation/timeouts and negative paths

Exercise Python async cancellation if provided, TypeScript AbortSignal,
connect/read timeouts, malformed identifiers, validation failures, conflicts,
missing resources, unauthorized requests, and server shutdown.

### WP7 — Package build/reproducibility

Build Python sdist/wheel artifacts for the pure client package and TypeScript
package/tarball artifacts. Verify generated sources are clean after
regeneration.

### WP8 — CI and release-smoke integration

Add focused language jobs or scripts without making the Rust core gate depend
on network package registries. Cache/install pinned tooling and preserve
bounded CI runtimes.

### WP9 — Documentation and closure

Add Python and TypeScript quick-start examples, document remote-versus-native
binding distinction, reconcile registry/roadmap, and record exact-candidate
closure evidence.

## Invariants and failure semantics

- OpenAPI/protocol contract from M032 remains the one semantic authority.
- SDKs never mutate server state except through documented native operations.
- No alternate fault validation/RNG/scenario compiler is created.
- No bearer token is emitted in logs/errors.
- Mutation methods do not retry implicitly.
- Metrics remain text.
- SDK-generated defaults must match server defaults; when absence has semantic
  meaning, the SDK must not eagerly substitute a client-side default that
  changes wire behavior.
- Unknown future enum/discriminator behavior is explicit and tested according
  to the selected generator's compatibility policy.
- No native extension or unsafe Rust is introduced.

## Required tests

At minimum for Python:

- import/package smoke;
- sync client health/version;
- async client health/version if shipped;
- every stream fault union serialize/deserialize;
- every datagram fault union serialize/deserialize;
- stream proxy CRUD + fault CRUD;
- datagram proxy CRUD + fault CRUD;
- connection/association list/get/terminate;
- Scenario V2 validate/compile/apply/get/cancel;
- reset/history;
- metrics text;
- auth success/failure and token redaction;
- timeout/transport failure versus native HTTP error;
- malformed identifier/path encoding;
- generated-artifact drift.

At minimum for TypeScript:

- package/typecheck/build smoke;
- all equivalent model/route cases above;
- AbortSignal cancellation;
- auth/token redaction;
- generated-artifact drift.

Cross-language fixture tests must demonstrate that equivalent inputs serialize
to semantically equivalent native JSON.

## Verification

Minimum repository gate after language dependencies are installed by the
focused scripts:

```sh
./scripts/check.sh
./scripts/release-smoke.sh
./scripts/qualify_eggfetch.sh
```

Add and run deterministic M033 scripts equivalent to:

```sh
./scripts/check_python_client.sh
./scripts/check_typescript_client.sh
./scripts/qualify_language_clients.sh
```

The exact script names may differ, but one command per package plus one
real-server cross-language qualification command must exist and be documented.

Run the pinned Toxiproxy oracle gate on the exact candidate because native
client work must not disturb compatibility routing indirectly:

```sh
TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" \
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 \
  ./scripts/qualify_toxiproxy_v2_12.sh
```

## Acceptance criteria

M033 closes only when:

- M032 is closed and its exact OpenAPI contract is the generator source;
- Python and TypeScript clients cover every native operation;
- stream/datagram fault unions remain strongly distinguishable in both
  languages;
- bearer auth, errors, metrics, timeouts, and cancellation are correctly
  represented;
- generated output is reproducible and drift-checked;
- equivalent real-server flows pass in both languages;
- package artifacts build successfully without publication;
- no FFI/native code or automatic daemon management is added;
- Rust workspace, EggFetch, Toxiproxy, deterministic stream/datagram, and
  Scenario V2 regressions remain green;
- documentation clearly distinguishes native control SDKs from Toxiproxy
  compatibility and future in-process bindings;
- closure evidence records exact generator versions, language/runtime matrix,
  candidate SHA, and known generator limitations.

Create
`plans/closure/M033-python-and-typescript-native-control-sdks-closure.md`.

## Stop/rejection conditions

Do not close if:

- either SDK has missing native operations;
- one language uses a handwritten schema independent of OpenAPI;
- generated artifacts cannot be deterministically reproduced;
- generator shortcomings lead to server contract changes;
- authentication secrets appear in logs/errors;
- mutation methods retry implicitly;
- Scenario V2 is reimplemented client-side instead of calling the native
  authority where server execution is intended;
- package build requires unpublished/local registry state that cannot be
  reproduced in CI;
- a native extension or C ABI is introduced as an implementation shortcut.

## Follow-on activation

A clean M033 closure makes M034 ready.

Other remote-language SDKs may be proposed after M033, but they should normally
reuse the same OpenAPI contract and do not block the Python native embedding
pilot.
