# ADR 003 — Datagram Impairment Boundary and Semantics

Status: accepted  
Date: 2026-09-24

## Context

The first eggchaos release deliberately models ordered byte streams. Its core contract (`ChaosStream<T>` plus `DirectionEngine`) is built around `AsyncWrite` ownership, byte-conservation, flush/shutdown semantics, TCP-capable termination, and fault probability selected per connection.

UDP has materially different semantics:

- datagrams have message boundaries that must be preserved;
- there is no transport connection whose lifetime can define fault activation;
- delay, jitter, duplication, and bandwidth can change datagram departure order;
- reordering requires more than one datagram to be owned concurrently;
- receiver-side backpressure cannot be propagated end-to-end as TCP flow control;
- per-client response demultiplexing and association expiry belong to a UDP runtime, not the protocol-neutral impairment engine.

The roadmap already states that real packet loss/reordering belongs in a future datagram or lower-layer subsystem rather than being simulated by dropping TCP stream chunks. This ADR activates that direction while preserving the stream subsystem.

Current Eggress has substantial UDP machinery (`eggress-udp`), including bounded associations, per-target connected UDP flows, idle reaping, packet/byte metrics, and test fixtures. Its general relay/runtime surfaces are routing/SOCKS/compatibility oriented, however, and its fixed-target compatibility loop is not the concurrency/association model eggchaos needs. Eggchaos should reuse a stable generic Eggress seam where one exists, but must not import routing/protocol layers merely to avoid a small fixed-target socket owner.

## Decision: a sibling datagram subsystem

Datagram impairment is a sibling to the stream engine, not another `FaultKind` in the existing `FaultPlan`.

The protocol-neutral core will expose datagram-specific concepts, approximately:

```text
DatagramPlan
DatagramFaultSpec
DatagramFaultKind
DatagramDirectionEngine
DatagramLivePolicy
DatagramEvidence
DatagramQueueLimits
ChaosDatagram (public facade where useful)
```

Exact names may vary during implementation, but the separation is architectural:

- stream plans keep stream semantics and connection-level probability;
- datagram plans keep datagram semantics and per-datagram probability;
- common primitives such as `Direction`, `FaultId`, `Probability`, RNG versioning, and seed-mixing helpers may be reused when their semantics remain identical;
- HTTP, CLI, listeners, DNS, `UdpSocket`, SOCKS, QUIC, and Toxiproxy vocabulary remain outside the core datagram engine.

Do not generalize the stream engine into a transport abstraction that obscures these differences.

## Initial datagram fault set

The first native datagram subsystem supports six ordered primitives:

1. `delay` — base delay plus deterministic symmetric jitter clipped at zero;
2. `loss` — deterministic Bernoulli discard per datagram;
3. `duplicate` — emit a bounded configured number/probability of additional copies;
4. `reorder` — selected datagrams receive an additional deterministic hold delay so later datagrams can overtake them;
5. `payload-corrupt` — deterministically mutate a bounded number of payload bytes/bits before the OS sends the datagram;
6. `bandwidth` — token-bucket scheduling of complete datagrams without splitting message boundaries.

Stream-only primitives such as `slice`, `slow-close`, `limit-data`, and TCP disconnect/reset are not automatically ported.

`payload-corrupt` means application payload mutation. The kernel will normally compute a valid UDP checksum over the mutated payload. Eggchaos must not claim this models an invalid on-wire checksum, IP corruption, MTU/path-MTU failure, ECN, TTL, ICMP, NIC queues, or lower-layer fragmentation loss.

## Ordered composition

Datagram fault order is explicit and observable.

For each accepted original datagram, the engine evaluates stages in plan order. Duplication can create additional candidates that continue through subsequent stages. Stable candidate identity is:

```text
(ingress_ordinal, copy_index)
```

where the original is `copy_index = 0` and duplicates use monotonically increasing copy indices for that original.

This makes compositions such as `duplicate -> loss` distinct from `loss -> duplicate` without depending on task scheduling.

## Determinism and probability

Stream `FaultSpec.probability` remains connection activation probability.

Datagram `DatagramFaultSpec.probability` is defined per candidate datagram at that stage. A value of zero never activates; one always activates; intermediate values consume the fault-local deterministic RNG.

Datagram RNG substreams are domain-separated from stream substreams and derive from stable identity:

```text
seed namespace
+ proxy identity
+ association key
+ direction
+ fault identity
+ RNG version
-> fault-local deterministic stream
```

Datagrams pass a given direction engine in a single deterministic ingress order. The engine must not use process-global randomness or spawn one random-owning task per datagram.

Golden vectors must pin the datagram derivation domain separately from the existing stream vectors.

## Queue and overflow semantics

Datagram scheduling uses one bounded direction-local queue governed by both:

- `max_queued_datagrams`;
- `max_queued_bytes`.

Delay, reorder, duplication, and bandwidth all consume this same resource budget. Duplicates count as separate queued datagrams and their bytes count separately.

A deadline-ordered queue should use a stable ordering equivalent to:

```text
(release_at, ingress_ordinal, copy_index)
```

so equal-deadline emission is reproducible.

The default full-queue behavior is explicit drop-newest of the candidate being admitted, with a distinct `queue_overflow` evidence counter. Eggchaos must not describe this as configured network loss. This differs deliberately from stream backpressure: stopping reads from a UDP socket merely shifts eventual loss into an opaque kernel receive queue.

No datagram queue or history may be unbounded.

## Live mutation boundary

A datagram snapshots the complete `(plan, generation, seed_namespace)` used to make its decisions at engine admission.

Queued datagrams retain those decisions until emitted or explicitly discarded. A newer generation applies to subsequently admitted datagrams immediately; old- and new-generation datagrams may therefore coexist in the queue and may depart out of generation order.

This is the datagram transition boundary. Do not copy the stream engine's drain-before-generation-swap state machine, because datagram independence makes admission-time snapshots both simpler and more accurate.

Administrative deletion/disable/shutdown may cancel queued datagrams, but those discards must be accounted separately from configured `loss`.

## Fixed-target runtime association model

The standalone UDP runtime is fixed-target and per-client-association based.

A client source `SocketAddr` maps to one bounded association containing:

- stable association ID/key;
- client address;
- one connected upstream UDP socket to the fixed target;
- upstream and downstream datagram engines/policies/evidence;
- last-activity metadata and lifecycle cancellation.

A separate connected upstream socket per client provides response demultiplexing and permits multiple or unsolicited responses to be routed back to the correct client. A single shared upstream socket with a `send -> recv one response` loop is not an acceptable runtime model.

Idle expiry must not reap an association while either impairment queue still owns delayed datagrams. Explicit operator/service removal may do so with administrative-discard evidence.

## Receive-size correctness

The listener must use a receive buffer large enough to observe the largest supported UDP datagram and perform the configured `max_datagram_size` check after receipt.

It must not size the kernel receive buffer passed to `recv_from` to the configured logical limit, because Tokio/OS UDP receive semantics can truncate and discard the remainder before eggchaos can classify the datagram. Oversized ingress is an explicit recorded drop, not a truncated datagram.

## Eggress reuse boundary

Before adding a production `eggress-udp` dependency, implementation must audit the exact published version used by eggchaos and the public seam required.

Reuse is preferred for a genuinely generic fixed-target association/socket primitive or testkit support. Rejected forms of reuse are:

- depending on Eggress routing/SOCKS/compatibility layers solely to obtain a connected UDP socket;
- copying Eggress source into eggchaos;
- using the current Eggress compatibility fixed-target loop as eggchaos's multi-client runtime;
- introducing mismatched Eggress crate generations into one binary.

If no suitably narrow published seam exists, a small Tokio `UdpSocket` owner in `eggchaos-server` is acceptable and keeps Eggress reuse limited to test/dev surfaces.

## Native control boundary

Datagram proxies and associations are separate native resources rather than adding `transport = "udp"` branches to the existing TCP `/v1/proxies` model.

Expected route family:

```text
/v1/datagram-proxies
/v1/datagram-proxies/{name}
/v1/datagram-proxies/{name}/faults
/v1/datagram-associations
/v1/datagram-associations/{id}
```

Exact methods and DTOs are defined in the implementation plan.

Toxiproxy v2.12 compatibility remains TCP/stream-only. No UDP feature may weaken or silently reinterpret the established compatibility surface.

## Consequences

Positive:

- TCP stream semantics and qualification remain stable;
- UDP loss/reorder/duplication have message-correct semantics;
- deterministic replay does not depend on Tokio task interleaving;
- runtime response demultiplexing is explicit;
- queue overflow is observable rather than hidden in kernel behavior;
- later embedded datagram users can reuse the core without the standalone listener.

Costs:

- there are parallel stream and datagram plan/policy/evidence types;
- the server gains a second transport runtime and registry;
- observability/scenario APIs need transport-specific resource types;
- lower-layer network phenomena remain outside this user-space subsystem.

Those costs are accepted because collapsing the models would produce a smaller type surface at the expense of incorrect semantics.

## Rejected alternatives

### Add UDP variants to `FaultKind` and feed datagrams through `DirectionEngine`

Rejected. Stream byte ownership, flush/shutdown behavior, connection probability, and FIFO buffering do not define datagram loss/reorder/duplication correctly.

### Represent UDP as a byte stream

Rejected. It destroys message boundaries and makes duplication/reordering ambiguous.

### Use one shared upstream UDP socket per proxy

Rejected for the primary runtime. Concurrent clients, multiple responses, and unsolicited responses need unambiguous reply ownership.

### Stop reading the UDP listener when the user-space queue is full

Rejected as the deterministic overflow contract. It delegates loss to opaque OS socket buffers and makes evidence workload/host dependent.

### Make Linux `tc netem` the implementation

Rejected as the primary engine because eggchaos is cross-platform and embeddable. `netem` remains a semantic reference and may be used as optional Linux comparison evidence where practical.

### Expand Toxiproxy compatibility to UDP

Rejected. Toxiproxy v2.12 is a TCP proxy compatibility target; datagram chaos is a native eggchaos subsystem.
