# Eggfetch integration

`eggchaos-eggfetch::ChaosDialer` implements Eggfetch's public `Dialer` seam.
It performs bounded direct TCP dialing and returns a `ChaosStream`; Eggfetch
continues to own HTTP framing, pooling, destination TLS/SNI, certificate
validation, and retry policy. A deterministic physical connection ordinal is
used in fault seed derivation.

Fault policy is physical-connection scoped. A pooled HTTP/2 connection can
therefore expose one policy to multiple logical streams; per-request chaos is
not claimed. The adapter never parses HTTP, inspects bodies, or performs TLS
interception. The current core write-side engine applies upstream faults; the
downstream policy handle is retained for the read-side qualification extension
and is intentionally not described as per-request behavior.
