# Eggfetch integration

`eggchaos-eggfetch::ChaosDialer` implements Eggfetch's public `Dialer` seam.
It decorates an arbitrary caller-selected inner dialer: the inner dialer
remains authoritative for resolution, routing, authentication, timeout, and
connect errors (its `DialError` kind and source pass through untouched),
and eggchaos wraps only the successfully returned physical stream in a
`BidirectionalChaosStream` with live upstream/downstream policies. The
direct convenience form (`ChaosDialer::new` / `with_policies`) composes the
same single wrapping path over a small built-in direct TCP dialer.
Eggfetch continues to own HTTP framing, pooling, destination TLS/SNI,
certificate validation, and retry policy.

Connection identity is caller-controlled and deterministic. Each adapter
assigns an adapter-local physical connection ordinal starting at `1` for
each successfully wrapped dial, then derives the connection key through a
configurable `ConnectionKeyProvider` fed only by `(ordinal, DialTarget,
integration identity)`. The default provider returns the ordinal, matching
historical behavior. Providers must not depend on request order, task IDs,
wall-clock time, random UUIDs, or scheduler order; provider failure fails
the dial before any byte reaches Eggfetch. Equal keys select equal
deterministic fault namespaces, so key collisions are meaningful only when
explicitly caller-selected.

Evidence is bounded, transport-level, and payload-free. Every wrapped
stream exposes a shareable `LiveBidirectionalEvidence` handle (connection
key, per-direction generations, seed namespaces, active faults, byte
counters, delay totals, termination) that stays readable after the stream
is dropped. An optional synchronous `ConnectionObserver` receives one
report per successfully wrapped dial before Eggfetch takes ownership;
failed inner dials create no evidence. The adapter retains no connection
history and spawns no background tasks.

Live policy publications engage pooled connections without reconnecting;
resolved downstream terminations surface as EOF (graceful) or errors (hard
reset) on the physical stream.

Fault policy is physical-connection scoped. A pooled HTTP/2 connection can
therefore expose one policy to multiple logical streams; per-request chaos is
not claimed. The adapter never parses HTTP, inspects bodies, or performs TLS
interception. Both directional policy handles are attached to the physical
stream and are intentionally not described as per-request behavior.

The default crate feature keeps Eggfetch on its native HTTP/1.1 profile. Enable
`eggchaos-eggfetch/http2` for the qualification profile; this adds Eggfetch's
HTTP/2 transport without changing the adapter boundary. In both profiles,
Eggfetch performs TLS/SNI and certificate validation above the returned
physical stream. A physical connection-level fault can consequently affect
multiple pooled HTTP/2 requests, while per-request chaos is out of scope.
