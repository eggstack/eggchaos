"""Native control client for eggchaos.

Remote control over the versioned `/v1` JSON contract (plus Prometheus
text on `/metrics`). Derived from the M032 OpenAPI authority; see
`eggchaos_client._generated` for the operation table.
"""

from .async_client import AsyncClient
from .client import Client
from .errors import EggchaosContractError, EggchaosError, EggchaosTransportError
from .models import (
    BandwidthFault,
    BlackholeFault,
    DatagramBandwidthFault,
    DatagramCorruptFault,
    DatagramDelayFault,
    DatagramDuplicateFault,
    DatagramFaultSpec,
    DatagramLossFault,
    DatagramProxyCreate,
    DatagramProxyPatch,
    DatagramReorderFault,
    DisconnectFault,
    FaultSpec,
    LatencyFault,
    LimitDataFault,
    ProxyCreate,
    ProxyPatch,
    ScenarioAction,
    ScenarioV1,
    ScheduleV2,
    SliceFault,
    SlowCloseFault,
    StreamLossFault,
    datagram_fault_from_dict,
    stream_fault_from_dict,
)

__all__ = [
    "AsyncClient",
    "BandwidthFault",
    "BlackholeFault",
    "Client",
    "DatagramBandwidthFault",
    "DatagramCorruptFault",
    "DatagramDelayFault",
    "DatagramDuplicateFault",
    "DatagramFaultSpec",
    "DatagramLossFault",
    "DatagramProxyCreate",
    "DatagramProxyPatch",
    "DatagramReorderFault",
    "DisconnectFault",
    "EggchaosContractError",
    "EggchaosError",
    "EggchaosTransportError",
    "FaultSpec",
    "LatencyFault",
    "LimitDataFault",
    "ProxyCreate",
    "ProxyPatch",
    "ScenarioAction",
    "ScenarioV1",
    "ScheduleV2",
    "SliceFault",
    "SlowCloseFault",
    "StreamLossFault",
    "datagram_fault_from_dict",
    "stream_fault_from_dict",
]
