# Toxiproxy v2.12 compatibility

The `eggchaos-toxiproxy` crate translates the v2.12 toxic vocabulary into the
native `FaultPlan` authority. It supports the seven v2.12 toxic types:
latency, bandwidth, slow_close, timeout, reset_peer, slicer, and limit_data.
Toxicity is a deterministic per-connection activation probability. Slicer
behavior is stream segmentation, not IP packet loss; reset capability is
platform-qualified by the runtime.

The adapter deliberately does not claim current-Toxiproxy `main` extensions
such as `packet_loss`. Compatibility qualification is run by
`scripts/qualify_toxiproxy_v2_12.sh`; its oracle identity and any unavailable
external execution are recorded in M006/M008 closure evidence.
