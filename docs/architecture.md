# Eggchaos architecture

The dependency direction is inward:

```text
eggchaos-cli -> eggchaos-server -> eggchaos-protocol -> eggchaos-experiment -> eggchaos-core
eggchaos-toxiproxy -> server/core (adapter, no separate state store)
eggchaos-eggfetch -> core (implements `eggfetch_core::Dialer`)
```

`eggchaos-core` is protocol-neutral. It owns typed stream and datagram fault
plans, validation, deterministic identity-scoped randomness, the
`ChaosStream<T>` write-side state machine, and a sibling whole-datagram
scheduler. The datagram engine handles application payloads only: corruption
does not model invalid UDP checksums or lower-layer packet faults. Core does
not know about HTTP, listeners, CLI, Toxiproxy, Eggfetch, or UDP sockets. The
empty stream plan delegates directly to the wrapped Tokio stream and does not
allocate a queue or timer.

`eggchaos-experiment` is the consumer-neutral Scenario V2 authority:
schedule semantics, the shared expected-generation driver, the
prepare/arm/start lifecycle with one monotonic epoch, and the
in-process stream policy target. It depends only on `eggchaos-core`.

`eggchaos-server` owns fixed-target TCP listeners and uses `eggress-relay` for
bidirectional copying and half-close policy. The native admin plane uses the
generic EggServe H1 leaf runtime when enabled. Compatibility and Eggfetch are
adapters over the server/core authority, never alternate state stores.
`eggchaos-eggfetch::ChaosDialer` decorates an arbitrary caller-selected
`Dialer` with deterministic connection identity and bounded transport
evidence.

The initial release is intentionally bounded: latency queues, connection
counts, request bodies, histories, and control-plane resources have explicit
limits. An abstract hard-reset request is separate from a concrete platform
capability; ordinary `AsyncWrite::poll_shutdown` is never advertised as TCP
RST.

The initial contracts were recorded in [ADR 001](../plans/adrs/001-stream-fault-engine-boundary.md)
and [ADR 002](../plans/adrs/002-determinism-and-live-mutation.md).

## Fault semantics

`poll_write` may report a write accepted as soon as the direction engine owns
the bytes inside its bounded queue; physical delivery is not required first.
`poll_flush` is the barrier guaranteeing every preserving accepted byte has
reached the inner writer. When the bound is full, `poll_write` returns
`Pending` after arming the release timer. It never reports zero-length
success for a full queue and never allocates beyond the configured limit.

Release-baseline execution:

- latency: each accepted segment gets an independent
  `accept_time + base_delay + deterministic_jitter` deadline. Segments
  accepted together drain as a burst; the base delay is not serialized once
  per write. Jitter is symmetric around the base delay with total delay
  floored at zero. Byte order is preserved.
- bandwidth: integer fixed-point token bucket. Sustained rate is
  `bytes_per_second`, capacity is `burst_bytes`, and the bucket starts full.
  Excess bytes stay queued and release as tokens refill from monotonic
  (Tokio-clock) elapsed time. A long idle period grants at most one burst.
- blackhole/timeout: `close_after = None` discards indefinitely until a
  policy transition or runtime cancellation. `close_after = Some(d)`
  discards until the deadline, then publishes a graceful termination request
  that fires even with no further application write.
- limit_data: accepts and forwards at most the remaining byte count and
  returns only the accepted prefix length. When the limit reaches zero, a
  graceful termination is published after the accepted prefix resolves; the
  caller suffix is never reported as accepted. Later writes fail
  deterministically with `ConnectionAborted` once the prefix drains.
- slicer: deterministic symmetric sizes in
  `[average - variation, average + variation]` (lower bound one) drawn from
  the fault-local SplitMix64-v1 stream, with the configured delay applied
  between logical slices rather than once per caller write.
- disconnect: publishes a termination request at `now + after`
  (`after == ZERO` means the first contract boundary). `hard_reset = true`
  requests a hard reset; otherwise graceful. Bytes accepted before the
  deadline still drain.
- slow_close: delays shutdown only, never ordinary writes.
- stream-loss (`stream-loss`, ADR 007): deterministic userspace
  stream-chunk loss, not IP/TCP packet loss. Loss decisions are keyed to
  the absolute accepted stream offset in fixed 32 KiB logical grains, so
  identical byte streams decide identically under any caller write
  fragmentation. Each active fault draws from its own domain-separated
  chunk RNG stream with burst correlation
  (`min(1, loss_rate + correlation)` after a dropped chunk); a byte range
  is discarded when any active stream-loss fault drops it, counted once.
  Dropped bytes resolve immediately without retaining payload; preserving
  bytes flow through the latency/bandwidth/slicer pipeline. The limit
  counts discards toward exact N-byte termination, blackhole stays
  dominant while active (stream-loss state freezes), and the frozen
  seven-slot activation arrays are untouched: stream loss reports through
  additive `stream_loss_*` evidence counters.

Termination is a durable level-triggered `TerminationHandle` shared with the
embedding runtime: the first published request wins, late waiters still
observe it, and a live-policy transition can never erase it. `eggchaos-core`
never applies TCP-specific behavior; the runtime edge maps the signal to
shutdown or reset. A generation transition drains old-generation preserving
bytes before swapping engines; byte-limit and RNG state restart per
generation while a due termination survives the swap.

## Datagram runtime

The native UDP runtime is a fixed-target sibling to the TCP proxy. Each client
socket address owns one connected upstream UDP socket and independent
upstream/downstream datagram fault engines, so multiple and unsolicited target
responses return to the correct client. Association counts, queues, ingress
buffering, and history are bounded. Oversized datagrams are classified after
receiving into a full-size UDP buffer; no truncated prefix is forwarded. Idle
expiry waits until both impairment queues and pre-engine ingress are empty.
Administrative listener shutdown or association removal cancels queued work
and records it separately from configured loss and queue overflow. UDP
associations are opaque datagrams and do not add QUIC or IP-layer semantics.
