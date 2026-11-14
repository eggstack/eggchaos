# eggchaos-native

In-process Eggchaos service for Python test harnesses. This alpha PyO3 package wraps the safe `eggchaos-embed` facade and exposes managed objects and value snapshots, not Rust streams, futures, borrows, or clock types.

The package is not currently published to PyPI. Build it from a checkout with Python 3.11 or newer, Rust 1.89 or newer, and maturin 1.9.5:

```sh
python -m pip install maturin==1.9.5
cd bindings/python-native
maturin develop
```

Activate the Python environment where you want to install the module before running `maturin develop`.

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
    # Send test traffic to proxy["bound_addr"].
```

The API is synchronous. In an async test harness, call it from a worker thread with `asyncio.to_thread`. The binding does not expose Rust futures or Python event-loop state.

## Lifecycle and errors

- `Service(seed=...)` starts an owned runtime. `close()` and the context manager shut down listeners and join service tasks.
- Dropping a service starts shutdown but does not join tasks synchronously. Prefer explicit `close()` or the context manager.
- Each `Service` owns independent state, so separate tests can use separate instances.
- Errors are subclasses of `NativeError`: `NativeValidationError`, `NativeNotFoundError`, `NativeConflictError`, `NativeUnsupportedError`, `NativeLifecycleError`, `NativeBindError`, and `NativeInternalError`.

## Safety and platform support

The binding contains no handwritten `unsafe` code. PyO3 generates the FFI glue. Python callbacks do not run on data-plane hot paths.

The package builds abi3 wheels for Python 3.11 and newer. CI currently exercises Linux x86_64 and the native macOS architecture using Python 3.12. Windows and other targets are not qualified. For separate-process control, use the source-only Python remote client in `bindings/python-client`; it talks to the admin API over HTTP.
