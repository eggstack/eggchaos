# ADR 006 — Cross-Language Control Contracts and Native Binding Boundary

Status: accepted  
Date: 2026-09-25

## Context

Eggchaos now has closure-backed stream and datagram fault engines, an explicit
native `/v1` control plane, Scenario V2 compilation/execution, a composable
EggFetch transport adapter, and a consumer-neutral experiment harness. M031
qualified the current public Rust seams and dependency direction on exact
candidate `fa189b9`.

The roadmap has long listed language bindings as a post-v1 direction. The
remaining question is which boundary should become cross-language. The current
Rust engine is intentionally Rust-native:

- stream impairment wraps Tokio `AsyncRead + AsyncWrite`;
- live policy publication uses Rust ownership and atomic snapshot types;
- Scenario V2 embedded execution uses Rust traits/futures and
  `tokio::time::Instant`;
- the standalone server already translates those internals into an explicit,
  versioned JSON contract in `eggchaos-server::native` and `native_v2`.

Binding those internal Rust types directly would freeze implementation details,
force foreign runtimes to understand Tokio ownership, and create difficult ABI
lifetime/error contracts. Conversely, reimplementing control semantics in each
language would create parallel authorities and drift from the qualified server.

Toxiproxy compatibility already supplies a broad cross-language ecosystem for
the legacy TCP subset. Native eggchaos clients are most valuable for the
eggchaos-specific surface: datagram resources, Scenario V2, native connection
and association evidence, history, and deterministic native fault semantics.

The repository also enforces `unsafe_code = "forbid"` across normal Rust
crates. Any FFI work must preserve that safety posture rather than weakening
the core/server crates to make a foreign ABI convenient.

## Decision 1: the native `/v1` JSON contract is the primary cross-language boundary

The first-class language-neutral API is the existing versioned native control
contract, not the layout of Rust structs and not a generic C ABI.

Python, TypeScript, Go, Java, .NET, and other remote clients should consume the
same `/v1` authority that the CLI consumes. They must not implement a second
listener/runtime/state path and must not translate through Toxiproxy when a
native route exists.

Toxiproxy clients remain supported for the declared v2.12 compatibility subset.
Native SDKs exist to expose eggchaos-native capabilities and stronger typed
contracts, not to duplicate compatibility libraries.

## Decision 2: extract a protocol crate before generating SDKs

M032 will introduce a narrow publishable `eggchaos-protocol` crate as the Rust
authority for stable native wire DTOs and contract metadata.

The dependency shape should remain equivalent to:

```text
eggchaos-core
      ^
      |
eggchaos-experiment
      ^
      |
eggchaos-protocol
      ^
      |
eggchaos-server ----> runtime/control authority
      ^
      |
eggchaos-cli

external SDKs ----> OpenAPI/native JSON contract
```

Exact dependency edges may be narrower where practical. The invariant is that
`eggchaos-protocol` must not depend on `eggchaos-server`, EggServe, CLI,
Toxiproxy, EggFetch, EggReplay, or EggProbe.

The protocol crate may depend on `eggchaos-core` and/or
`eggchaos-experiment` when conversion into already-public semantic types is
the cleanest single authority. Server-only runtime conversions remain in the
server.

Existing `eggchaos-server` public DTO exports should remain compatibility
re-exports when practical so M032 is an extraction rather than gratuitous Rust
API churn.

## Decision 3: OpenAPI is a generated/verified artifact, not a second semantic authority

M032 will add a checked-in OpenAPI document for the native API, initially at
`api/openapi/eggchaos-v1.yaml`.

The OpenAPI document must be derived from, or mechanically checked against, the
same protocol DTO/route authority used by the running server. A hand-maintained
spec that can silently diverge from Serde behavior is rejected.

The contract must describe at least:

- all native stream proxy/fault/connection routes;
- all native datagram proxy/fault/association routes;
- Scenario V1 and Scenario V2 validate/compile/apply/run/cancel routes;
- health, version, reset, history, and metrics behavior;
- bearer authentication where applicable;
- stable status/error envelope behavior;
- discriminated stream and datagram fault unions;
- integer units/bounds that are already part of the native contract.

Schema generation must not change runtime semantics merely to satisfy a
generator. If a client generator cannot model a valid protocol construct, fix
or layer the generator rather than weakening the server contract.

## Decision 4: Python and TypeScript are the first native control SDKs

M033 will build remote control SDKs for Python and TypeScript/Node against the
M032 contract.

Each SDK should have:

1. a reproducible generated or mechanically derived low-level contract layer;
2. a small handwritten idiomatic facade for authentication, errors, and common
   operations;
3. no embedded Rust/native code;
4. no binary/process lifecycle management in the initial SDK tranche.

The initial SDKs must expose the complete native surface, not just the
Toxiproxy-compatible subset.

Other languages should preferentially consume the same OpenAPI contract later.
Go, Java/Kotlin, and .NET do not receive native FFI bindings merely because a
C ABI could theoretically serve them.

## Decision 5: native embedding starts with a Python pilot over a safe Rust facade

M034 may add native Python embedding only after M032/M033 prove the protocol and
user-facing client model.

The Python extension must not expose `ChaosStream<T>`, Tokio stream traits,
Rust futures, `Arc`, borrowed Rust references, or `tokio::time::Instant` as
foreign ABI concepts.

Instead, M034 should introduce a coarse, safe Rust embedding facade (preferred
crate name: `eggchaos-embed`) that owns service/runtime lifecycle and delegates
all state changes to existing eggchaos authorities. The Python binding then
wraps that facade with managed objects and snapshot/value types.

The embedded facade is not another server implementation. It owns lifecycle
and calls the existing server/control/runtime machinery directly. Binding code
must preserve the same validation, generation, determinism, bounds, and error
semantics as remote control.

## Decision 6: no generic C ABI is activated by this tranche

M032–M034 do not create a generic C ABI, JNI layer, P/Invoke layer, Node native
addon, or Go/cgo binding.

A future generic C ABI requires:

- a separate ADR specifically freezing ABI ownership/lifetime/error/version
  rules;
- at least two concrete native consumers, or otherwise strong evidence that a
  shared ABI is preferable to language-specific safe bindings;
- opaque handles rather than exported Rust layouts;
- explicit allocator ownership and destruction functions;
- panic containment and stable integer/error representations;
- independent ABI compatibility qualification.

The future C ABI may reuse the M034 safe embedding facade, but M034 must not
pre-freeze C-compatible layouts in anticipation of hypothetical consumers.

## Decision 7: FFI safety exceptions remain isolated

No handwritten `unsafe` is permitted in `eggchaos-core`,
`eggchaos-experiment`, `eggchaos-protocol`, `eggchaos-server`, or
`eggchaos-embed`.

A binding framework such as PyO3 may necessarily generate/use unsafe FFI glue
inside its own dependency or macro boundary. If the binding crate cannot
compile under the workspace `unsafe_code = "forbid"` lint solely because of
generated framework code, M034 may use the narrowest crate-local lint
exception required for that generated boundary and must record the reason and
audit evidence in closure. Handwritten unsafe code still stops the milestone
and requires a new explicit ADR.

This decision does not authorize a handwritten C ABI.

## Decision 8: WASM is limited to pure tooling if activated later

The Scenario V2 compiler/fingerprint path is a plausible future WASM target
because it is deterministic and largely pure. The network engines and listener
runtime are not browser-WASM targets.

No WASM extraction is part of M032–M034. It should be activated only by a
concrete editor/browser/tooling consumer and must reuse the same compiler
semantics rather than reimplement them in TypeScript.

## Consequences

Positive:

- cross-language clients reuse the already-qualified native authority;
- the server's wire contract becomes independently consumable by Rust and
  external generators;
- Python and TypeScript gain native datagram/Scenario V2/evidence coverage
  without FFI complexity;
- native Python embedding is isolated behind a coarse managed facade;
- core Tokio/stream types remain free to evolve without becoming ABI;
- future languages can use the OpenAPI contract without changing eggchaos;
- any later C ABI starts from evidence rather than speculation.

Costs:

- DTO extraction requires compatibility re-exports and careful package-order
  changes;
- OpenAPI drift checks become a permanent CI/release responsibility;
- generated SDK artifacts add non-Rust package tooling to the repository;
- M034 introduces Python wheel/platform qualification and interpreter/runtime
  lifecycle work;
- remote SDKs and the native Python package are distinct distribution surfaces.

## Rejected alternatives

### Build a universal C ABI first

Rejected. It would freeze the hardest lifetime/error/version boundary before
there is evidence for multiple native consumers and would pressure the project
to weaken its current safe-Rust boundary.

### Bind `eggchaos-core` streams directly into every language

Rejected. Tokio stream traits, polling, pinning, live policy ownership, and
termination semantics are Rust implementation concepts, not a stable foreign
ABI.

### Generate clients directly from server implementation types

Rejected. M017 intentionally separated the native wire contract from internal
runtime layout. Cross-language clients must reinforce that separation, not
reverse it.

### Hand-write each SDK without one machine-readable contract

Rejected. That creates drift in fault discriminators, defaults, bounds, error
envelopes, and newly added routes.

### Ship Node, JNI, .NET, and Go native bindings together

Rejected. Those ecosystems already consume HTTP naturally. Native bindings are
activated only where in-process embedding produces a demonstrated benefit.

### Make the SDK spawn/manage the eggchaos binary by default

Rejected for the initial tranche. Binary discovery, version matching, service
management, and process cleanup are separate product concerns. The first SDK
contract is remote control of an explicitly selected endpoint.
