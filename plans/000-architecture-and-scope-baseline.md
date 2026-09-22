# M000 — Architecture and Scope Baseline

Status: closed  
Date: 2026-09-22

## Objective

Establish a researched, implementation-ready architecture for eggchaos before code is added to the fresh repository.

The baseline must answer:

- what eggchaos owns;
- what it deliberately delegates to Eggstack siblings;
- what compatibility target is concrete;
- what fault semantics are required;
- where reproducibility, buffering, live mutation, and reset behavior are risky;
- what the first implementation sequence is;
- what evidence is required before the first release.

## Repository baseline

At investigation time `eggstack/eggchaos` was an empty repository whose description was “stream-fault engine for testing chaotic network conditions.”

There is therefore no legacy API, source layout, dependency graph, or compatibility surface to preserve.

Initial language/toolchain baseline:

- Rust edition 2021;
- MSRV 1.89 to align with current Eggstack networking repositories;
- Tokio async runtime;
- workspace-wide `unsafe_code = "deny"` unless a later explicit ADR approves a narrow exception.

## External reference baseline

### Toxiproxy

Latest tagged release inspected: Shopify Toxiproxy v2.12.0, published 2025-03-18.

The v2.12.0 toxic set is:

- latency;
- bandwidth;
- slow_close;
- timeout;
- reset_peer;
- slicer;
- limit_data.

The v2.12 API exposes JSON endpoints for proxy CRUD, `/populate`, toxic CRUD, `/reset`, `/version`, and Prometheus-compatible `/metrics`. Toxic records have name, type, stream, toxicity, and attributes; stream is upstream or downstream.

Current Toxiproxy `main` also contains `packet_loss`, but that file is absent from the v2.12.0 tag. Eggchaos compatibility therefore targets v2.12.0 first and treats later `main` additions as separately versioned future work.

Toxiproxy's custom-toxic guidance establishes two important behavioral constraints:

1. toxic updates may interrupt active pipelines, so bytes already accepted by a toxic must not be accidentally lost when an administrative update occurs;
2. latency needs buffering so the act of delaying chunks does not unintentionally impose a throughput cap.

These constraints are adopted as eggchaos design requirements without adopting Toxiproxy's Go channel implementation.

### Contemporary Rust comparison

Trixter/tokio-netem demonstrates that composable Tokio `AsyncRead`/`AsyncWrite` adapters are a viable Rust-native model for delay, throttle, slicing, termination, corruption, and runtime controls.

Eggchaos does not depend on Trixter. It uses the comparison to validate the stream-adapter boundary while differentiating through Eggstack integration, reproducibility/evidence, Toxiproxy compatibility, and a stricter semantic distinction between stream corruption and real network packet loss.

## Eggstack reuse investigation

### eggress

Current Eggress workspace version inspected: 1.0.7; MSRV 1.89.

`eggress-relay` is a small published crate whose job is exactly the transport primitive eggchaos needs: generic asynchronous bidirectional relay over Tokio-compatible streams, directional byte accounting, directional failures, and explicit half-close drain policy. It knows nothing about listeners, routing, TLS, metrics, or proxy protocols.

Decision: use `eggress-relay` as the relay authority. Do not fork or copy its poll loop into eggchaos.

`eggress-testkit` already supplies echo, half-close, HTTP-origin, slow-I/O, and fragmented-stream fixtures.

Decision: use it as a dev dependency where its public fixtures match eggchaos qualification needs. Add eggchaos-specific fixtures only when semantics differ.

`eggress-outbound` supplies listener-free outbound proxy-chain execution.

Decision: defer it behind a future optional feature. The first release connects directly to fixed upstream targets.

`eggress-admin` is not a generic admin server; it is coupled to Eggress config/routing/metrics/UDP/reverse state.

Decision: do not depend on it.

### eggfetch

Current `eggfetch-core` version inspected: 0.2.0; MSRV inherited as 1.89.

Its `advanced-routing` feature publicly exports `Dialer`, `DialTarget`, `DialStream`, and typed `DialError`. A caller-owned dialer supplies the raw byte stream while Eggfetch retains HTTP framing, destination TLS, SNI, verification, redirects, retries, pooling, and response semantics.

Decision: build `eggchaos-eggfetch` as a later adapter implementing this public seam. No Eggfetch source modification is required for the planned first integration.

The CLI control client can also use `eggfetch-core` with a minimal H1 profile against the local native admin API, avoiding a second ad-hoc HTTP client.

### eggserve

Current EggServe workspace version inspected: 0.2.0; MSRV 1.89.

`eggserve-server` is the generic H1 connection/runtime/service authority. It accepts caller services and has no filesystem/static requirement. `eggserve-primitives` is transport-neutral.

Decision: use these leaf crates for the native admin HTTP service. Avoid `eggserve-core` unless a later need requires its broader compatibility/multiprotocol composition.

## Ownership boundaries

### eggchaos-core owns

- fault definitions/config validation;
- directional fault pipeline;
- bounded buffering/backpressure;
- deterministic seed derivation and random decisions;
- generic fault-wrapped Tokio stream types;
- per-connection fault runtime state;
- native fault outcome/evidence values;
- generic stream capability vocabulary.

It does not own listeners, HTTP, CLI, Toxiproxy JSON shapes, Prometheus exposition, or upstream proxy chains.

### eggchaos-server owns

- fixed-target proxy definitions;
- listener bind/adoption;
- direct upstream TCP dialing;
- connection IDs/ordinals;
- connection admission and lifecycle;
- proxy/fault registry snapshots;
- composition of client/upstream `ChaosStream` values around `eggress-relay`;
- graceful shutdown;
- concrete TCP reset capability implementation;
- metrics/events;
- embeddable service handles.

### eggchaos-cli owns

- command parsing;
- human output;
- stable JSON output;
- config entrypoint;
- native admin client calls.

No network fault behavior lives in the CLI.

### eggchaos-toxiproxy owns

- Toxiproxy v2.12 JSON request/response types;
- compatibility route table;
- field/default/error translation;
- v2.12 toxic-name/attribute mapping;
- differential oracle harness support.

It does not own fault execution.

### eggchaos-eggfetch owns

- `eggfetch_core::Dialer` implementation;
- direct physical dialing needed by that adapter;
- mapping eggchaos failures into safe `DialErrorKind`/messages;
- shared live policy handle integration.

It does not own HTTP semantics.

## Core stream composition

For a standalone proxied connection:

```text
client socket
  read ----------------------------------------------+
  write <- downstream fault writer <----------------|---+
                                                       |
                                                       | eggress-relay
                                                       |
upstream socket                                       |
  write <- upstream fault writer <--------------------+
  read ------------------------------------------------>
```

In implementation terms, each full-duplex stream can be wrapped in a type whose reads pass through and whose writes are processed by the directional fault engine. The wrapper around the target stream applies upstream faults; the wrapper around the client stream applies downstream faults. The resulting streams are passed to `eggress_relay::relay_with_options`.

This preserves Eggress's half-close behavior and makes fault direction unambiguous.

## Fault-state requirements

### Latency

Use a bounded queue of accepted segments with release deadlines. The implementation must permit multiple delayed segments to be queued so configured latency does not automatically collapse throughput.

When the buffer bound is reached, propagate backpressure by returning `Pending`; do not allocate without bound.

### Bandwidth

Use a monotonic-time token bucket or equivalently specified limiter. The configuration must make burst capacity explicit. Mutation must not create an accidental unbounded burst.

### Timeout / blackhole

Native semantics should distinguish:

- blackhole indefinitely until policy changes;
- blackhole then terminate after a configured duration.

Dropped-by-design bytes are counted as deliberately discarded evidence, not transport success.

### Byte limit

Track forwarded bytes per connection/direction/fault instance and terminate at the exact configured boundary. Partial writes at the boundary must be covered by tests.

### Slow close

Delay shutdown propagation without delaying unrelated in-flight reads/writes. Shutdown must remain cancellable by service shutdown.

### Slicing

Split accepted writes into bounded subsegments; optional inter-slice delay is deterministic under a seed. This is stream segmentation, not MTU emulation.

### Disconnect/reset

Generic core supports an explicit termination request/capability. A true TCP RST requires a concrete TCP transport capability and therefore belongs at the server/dialer edge. Expose capability truthfully:

- graceful close supported;
- half-close supported;
- hard reset supported / unsupported.

Never report a generic stream close as a guaranteed TCP RST.

## Deterministic randomness

The RNG contract is versioned public behavior for evidence/replay.

M001 should reserve a `RngVersion`; M002 should implement and freeze golden vectors.

Recommended algorithm for v1: an explicitly implemented, non-cryptographic SplitMix64 state/derivation function, avoiding a dependency whose reproducibility could change across releases. If implementation chooses a crate instead, the plan must be amended to document how cross-version replay stability is guaranteed.

Sub-seeds are derived from a run seed and stable connection/fault dimensions. Never consume randomness from one process-global sequence shared across concurrent tasks.

## Configuration/live update model

Configuration is immutable by generation at registry level.

M003 can begin with new-connections-only snapshots if necessary. M005 must add active-connection mutation through shared per-proxy/per-fault handles with explicit generation transitions.

A live structural update must not discard bytes that were already accepted by a preserving fault stage. Either drain the old state before activating the new stage or retain the state until its buffered bytes are resolved.

## Native vs compatibility semantics

Native APIs should use precise terms. In particular, user-space chunk dropping must not be marketed as real packet loss.

Toxiproxy adapter fields retain v2.12 names exactly where required for clients.

Current-main features beyond v2.12 receive no compatibility claim until separately planned and differentially tested.

## Dependency sketch

Expected initial workspace:

```text
eggchaos-core
  tokio
  tokio-util
  bytes
  thiserror
  serde (feature-gated if practical)
  arc-swap or equivalent snapshot primitive

eggchaos-server
  eggchaos-core
  eggress-relay = 1.0.7
  eggserve-primitives = 0.2.0
  eggserve-server = 0.2.0
  tokio / tokio-util
  serde / serde_json / toml
  tracing
  prometheus-client
  socket2 (only if required for portable reset capability)

eggchaos-cli
  eggchaos-server (serve/local embedding as appropriate)
  eggfetch-core = 0.2.0, minimal H1 features
  clap
  serde / serde_json
  tracing-subscriber

eggchaos-toxiproxy
  eggchaos-server
  serde / serde_json

eggchaos-eggfetch
  eggchaos-core
  eggfetch-core = 0.2.0 with advanced-routing-compatible slice
  tokio
```

Exact feature flags are frozen by M001 after a compile/feature-tree inspection.

## Initial non-goals

The first release does not implement:

- TLS interception or HTTP request rewriting;
- arbitrary destination forward-proxy routing;
- SOCKS server behavior;
- UDP/datagram impairment;
- SSH/Shadowsocks/Trojan/QUIC proxy protocols;
- general `eggress-embed` parity;
- kernel/netem-like packet scheduling;
- a plugin ABI;
- Python/Node/FFI bindings;
- distributed chaos coordination;
- persistence/database state.

## Acceptance evidence for M000

M000 is closed because:

- the empty repository state was verified;
- current Eggress, Eggfetch, and EggServe public seams were inspected;
- Toxiproxy v2.12.0 and current-main distinctions were investigated;
- architectural ownership and non-goals are explicit;
- numbered implementation plans M001–M008 are registered;
- roadmap, parity, verification, and ADR documents are present.

The next active handoff is M001 only.
