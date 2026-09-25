# eggchaos-native (Python embedding pilot)

In-process eggchaos service for Python test harnesses. This is the M034
native binding pilot: a PyO3 module over the safe `eggchaos-embed` Rust
facade. Managed objects and snapshot/value models only — no Tokio
streams, futures, borrows, or `Instant` cross the boundary.

Remote-control users should prefer `eggchaos-client`
(portability-first, no compiler needed). Use this module when the test
process itself must own the eggchaos lifecycle with no admin HTTP hop.

## Quick start

```python
from eggchaos_native import Fault, Service

with Service(seed=7) as chaos:
    proxy = chaos.create_proxy(
        name="redis", listen="127.0.0.1:0", upstream="127.0.0.1:6379",
    )
    chaos.set_fault(
        "redis",
        Fault.latency("lag", direction="downstream", delay_ns=200_000_000),
    )
    # ... run workload through proxy["bound_addr"] ...
```

Async harnesses should call the blocking API from worker threads
(`asyncio.to_thread`); the binding never exposes Rust futures and holds
no Python event-loop state.

## Lifecycle rules

- `Service(seed=...)` starts an owned runtime; `close()` (or the
  context manager) shuts down listeners and joins tasks idempotently.
- Dropping without `close()` still initiates shutdown; join on drop is
  best-effort by runtime-drop semantics — always prefer explicit close.
- Each `Service` owns independent state; parallel test processes or
  threads use independent instances.
- Errors mirror the remote client categories: `NativeValidationError`,
  `NativeNotFoundError`, `NativeConflictError`,
  `NativeUnsupportedError`, `NativeLifecycleError`, `NativeBindError`,
  `NativeInternalError` (all subclasses of `NativeError`).

## Safety boundary

No handwritten `unsafe` exists in the binding sources; the crate-local
`allow(unsafe_code)` covers PyO3 macro-generated FFI glue only (see the
M034 closure audit). No Python callback runs on data-plane hot paths;
Python never executes per-packet code.
