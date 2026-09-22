# Eggfetch integration

`eggchaos-eggfetch::ChaosDialer` implements Eggfetch's public `Dialer` seam.
It performs bounded direct TCP dialing and returns a
`BidirectionalChaosStream` with live upstream/downstream policies; Eggfetch
continues to own HTTP framing, pooling, destination TLS/SNI, certificate
validation, and retry policy. A deterministic physical connection ordinal is
used in fault seed derivation. Live policy publications engage pooled
connections without reconnecting; resolved downstream terminations surface
as EOF (graceful) or errors (hard reset) on the physical stream.

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
