# M029 — Composable Transport Chaos Adapter and Evidence

Status: ready  
Depends on: M028, ADR 005  
Role: cross-project integration transport substrate

## Objective

Refactor the EggFetch integration from a direct-only chaos Dialer into a
composable physical-stream impairment layer that can wrap an arbitrary
`eggfetch_core::Dialer`, while preserving the existing direct convenience
path.

At the same time, freeze a caller-controllable deterministic physical
connection identity seam and expose bounded bidirectional transport evidence
outside the erased EggFetch stream so downstream consumers can correlate their
own observations with the actual eggchaos realization.

M029 is deliberately consumer-neutral. It does not add EggReplay or EggProbe
dependencies or product-specific report types.

## Baseline and dependencies

M007 established `eggchaos-eggfetch::ChaosDialer` and proved that EggFetch can
own HTTP/TLS/pooling above a chaos-wrapped physical stream. M013 and M019
requalified that path after the core/live-policy corrective work.

The current adapter has three limitations for cross-project use:

1. it performs DNS resolution and direct TCP dialing itself, so it cannot
   transparently decorate another route-authoritative Dialer;
2. it owns an internal monotonically increasing connection ordinal with no
   caller-controlled deterministic identity derivation;
3. the useful bidirectional stream evidence is not externally consumable once
   EggFetch receives the type-erased DialStream.

ADR 005 is authoritative for dependency direction, route-versus-impairment
separation, physical-connection identity, evidence, and pooling semantics.

## Scope

### In scope

- A public composable EggFetch Dialer adapter that wraps another Dialer.
- Preservation of a simple direct-dial convenience constructor/type.
- The inner Dialer remains authoritative for resolution, routing,
  authentication, timeout, and connect errors.
- Eggchaos wraps only successfully returned physical streams.
- Caller-configurable deterministic physical connection-key derivation.
- A documented default connection-key policy compatible with current behavior.
- Explicit physical connection ordinal and key evidence.
- Bounded, shareable upstream/downstream evidence for
  `BidirectionalChaosStream`.
- An optional caller-supplied observer/sink that receives connection evidence
  handles or finalized snapshots without an unbounded built-in registry.
- Live policy publication semantics equivalent to the current adapter.
- H1/H2/TLS/pooling regression coverage.
- Composition tests using at least one non-direct inner Dialer fixture.
- Documentation of pooling/multiplexing semantics and evidence lifecycle.

### Non-goals

- No EggReplay or EggProbe dependency.
- No new Eggress production dependency merely to prove composition.
- No new routing grammar or proxy-chain ownership.
- No per-request transport-fault identity.
- No request/header/body inspection.
- No change to fault-local RNG algorithms or existing golden vectors.
- No new stream/datagram fault kinds.
- No Scenario V2 execution changes; those belong to M030.
- No global unbounded connection-history store.
- No promise that a hard reset maps identically through every possible inner
  Dialer transport.

## Affected surfaces

Expected surfaces include:

- `crates/eggchaos-eggfetch/src/lib.rs`;
- `crates/eggchaos-eggfetch/Cargo.toml` if feature decomposition is useful;
- `crates/eggchaos-core/src/stream.rs` for bidirectional evidence only;
- `crates/eggchaos-core/src/lib.rs` re-exports if new evidence types belong in
  core;
- `architecture/eggfetch-integration.md`;
- `docs/eggfetch.md`;
- README embedding examples;
- focused tests/fixtures and M031 qualification inputs.

Avoid changing `eggchaos-server` unless a small shared evidence type genuinely
belongs there; the adapter must not depend on server/runtime state.

## Required composition model

Exact Rust names may vary, but the public model must support behavior
equivalent to:

    inner Dialer
       |
       +-- dial(DialTarget)
              |
              +-- physical DialStream
                     |
                     +-- BidirectionalChaosStream
                            |
                            +-- EggFetch HTTP/TLS/pooling

A generic form such as `ChaosDialer<D>`, a `ChaosLayer<D>`, or an equivalent
constructor is acceptable.

The inner Dialer's `DialError` category and source should pass through without
being collapsed into a generic eggchaos error. Eggchaos-specific construction
errors should occur at adapter configuration time wherever possible rather than
after a successful network dial.

The direct convenience mode may use a small direct Dialer implementation under
the hood, but there must be only one chaos wrapping path.

## Physical connection identity

M029 must freeze one explicit connection identity contract.

Required concepts:

- adapter-local physical connection ordinal, beginning from a documented
  deterministic initial value;
- caller-supplied integration/execution identity or seed namespace;
- caller-supplied connection-key factory/provider, or an equivalent stable
  mechanism;
- a documented default derivation that preserves current ordinal behavior.

A provider may receive stable inputs such as the `DialTarget`, configured
integration identity, and physical connection ordinal. It must not receive or
depend on logical HTTP request order, task IDs, wall-clock timestamps, random
UUIDs, or scheduler order.

Provider failure must fail the dial before bytes are exposed to EggFetch and
must produce a bounded typed error.

Connection-key collisions are allowed only if explicitly caller-selected; the
documentation must explain that equal keys select equal deterministic
fault-local namespaces where all other derivation inputs match.

## Bidirectional evidence model

`BidirectionalChaosStream` must gain a bounded evidence surface equivalent in
quality to the existing directional `StreamEvidence`.

At minimum expose, live or by snapshot:

- connection key;
- observed upstream/downstream generations;
- pending generation where meaningful;
- upstream/downstream seed namespaces;
- upstream/downstream active fault IDs/types with the existing hard bound;
- bytes accepted/forwarded/discarded;
- high-water bytes;
- injected and throttled delay totals where already tracked by the engine;
- transition count where meaningful;
- upstream/downstream termination info;
- deterministic RNG version.

If reusing `StreamEvidence` internally is practical, prefer one authority over
parallel counters. Do not weaken the empty-plan fast path materially merely to
collect evidence.

Evidence reads must be lock-bounded and must not require locking the underlying
network stream.

## Observer/sink contract

Because EggFetch receives a type-erased `DialStream`, consumers need an
out-of-band way to discover the evidence for each physical dial.

Provide an optional observer/sink or equivalent hook invoked when a successfully
wrapped physical stream is created. It should receive only bounded transport
metadata and a shareable evidence handle/snapshot.

Required behavior:

- observer invocation is synchronous and non-blocking by contract, or otherwise
  strictly bounded;
- observer failure cannot corrupt the physical stream after it has been handed
  to EggFetch;
- no payload bytes are exposed;
- no implicit unbounded retention;
- the adapter itself does not spawn detached evidence tasks;
- abrupt stream drop is representable as incomplete/finalization-unknown if the
  implementation cannot reliably emit a final callback.

A small bounded test observer/collector may be provided for tests, but it must
have explicit capacity/eviction semantics if public.

## Live policy semantics

The composable adapter retains one upstream and one downstream `LivePolicy`
authority per configured adapter instance.

Current guarantees remain:

- an already-open pooled connection observes supported live generations;
- transitions occur only after the old preserving queue is drained according
  to existing `BidirectionalChaosStream` semantics;
- no reconnect is required merely to publish a new plan;
- seed namespace publication stays atomic with generation/plan.

M029 must not change standalone server `ControlState` generation semantics.

## Pooling and multiplexing contract

Tests and documentation must make physical ownership explicit.

Required cases:

- two sequential H1 requests reusing one keep-alive connection observe one
  physical connection key;
- two H1 requests forced onto separate connections receive separate ordinals /
  keys under the default provider;
- an H2 connection uses one physical connection key for multiplexed requests;
- a live policy update applies at the physical stream transition boundary, not
  per logical request;
- evidence is associated with the physical connection, not copied or fabricated
  per request.

Do not add APIs that imply transport-fault isolation between H2 streams.

## Ordered work packages

### WP1 — Factor direct dialing from impairment

Separate the existing direct resolve/connect behavior from the chaos wrapping
authority. Preserve current connect timeout/error behavior for the convenience
direct mode.

### WP2 — Generic Dialer composition

Implement the arbitrary-inner-Dialer form and prove that inner route/connect
errors survive with useful `DialErrorKind` provenance.

### WP3 — Deterministic connection-key provider

Add the configurable identity/key seam, define default behavior, bound any
caller strings/labels, and add golden/unit tests for deterministic inputs.

### WP4 — Bidirectional stream evidence

Add one authoritative live evidence implementation for the two direction
engines. Preserve no-fault fast-path behavior and existing termination
semantics.

### WP5 — External evidence observer

Expose evidence handles/snapshots at successful physical dial creation with no
unbounded retention or detached tasks.

### WP6 — H1/H2/TLS/pooling tests

Exercise direct convenience mode, a custom non-direct fixture Dialer,
keep-alive reuse, H2 multiplexing, TLS handshake bytes, live publication,
termination, and cancellation.

### WP7 — Documentation and compatibility

Update public examples and architecture docs. Preserve existing simple
`ChaosDialer` usage where reasonably source-compatible; if an API change is
unavoidable, provide a migration note and keep the direct path trivial.

## Invariants and failure semantics

- The inner Dialer is the sole authority for route/connect behavior.
- Eggchaos never performs a second dial after the inner Dialer succeeds.
- Physical connection key is selected exactly once before the wrapped stream is
  exposed.
- One physical connection has one connection key for its lifetime.
- Logical requests do not change the key.
- Existing fault-local RNG golden vectors are unchanged.
- Evidence is bounded and contains no payload.
- Observer behavior cannot introduce unbounded buffering or detached tasks.
- Live policy transition semantics remain byte-safe.
- A no-fault adapter remains close to the current direct path and does not add
  per-byte heap allocation.

## Required tests

At minimum:

- wrap a fake/custom Dialer and prove its dial target is forwarded exactly;
- inner authentication/rejected/timeout/connection error kinds are preserved;
- direct convenience mode still resolves/connects successfully;
- deterministic provider receives the expected ordinal/target inputs;
- repeated identical provider inputs return identical connection keys;
- default provider ordinals are stable within one adapter instance;
- separate physical connections receive distinct default keys;
- H1 keep-alive reuse uses one key;
- H2 multiplexed requests use one key;
- live upstream/downstream policy updates reach an existing pooled connection;
- upstream/downstream evidence records correct generations, byte counts,
  active faults, and termination;
- evidence handle remains safe to inspect after the network stream is dropped;
- observer is called exactly once per successfully wrapped physical dial;
- failed inner dials do not create false connection evidence;
- no-fault H1/H2 regressions remain green.

## Verification

Minimum:

    cargo fmt --all -- --check
    cargo clippy -p eggchaos-core -p eggchaos-eggfetch --all-targets --all-features -- -D warnings
    cargo test -p eggchaos-core --all-features
    cargo test -p eggchaos-eggfetch --all-features
    cargo test -p eggchaos-eggfetch --all-features --features http2
    cargo test --workspace --all-features
    cargo doc --workspace --all-features --no-deps

Also run the existing EggFetch qualification script if M029 changes any behavior
covered by it:

    ./scripts/qualify_eggfetch.sh

M031 remains the exact-candidate cross-project-boundary qualification gate.

## Acceptance criteria

M029 closes only when:

- arbitrary EggFetch Dialers can be decorated without duplicating their
  route/connect logic;
- the direct convenience path remains available;
- caller-controlled deterministic physical connection identity is implemented
  and documented;
- bidirectional live/final evidence is externally consumable and bounded;
- no EggReplay/EggProbe production dependency is introduced;
- H1/H2/TLS/pooling tests prove physical rather than request-level ownership;
- live publication works through a wrapped non-direct Dialer;
- existing RNG and stream semantics remain unchanged;
- required tests pass on one exact candidate;
- closure evidence identifies the candidate, API compatibility notes, and
  residual limitations.

Create
`plans/closure/M029-composable-transport-chaos-adapter-and-evidence-closure.md`.

## Stop/rejection conditions

Do not close if:

- the adapter resolves/dials independently when an inner Dialer was supplied;
- inner route errors are flattened into opaque eggchaos errors;
- connection identity depends on request order, wall clock, random UUIDs, or
  Tokio scheduling;
- H2 requests are assigned independent transport keys on one physical stream;
- evidence capture requires payload inspection;
- evidence retention is unbounded;
- a live update can discard preserving bytes merely because the wrapper was
  refactored;
- existing direct users require a complex migration without documented
  justification;
- an EggReplay/EggProbe type enters the eggchaos public API.

## Follow-on activation

On clean M029 closure, M030 becomes ready.

If implementation shows that arbitrary Dialer composition cannot preserve the
inner route's typed error semantics through EggFetch's current public API,
stop and document the exact upstream seam required rather than copying route
logic into eggchaos.
