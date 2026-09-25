"""Native in-process eggchaos service for Python test harnesses.

Remote-control users should prefer `eggchaos_client` (portability-first).
Use this module when the test process itself must own the eggchaos
lifecycle with no admin HTTP hop::

    from eggchaos_native import Fault, Service

    with Service(seed=7) as chaos:
        chaos.create_proxy(name="redis", listen="127.0.0.1:0",
                           upstream="127.0.0.1:6379")
        chaos.set_fault("redis", Fault.latency(
            "lag", direction="downstream", delay_ns=200_000_000))
"""

from .eggchaos_native import (
    Fault,
    NativeBindError,
    NativeConflictError,
    NativeError,
    NativeInternalError,
    NativeLifecycleError,
    NativeNotFoundError,
    NativeUnsupportedError,
    NativeValidationError,
    Service,
)

__all__ = [
    "Fault",
    "NativeBindError",
    "NativeConflictError",
    "NativeError",
    "NativeInternalError",
    "NativeLifecycleError",
    "NativeNotFoundError",
    "NativeUnsupportedError",
    "NativeValidationError",
    "Service",
]
