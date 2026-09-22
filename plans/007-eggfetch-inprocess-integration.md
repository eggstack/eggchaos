# M007 — Eggfetch In-Process Integration

Status: blocked  
Depends on: M005  
Parallel with: M006  
Successor gate: M008

## Objective

Provide a small adapter that injects eggchaos stream faults into Eggfetch physical connections through the public `eggfetch_core::Dialer` contract, without a standalone proxy listener and without taking ownership of HTTP or TLS semantics.

This is the strongest Eggstack-specific differentiator for eggchaos.

## User-visible outcome

A Rust application/test can construct an Eggfetch client whose physical streams are fault-wrapped:

```rust
let policy = ChaosPolicy::new(...);

let dialer = ChaosDialer::builder()
    .policy(policy.clone())
    .build();

let client = eggfetch_core::Client::builder()
    .dialer(dialer)
    .build();
```

HTTPS remains verified by Eggfetch; HTTP/1.1 and HTTP/2 continue to be framed/pool-managed by Eggfetch.

Updating the shared eggchaos policy can affect an already-existing wrapped physical connection for supported live mutation classes.

## Preconditions

M005 is closed with:

- stable `ChaosStream`;
- live-policy handle;
- generation transitions;
- deterministic evidence;
- typed failure/capability values.

Recheck current `eggfetch-core` API before implementation. The planning baseline inspected version 0.2.0, where `advanced-routing` publicly exports:

- `Dialer`;
- `DialTarget`;
- `DialStream`;
- `DialError`;
- `DialErrorKind`.

The public contract states that a custom dialer supplies the raw byte stream while Eggfetch owns HTTP framing, destination TLS, SNI, certificate verification, redirects, retries, and response bodies.

If this contract has materially changed, update the plan/ADR before implementation.

## Crate boundary

Implement in `eggchaos-eggfetch`.

Production dependencies should be narrow:

- `eggchaos-core`;
- `eggfetch-core` with the smallest feature slice needed for `Dialer`/H1/H2 qualification;
- Tokio;
- minimal DNS/socket dependency needed for direct dial;
- `thiserror` if adapter-local errors are useful.

Do not depend on `eggchaos-server` for the ordinary adapter. The point is listener-free composition.

A test-only dependency on server fixtures is acceptable if justified.

## Direct physical dialing

The adapter needs an underlying direct dialer because Eggfetch delegates physical stream creation entirely to the custom dialer.

Implement a small direct TCP dialer or compose an existing narrow Eggstack connector if one is publicly reusable without pulling unrelated server/routing machinery.

Requirements:

- resolve `DialTarget.host()` and port;
- bounded connect timeout if the adapter owns one, or clearly delegate connect deadline to Eggfetch's outer connect timeout;
- Happy Eyeballs is not required unless the baseline Eggfetch custom-dialer contract expects caller parity;
- return safe typed `DialError`;
- do not inspect URL path/query/headers; `DialTarget` intentionally contains only host/port;
- never terminate destination TLS here for normal HTTPS.

If using `eggress-outbound` simply for direct dialing would pull a broad graph, do not use it.

## ChaosDialer model

Suggested shape:

```text
ChaosDialer
  direct connector
  shared ChaosPolicyHandle
  seed/connection-key allocator
  optional evidence sink
```

On `dial(target)`:

1. allocate a stable physical connection key;
2. direct-dial the logical host/port;
3. compile/attach the current policy generation for the configured target/policy scope;
4. wrap the raw TCP stream in `ChaosStream`;
5. return it as `eggfetch_core::DialStream`.

The stream object must retain the shared live-policy handle so M005 live changes can be observed after the connection has entered Eggfetch's pool.

## Policy scoping

Do not infer arbitrary policy from full URLs because the Dialer only receives logical host/port.

Provide one of:

- one policy per ChaosDialer/client;
- host/port keyed policy table;
- caller-supplied policy resolver over `DialTarget`.

Prefer the smallest useful initial contract. A one-policy-per-dialer model plus optional target filter is likely sufficient for M007.

Any target map must be bounded and should use canonical host/port normalization.

## Pooling semantics

This area must be documented carefully.

The Eggfetch custom dialer runs when Eggfetch creates a new physical connection. Eggfetch/Hyper may then reuse that physical connection:

- H1 keep-alive: sequential requests can share it;
- H2: concurrent logical streams can share one physical connection.

Therefore:

- changing only what future `dial()` calls return is insufficient for active pooled connections;
- the returned `ChaosStream` must carry the live policy mechanism;
- a physical-stream fault can affect multiple H2 requests simultaneously, which is correct for a network-layer fault;
- a connection-level disconnect/reset affects the whole H2 connection, not one stream;
- per-HTTP-request fault semantics are explicitly out of scope for this adapter.

## TLS ownership

For `https://` requests:

```text
Eggfetch HTTP
   |
Eggfetch TLS/SNI/cert verification
   |
ChaosStream returned by Dialer
   |
TCP socket
```

Because Eggfetch wraps the raw dialed stream with destination TLS above the dialer, byte-stream faults apply to TLS records/on-wire encrypted bytes.

Do not MITM, generate certificates, or inspect HTTP bodies.

Tests should prove that:

- correct SNI/certificate validation still succeeds without faults;
- an invalid certificate still fails according to Eggfetch policy;
- eggchaos does not alter trust roots.

## Failure mapping

Physical dial failure must map safely to `DialErrorKind`:

- connection;
- timeout;
- rejected/other as appropriate.

Injected post-connect faults generally surface later as Eggfetch transport/read/write/timeout failures through the wrapped stream. Do not mislabel them as initial dial failures.

Error strings must not include:

- URL path/query;
- headers;
- credentials;
- body bytes.

Target host/port exposure follows Eggfetch's own safe contract.

## Seed/connection keys

The adapter needs reproducible physical connection identity.

Record:

- adapter run seed;
- physical connection ordinal/key;
- target host/port or a safe stable hash if evidence policy prefers;
- policy generation;
- RNG version.

If the user needs replay independent of connection creation ordering, allow an advanced caller to provide a deterministic connection-key source. Keep the default simple.

## Integration tests

Use real Eggfetch clients and local origins.

### HTTP/1.1

- no fault GET;
- keep-alive second request reuses a wrapped connection where observable;
- downstream latency;
- upstream bandwidth on request upload if streaming support makes this practical;
- downstream bandwidth on streamed body;
- mid-response graceful disconnect;
- blackhole triggers configured Eggfetch timeout;
- fault update affects the already pooled physical connection;
- new generation on new connection after forced close.

### HTTPS

- private/local test certificate accepted through Eggfetch configured trust;
- correct SNI;
- latency/bandwidth apply without TLS interception;
- certificate failure behavior remains Eggfetch-owned.

### HTTP/2

When Eggfetch H2 test profile is available:

- multiple concurrent requests over one physical H2 connection;
- physical downstream latency affects the shared connection as expected;
- connection-level disconnect causes appropriate H2/transport failures for affected streams;
- live policy update on existing H2 physical connection.

Do not promise logical-stream-specific chaos.

### Retry/timeout interplay

- injected connect/dial failure maps to expected Eggfetch failure class;
- blackhole/read delay interacts with total/read timeout as configured;
- retry policy may create a new physical connection and therefore a new deterministic connection key;
- evidence distinguishes first and retry connection decisions.

The test should assert eggchaos behavior, not overfit private Eggfetch implementation details that are not part of its public contract.

## Feature/dependency audit

Record `cargo tree` for `eggchaos-eggfetch`.

Verify:

- no eggchaos server/admin dependency;
- no Python/FFI/Node Eggfetch adapters;
- no unnecessary H3/QUIC for the baseline;
- H2 is enabled only in the qualification feature/job that needs it if that keeps the default adapter smaller.

## Documentation

Provide:

- Rust example;
- architecture diagram showing TLS above `ChaosStream`;
- pooling semantics;
- H2 shared-connection semantics;
- live update behavior;
- timeout/retry caveats;
- security/redaction model.

## Ordered work packages

Execute in this order:

1. **WP1 — Requalify Eggfetch seam:** pin/record the current `eggfetch-core` version/features and compile an external-style minimal `Dialer` fixture before adapter implementation.
2. **WP2 — Direct dial authority:** implement the narrow raw TCP dialing path with safe `DialErrorKind` mapping and no HTTP/TLS ownership.
3. **WP3 — ChaosDialer:** return `ChaosStream`-wrapped physical connections carrying deterministic connection keys and the M005 live policy handle.
4. **WP4 — H1/HTTPS qualification:** prove keep-alive, destination TLS/SNI/cert ownership, timeout, bandwidth, blackhole, disconnect, retry, and redaction behavior.
5. **WP5 — Pooled live updates:** prove an already pooled H1 physical stream observes supported policy changes without a new `dial()`.
6. **WP6 — H2 qualification:** prove shared-physical-connection semantics for concurrent streams, live updates, and connection-level termination under the supported Eggfetch H2 profile.
7. **WP7 — Dependency/docs pass:** minimize feature graph and document physical-stream rather than per-request semantics.
8. **WP8 — Closure pass:** record exact Eggfetch/version/profile evidence and close M007; activate M008 only if M006 is also closed.

## Acceptance criteria

M007 closes only when:

- `ChaosDialer` implements the current public Eggfetch `Dialer` contract;
- no standalone listener is needed;
- HTTPS remains Eggfetch-owned and certificate/SNI tests pass;
- H1 pooling behavior is qualified;
- H2 shared physical connection behavior is qualified when feature-enabled;
- live policy changes affect an already pooled physical connection for supported mutation classes;
- failure mapping/redaction is safe;
- dependency tree is narrow;
- routine/integration tests pass;
- closure evidence is committed.

## Stop/rejection conditions

Stop/revise if:

- current Eggfetch custom dialer no longer allows Eggfetch to own destination TLS;
- adapter requires private Eggfetch APIs;
- live updates only affect future physical connections, contradicting the M007 goal;
- implementation starts parsing HTTP;
- H2 behavior is described per request when the fault actually applies to the physical connection;
- direct dialing requires a broad proxy stack without justification.

## Closure evidence

Create `plans/closure/M007-eggfetch-inprocess-integration-closure.md` containing:

- candidate SHA;
- exact Eggfetch version/features;
- dependency tree;
- H1/HTTPS/H2 results;
- pooled-live-update evidence;
- timeout/retry cases;
- error redaction cases;
- acceptance verdict.

Then update registry:

- M007 -> `closed`;
- M008 becomes `ready` only if M006 is also closed.
