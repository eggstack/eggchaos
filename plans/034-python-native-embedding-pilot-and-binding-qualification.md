# M034 — Python Native Embedding Pilot and Binding Qualification

Status: blocked  
Depends on: M033 (must close), ADR 006  
Role: first in-process foreign-language embedding surface

## Objective

Add and qualify a Python native embedding surface that can run eggchaos
in-process without exposing Rust/Tokio stream internals or creating a second
fault/runtime implementation.

M034 should first introduce a coarse safe Rust embedding facade, preferably a
new `eggchaos-embed` crate, that owns lifecycle and delegates to existing
eggchaos server/control authorities. A PyO3/maturin binding then wraps that
facade with managed Python objects and snapshot/value models.

The goal is to prove whether native embedding materially improves Python test
harness ergonomics while preserving the same deterministic semantics,
validation, bounds, and lifecycle guarantees as the qualified Rust and remote
control surfaces.

M034 does not activate a generic C ABI.

## Baseline and dependencies

M032 must have closed with:

- `eggchaos-protocol` as the stable native wire-contract owner;
- mechanically checked OpenAPI;
- protocol/server compatibility evidence.

M033 must have closed with a real Python control SDK and TypeScript SDK over
that contract. The Python remote client gives M034 a concrete user-facing
semantic reference and error/model vocabulary instead of inventing a separate
native API from scratch.

M030/M031 already provide:

- `eggchaos-experiment` as a consumer-neutral Scenario V2 authority;
- safe Rust embedded experiment primitives;
- shared monotonic epoch support;
- bounded evidence and policy-target abstractions.

ADR 006 requires native bindings to use a coarse facade and forbids direct
foreign exposure of Tokio stream/pinning/lifetime concepts.

Current PyO3 releases support Rust versions below eggchaos's pinned 1.89
baseline, so the pilot should not require an MSRV increase merely to adopt the
binding framework. The exact PyO3/maturin versions used by implementation must
be pinned and recorded in closure.

## Scope

### In scope

- Add a safe Rust embedding facade, preferred name
  `crates/eggchaos-embed`.
- Explicit ownership of an embedded service lifecycle.
- Reuse `eggchaos-server`/`ControlState`/runtime authorities rather than
  implementing networking again.
- Run embedded lifecycle on an owned Tokio runtime/thread model that is hidden
  from foreign callers.
- Start/stop/shutdown with idempotent, testable ownership semantics.
- In-process stream proxy/fault CRUD.
- In-process datagram proxy/fault CRUD.
- Connection/association snapshot/control where practical through existing
  authority.
- Scenario V2 validate/compile/run support by reusing
  `eggchaos-experiment` and/or existing server control adapters.
- Native error mapping aligned with the protocol/remote Python client.
- Python extension under a dedicated path such as
  `bindings/python-native/`.
- PyO3-based Python classes/functions.
- Maturin-based wheel/sdist build path where applicable.
- Managed Python context-manager lifecycle.
- Async-friendly wrappers where they can be implemented without exposing
  Rust futures or coupling eggchaos to a specific Python event-loop internals.
- Prefer a stable CPython ABI wheel strategy such as `abi3` when it supports
  the required surface; document and qualify any reason not to use it.
- Cross-platform wheel build and import smoke on the supported release host
  matrix.
- Interpreter shutdown, repeated start/stop, and exception-path lifecycle
  qualification.
- Benchmark embedded startup/control overhead against the remote Python client
  for representative test-harness operations.
- Document an explicit go/no-go conclusion for any later generic C ABI.

### Non-goals

- No raw `ChaosStream<T>` or Tokio `AsyncRead/AsyncWrite` exposure.
- No foreign ownership of `Arc`, borrowed Rust references, pinned futures, or
  `tokio::time::Instant`.
- No Python callback invoked from networking hot paths in the initial pilot.
- No arbitrary Python fault callback/plugin.
- No per-packet Python execution.
- No new fault or scenario semantics.
- No second listener/runtime/state authority.
- No generic C ABI.
- No Node native addon, JNI, P/Invoke, cgo, UniFFI, or WASM.
- No requirement that the native package and remote Python package merge into
  one distribution.
- No implicit cross-process timing claim.
- No automatic installation or updating of eggchaos binaries.

## Safe embedding facade

The binding must not call deep server internals ad hoc from PyO3 methods.
Introduce one Rust facade that is independently testable without Python.

Preferred conceptual shape:

```text
Python
  |
eggchaos-native (PyO3)
  |
eggchaos-embed
  |
  +--> eggchaos-protocol value/request types
  +--> eggchaos-server lifecycle + ControlState
  +--> eggchaos-experiment Scenario V2 authority
  |
eggchaos-core / runtime
```

The exact public Rust names may vary, but behavior should be equivalent to:

```text
EmbeddedService::start(config/options) -> EmbeddedService
service.health/version
service.create/list/get/patch/delete stream proxy
service.create/list/get/patch/delete stream fault
service.create/list/get/patch/delete datagram proxy/fault
service.connections/associations snapshots + terminate
service.scenario_validate/compile/apply/status/cancel
service.reset
service.shutdown
```

The facade may expose protocol DTOs directly or map them to narrowly named
embedding request/view types. It must not invent alternative defaults or
validation.

## Runtime ownership model

M034 must explicitly choose and document the runtime/thread model before adding
Python classes.

Preferred default: each embedded service owns a dedicated background thread
with one Tokio runtime and a bounded command/control path. This prevents
nested-runtime assumptions when Python is invoked from synchronous code or from
an existing asyncio loop.

Alternative ownership is acceptable only if it proves:

- no nested `block_on` panic;
- no reliance on Python's current thread being a Tokio worker;
- prompt cancellation/shutdown;
- no detached Tokio tasks after service close;
- deterministic drop behavior;
- safe repeated construction/destruction.

Foreign callers should not need to know that Tokio exists.

## Python API shape

Prefer a small managed API rather than one Python class per internal Rust type.

Illustrative shape:

```python
from eggchaos_native import Service, Fault

with Service(seed=7) as chaos:
    proxy = chaos.create_proxy(
        name="redis",
        listen="127.0.0.1:0",
        upstream="127.0.0.1:6379",
    )
    chaos.set_fault(
        "redis",
        Fault.latency("lag", direction="downstream", delay_ns=200_000_000),
    )
```

Async ergonomics may be layered as Python wrappers around safe blocking/native
operations when that avoids binding Rust futures directly. If direct PyO3 async
integration is selected, it must have a clear runtime ownership proof and
cancellation behavior.

The native Python API should reuse the M033 model vocabulary where sensible,
but the two distributions need not share generated transport classes.

## Error contract

Python native errors must preserve the same meaningful categories as the remote
client:

- validation;
- missing resource;
- generation conflict;
- unsupported capability;
- lifecycle/state error;
- bind/connect/runtime failure;
- cancellation;
- internal failure.

Exceptions must carry bounded details and must not expose secrets.

Rust panics must not become normal control flow. Any panic crossing an FFI
boundary is a milestone failure until containment/behavior is understood and
tested.

## FFI and unsafe policy

No manually authored unsafe code is permitted in Eggchaos implementation
crates.

All normal Rust crates, including `eggchaos-embed`, retain the repository's
`unsafe_code = "forbid"` policy.

The PyO3 binding crate should also retain that policy if the framework/macros
permit it. If generated macro glue makes that impossible, use the narrowest
crate-local exception that still forbids handwritten unsafe where practical,
document the exact reason, inspect the expanded/generated boundary, and record
the audit in closure.

If implementation requires handwritten unsafe for object lifetime, callback,
allocator, or thread transfer, stop M034. That requires a new ADR rather than an
inline exception.

## ABI and packaging policy

Prefer CPython stable ABI wheels where compatible with the required API to
reduce the wheel matrix.

At minimum qualify the repository's release host architectures that can build
the extension:

- Linux x86_64;
- Linux aarch64 where the existing cross/release environment can support
  Python wheel production correctly;
- macOS x86_64;
- macOS aarch64;
- Windows x86_64.

Do not claim a wheel target until import/runtime smoke actually runs on that
platform/architecture or an accepted cross-build verification strategy exists.

The package publication step remains owner-controlled. M034 must build and
smoke artifacts but does not need to publish to PyPI.

## Relationship to remote Python SDK

The remote M033 SDK remains supported and is not replaced by the native module.

Document the choice:

- remote client: control an explicit eggchaos daemon/process, simplest
  portability and isolation;
- native module: embed eggchaos lifecycle in the Python test process, useful for
  fixture-local startup/control with no admin HTTP hop.

Equivalent supported operations should share semantics and fixtures. A
cross-surface conformance test should run the same representative scenario
through remote and native Python surfaces and compare resulting native views
and deterministic identifiers where timing-independent equality is defined.

## Evidence and observability

Return snapshots/value objects to Python. Do not expose live Rust references
whose validity depends on service ownership.

If live evidence handles are exposed later, they need an explicit lifetime
model. Initial M034 should prefer immutable snapshots plus stable IDs.

No payload capture is added.

## Performance qualification

Measure rather than assume a native benefit.

At minimum compare:

1. process-local native service startup/shutdown;
2. remote Python client against a loopback daemon for repeated fault mutations;
3. native Python equivalent repeated mutations;
4. Scenario V2 validate/compile small schedule;
5. Scenario V2 1024-event compile/prepare path if exposed;
6. representative no-fault TCP/UDP data path to prove the binding facade did
   not alter underlying runtime performance.

Do not freeze a numerical performance budget before stable measurement. Any
material data-plane regression is a stop condition because Python control
should not enter the packet/byte hot path.

## Ordered work packages

### WP1 — Freeze embedding API and runtime ownership

Before binding Python, design the `eggchaos-embed` lifecycle, thread/runtime
ownership, request/value boundary, shutdown semantics, and error categories.
Add Rust-only tests first.

### WP2 — Implement `eggchaos-embed`

Wire the facade to existing server/control/experiment authorities. Keep all
state ownership in existing runtime structures. Prove repeated start/stop and
drop behavior.

### WP3 — Rust conformance against native protocol

Run representative operations through the facade and server HTTP path and
compare protocol-equivalent outputs/errors.

### WP4 — Add PyO3 binding crate/package

Expose managed Service/value/error types. Keep foreign methods coarse and
bounded. Add context-manager behavior and explicit close/shutdown.

### WP5 — Async ergonomics

Add only the minimal async-facing Python layer justified by tests. Prefer Python
wrapper/offload patterns over exporting Rust futures unless direct integration
has clear lifecycle/cancellation benefits.

### WP6 — Lifecycle and failure qualification

Exercise interpreter exceptions, bind failure, start failure, cancellation,
double close, drop without explicit close, repeated construction, process exit,
and test-suite parallelism.

### WP7 — Wheel/package matrix

Pin PyO3/maturin, build artifacts, run import/basic-runtime smoke on supported
targets, and record any unavailable target as incomplete rather than claiming
support.

### WP8 — Remote/native conformance

Run equivalent M033 remote and M034 native fixtures for stream/datagram
mutations, Scenario V2 identity, validation/conflict errors, and reset.

### WP9 — Performance and safety review

Record startup/control measurements, ensure Python never enters data-plane hot
loops, inspect unsafe/macro boundary, run dependency/security gates, and decide
whether evidence supports any future generic ABI planning.

### WP10 — Documentation and closure

Document Python native usage, lifecycle rules, remote-versus-native selection,
supported Python/platform matrix, safety boundary, and future-binding decision.
Reconcile roadmap/registry and create exact-candidate closure evidence.

## Invariants and failure semantics

- One server/runtime/control authority exists.
- `eggchaos-embed` is a facade, not a second implementation.
- No raw Rust borrow/pointer/stream/future crosses the public Python contract.
- Python code never executes in the byte/datagram hot path.
- Service close is idempotent and bounded.
- Drop cannot silently leave an unowned service/runtime running.
- Existing deterministic RNG and Scenario V2 identity are unchanged.
- Remote and native supported operations share validation/default semantics.
- No payload or bearer secret enters binding evidence/errors.
- No handwritten unsafe code is introduced under M034.
- No generic C ABI is created.

## Required tests

Rust facade:

- start/health/version/shutdown;
- repeated start/shutdown;
- drop cleanup;
- stream proxy/fault CRUD;
- datagram proxy/fault CRUD;
- connection/association control;
- Scenario V2 validate/compile/run/cancel;
- reset;
- bind failure and invalid config;
- conflict/missing/unsupported errors;
- no detached tasks after close.

Python:

- import smoke;
- context-manager close on normal return and exception;
- explicit close + double close;
- repeated Service instances;
- stream/datagram CRUD;
- representative connection/association snapshot;
- Scenario V2 validate/compile/apply/status/cancel;
- error category/detail mapping;
- invalid identifier and bounds;
- parallel test-process/service instances with independent state;
- interpreter shutdown smoke;
- native versus remote conformance fixtures.

Packaging:

- wheel build/import on every claimed target;
- source distribution build if shipped;
- stable-ABI verification if used;
- generated artifact reproducibility.

Regression:

- all existing workspace tests;
- stream/datagram deterministic trace corpus;
- Scenario V2 fingerprint/namespace golden corpus;
- EggFetch qualification;
- pinned Toxiproxy qualification;
- release/package smoke;
- existing performance budgets.

## Verification

Minimum:

```sh
./scripts/check.sh
cargo test -p eggchaos-embed --all-features
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
./scripts/release-smoke.sh
./scripts/qualify_eggfetch.sh
TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" \
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 \
  ./scripts/qualify_toxiproxy_v2_12.sh
```

Add focused native-binding commands equivalent to:

```sh
./scripts/check_python_native.sh
./scripts/qualify_python_native.sh
./scripts/build_python_native_artifacts.sh
```

Run the existing datagram benchmark and any M034 embedding/control benchmark on
the exact candidate.

## Acceptance criteria

M034 closes only when:

- M032 and M033 are closed;
- a safe Rust embedding facade exists and is independently tested;
- facade lifecycle owns all runtime/tasks without leaks or detached work;
- Python can embed stream and datagram control plus representative Scenario V2
  operations without an admin HTTP hop;
- no raw Tokio/Rust lifetime types cross the Python API;
- errors and validation semantics conform to the native protocol/remote client;
- remote/native conformance fixtures pass for the supported overlap;
- wheel artifacts build and import on every claimed target;
- interpreter/lifecycle failure paths are qualified;
- no handwritten unsafe code was added;
- no unexplained data-plane performance regression exists;
- full Rust/Toxiproxy/EggFetch/Scenario/datagram regressions remain green;
- documentation clearly preserves the remote client as the portability-first
  option;
- closure records a reasoned decision on whether a future generic C ABI has
  enough demonstrated consumers to justify separate planning.

Create
`plans/closure/M034-python-native-embedding-pilot-and-binding-qualification-closure.md`.

## Stop/rejection conditions

Do not close if:

- the binding directly exposes Tokio stream/pinning/future internals;
- Python callbacks run on data-plane hot paths;
- service shutdown can leak threads/tasks;
- nested runtime/event-loop behavior can panic under normal supported use;
- the facade duplicates server/control state;
- native and remote validation/default semantics diverge;
- handwritten unsafe is required without a new ADR;
- wheel support is claimed without runtime smoke;
- deterministic golden identities change;
- a generic C ABI is slipped into the milestone without the required separate
  architecture decision.

## Follow-on activation

M034 activates no automatic native-binding successor.

A generic C ABI may be planned only after M034 closes and there is evidence for
at least one additional concrete native consumer beyond Python, or another
strong cross-language embedding requirement that justifies a stable ABI.

Node native, JNI, .NET P/Invoke, Go/cgo, UniFFI, and WASM remain demand-driven
and require separate planning. Remote Go/Java/.NET clients should normally
reuse the M032 OpenAPI contract instead.
