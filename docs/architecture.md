# Eggchaos architecture

The dependency direction is inward:

```text
eggchaos-cli -> eggchaos-server -> eggchaos-core
eggchaos-toxiproxy -----------^        |
eggchaos-eggfetch ------------^        +-> Tokio byte streams
```

`eggchaos-core` is protocol-neutral. It owns typed fault plans, validation,
deterministic identity-scoped randomness, and the `ChaosStream<T>` write-side
state machine. It does not know about HTTP, listeners, CLIs, Toxiproxy, or
Eggfetch. The empty plan delegates directly to the wrapped Tokio stream and
does not allocate a queue or timer.

`eggchaos-server` owns fixed-target TCP listeners and uses `eggress-relay` for
bidirectional copying and half-close policy. The native admin plane uses the
generic EggServe H1 leaf runtime when enabled. Compatibility and Eggfetch are
adapters over the server/core authority, never alternate state stores.

The initial release is intentionally bounded: latency queues, connection
counts, request bodies, histories, and control-plane resources have explicit
limits. An abstract hard-reset request is separate from a concrete platform
capability; ordinary `AsyncWrite::poll_shutdown` is never advertised as TCP
RST.

The initial contracts were recorded in [ADR 001](../plans/adrs/001-stream-fault-engine-boundary.md)
and [ADR 002](../plans/adrs/002-determinism-and-live-mutation.md).
