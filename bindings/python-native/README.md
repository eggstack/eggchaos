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

## Platform support (M035)

Wheels are abi3 (`abi3-py311`) built by maturin (pinned `1.9.5` in the
hosted gate). Target selection is host-aware: native-host builds by
default, Apple targets only on a Darwin host (`EGGCHAOS_NATIVE_TARGET`
overrides for intentional cross builds). Support is claimed only where
a matching interpreter actually imports and exercises the wheel:

- Hosted runtime-qualified: Linux x86_64 (`ubuntu-latest`, Python
  3.12) and macOS native architecture (`macos-latest`, Python 3.12),
  via the dedicated `python-native` CI job (`check_python_native.sh`
  + `qualify_python_native.sh`: embed/binding tests, unsafe audit,
  abi3 inspection, import/runtime smoke, remote/native conformance).
- Built-only: macOS cross-arch wheels produced by
  `build_python_native_artifacts.sh` on a Darwin host (import-smoked
  only when a matching interpreter is present).
- Unqualified: Windows and every other platform (no hosted
  native-Python gate, no import/runtime smoke — no support claimed).

No claim is inferred from the Rust CLI artifact matrix. Remote-control
users who need portability without a compiler should prefer
`eggchaos-client`.
