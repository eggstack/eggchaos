# ADR 001 — Stream Fault Engine Boundary

Status: accepted  
Date: 2026-09-22

## Context

Eggchaos needs to support two materially different execution modes:

1. standalone fixed-target TCP proxying;
2. in-process wrapping of caller-owned Tokio byte streams, especially through `eggfetch_core::Dialer`.

Eggress already owns a correct generic bidirectional relay. Reimplementing that relay in eggchaos would duplicate half-close, directional error, and byte-accounting logic. Conversely, putting chaos semantics into `eggress-relay` would make a general-purpose Eggress primitive depend on a test-specific policy domain.

The fault engine also must not assume HTTP because primary use cases include Redis, PostgreSQL, arbitrary TLS, custom protocols, and future Eggstack transports.

## Decision

The canonical fault primitive is a protocol-neutral wrapper over a caller-owned Tokio-compatible full-duplex stream.

Conceptually:

```rust
pub struct ChaosStream<T> {
    inner: T,
    write_engine: DirectionEngine,
    // read side is pass-through in v1
}
```

The wrapper implements `AsyncRead` by delegating to `inner` and implements `AsyncWrite` through a directional impairment engine.

For a standalone connection:

- the client-side wrapper's write engine is the downstream plan;
- the upstream-side wrapper's write engine is the upstream plan;
- the two wrapped streams are passed to `eggress_relay::relay_with_options`.

The fault engine owns accepted-byte buffering and impairment state. Eggress remains responsible for bidirectional relay and half-close orchestration.

## Why write-side impairment

Write-side composition makes direction explicit and works with the existing Eggress relay without requiring that relay to expose its internal copy loop.

If `eggress-relay` reads bytes from the client and writes them to the target wrapper, the target wrapper can delay/throttle/slice/drop/terminate exactly the upstream direction. The inverse holds for downstream.

This also maps naturally to the Eggfetch `Dialer` integration: the dialer returns a wrapped physical stream and Eggfetch continues to own HTTP and TLS above it.

## Internal engine shape

The implementation should prefer one heterogeneous `DirectionEngine` state machine over a deeply nested generic wrapper type per fault. Reasons:

- native fault lists are runtime data;
- faults can be added/updated dynamically;
- deeply nested concrete types do not map cleanly to JSON/TOML configuration;
- ordered fault execution and per-fault state need inspection/evidence.

A plausible internal shape is:

```text
FaultPlan
  -> compiled DirectionProgram
       -> ordered FaultRuntime entries
            Latency
            Bandwidth
            Timeout
            LimitData
            SlowClose
            Slice
            Disconnect/Reset request
```

The exact enum/state split is implementation detail, but there must be one compilation/validation authority from config to runtime behavior.

## Required AsyncWrite correctness

A fault writer must obey the Tokio `AsyncWrite` contract.

If `poll_write` reports `Ready(Ok(n))`, those n bytes are either:

- committed to the inner writer; or
- owned by a bounded internal buffer whose later delivery/discard semantics are deterministic.

It must never report bytes accepted and then lose them because a future administrative update simply dropped an old buffer.

If internal bounds prevent accepting more data, return `Pending` after arranging a wakeup when capacity becomes available.

`poll_flush` must flush all bytes that preserving faults have accepted. `poll_shutdown` must respect the active close fault while remaining cancellable by runtime shutdown.

## Fault ordering

Fault order is explicit and preserved.

A configuration such as latency -> slice is observably different from slice -> latency if each slice gets its own delay. The core model therefore stores an ordered fault list rather than a map.

Toxiproxy compatibility preserves the order exposed by the compatibility collection where external behavior depends on it.

## Connection-level vs direction-level behavior

Most faults are direction-level. Some effects are connection-level:

- total connection duration;
- hard reset capability;
- operator kill;
- service shutdown.

Those are coordinated by the server/dialer supervisor and represented to the directional engines via cancellation/termination signals. Do not smuggle listener/runtime lifecycle into `eggchaos-core`.

## Consequences

Positive:

- core is reusable outside the standalone daemon;
- Eggress relay behavior is reused rather than forked;
- HTTP remains outside the fault engine;
- dynamic configuration can compile into one inspectable runtime model;
- Eggfetch integration is natural.

Costs:

- a correct buffered `AsyncWrite` implementation is more complex than sleeping around `copy_bidirectional`;
- hard TCP reset cannot be fully generic and requires a capability edge;
- live structural mutation requires explicit generation/state transition machinery.

Those costs are accepted because they are the semantics eggchaos exists to get right.

## Rejected alternatives

### Modify eggress-relay to add fault hooks

Rejected. It would couple a generic relay primitive to a test/chaos policy domain and create cross-repository release coordination for every new fault.

### Reimplement bidirectional relay in eggchaos

Rejected. Half-close and directional relay correctness already exist in `eggress-relay`.

### HTTP middleware as the primary abstraction

Rejected. It cannot cover arbitrary TCP protocols or encrypted payloads without interception.

### Kernel netem as the primary implementation

Rejected for the primary cross-platform library/CI use case. Kernel netem may later be an external oracle for lower-layer impairment, but it requires OS-specific privileges and does not solve embeddable stream faults.
