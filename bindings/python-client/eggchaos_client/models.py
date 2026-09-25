"""Typed native models. Absence (`None`) is preserved on the wire: fields
that are `None` are omitted from request bodies so server defaults apply."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Optional, Union

from .errors import EggchaosContractError


def _omit_none(payload: dict[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in payload.items() if value is not None}


# ---------------------------------------------------------------- stream faults


@dataclass
class LatencyFault:
    type: str = "latency"
    delay_ns: int = 0
    jitter_ns: int = 0
    max_buffer_bytes: Optional[int] = None

    def to_dict(self) -> dict[str, Any]:
        return _omit_none(
            {
                "type": self.type,
                "delay_ns": self.delay_ns,
                "jitter_ns": self.jitter_ns,
                "max_buffer_bytes": self.max_buffer_bytes,
            }
        )


@dataclass
class BandwidthFault:
    type: str = "bandwidth"
    bytes_per_second: Optional[int] = None
    burst_bytes: Optional[int] = None

    def to_dict(self) -> dict[str, Any]:
        return _omit_none(
            {
                "type": self.type,
                "bytes_per_second": self.bytes_per_second,
                "burst_bytes": self.burst_bytes,
            }
        )


@dataclass
class BlackholeFault:
    type: str = "blackhole"
    close_after_ns: Optional[int] = None

    def to_dict(self) -> dict[str, Any]:
        # close_after_ns is nullable (not omittable): None means indefinite.
        return {"type": self.type, "close_after_ns": self.close_after_ns}


@dataclass
class LimitDataFault:
    type: str = "limit-data"
    bytes: Optional[int] = None

    def to_dict(self) -> dict[str, Any]:
        return _omit_none({"type": self.type, "bytes": self.bytes})


@dataclass
class SlowCloseFault:
    type: str = "slow-close"
    delay_ns: int = 0

    def to_dict(self) -> dict[str, Any]:
        return {"type": self.type, "delay_ns": self.delay_ns}


@dataclass
class SliceFault:
    type: str = "slice"
    average_size: Optional[int] = None
    variation: int = 0
    delay_ns: int = 0

    def to_dict(self) -> dict[str, Any]:
        return _omit_none(
            {
                "type": self.type,
                "average_size": self.average_size,
                "variation": self.variation,
                "delay_ns": self.delay_ns,
            }
        )


@dataclass
class DisconnectFault:
    type: str = "disconnect"
    after_ns: int = 0
    hard_reset: bool = False

    def to_dict(self) -> dict[str, Any]:
        return {"type": self.type, "after_ns": self.after_ns, "hard_reset": self.hard_reset}


@dataclass
class StreamLossFault:
    """Deterministic userspace stream-chunk loss (ADR 007 / M037).

    Loss is decided in fixed 32 KiB logical chunks keyed to the absolute
    accepted stream offset, so identical byte streams decide identically
    under any caller write fragmentation. This is userspace stream-chunk
    loss, not IP/TCP packet loss.
    """

    type: str = "stream-loss"
    loss_rate: float = 0.0
    correlation: float = 0.0

    def to_dict(self) -> dict[str, Any]:
        return {
            "type": self.type,
            "loss_rate": self.loss_rate,
            "correlation": self.correlation,
        }


StreamFaultKind = Union[
    LatencyFault,
    BandwidthFault,
    BlackholeFault,
    LimitDataFault,
    SlowCloseFault,
    SliceFault,
    DisconnectFault,
    StreamLossFault,
]

_STREAM_FAULTS: dict[str, Any] = {
    "latency": LatencyFault,
    "bandwidth": BandwidthFault,
    "blackhole": BlackholeFault,
    "limit-data": LimitDataFault,
    "slow-close": SlowCloseFault,
    "slice": SliceFault,
    "disconnect": DisconnectFault,
    "stream-loss": StreamLossFault,
}


def stream_fault_from_dict(data: dict[str, Any]) -> StreamFaultKind:
    """Decode a stream fault by its `type` discriminator."""
    tag = data.get("type")
    cls = _STREAM_FAULTS.get(tag) if isinstance(data, dict) else None
    if cls is None:
        raise EggchaosContractError(f"unknown stream fault type: {tag!r}")
    accepted = {f for f in cls.__dataclass_fields__ if f != "type"}
    return cls(**{key: data[key] for key in accepted if key in data})


# --------------------------------------------------------------- datagram faults


@dataclass
class DatagramDelayFault:
    type: str = "delay"
    delay_ns: int = 0
    jitter_ns: int = 0

    def to_dict(self) -> dict[str, Any]:
        return {"type": self.type, "delay_ns": self.delay_ns, "jitter_ns": self.jitter_ns}


@dataclass
class DatagramLossFault:
    type: str = "loss"

    def to_dict(self) -> dict[str, Any]:
        return {"type": self.type}


@dataclass
class DatagramDuplicateFault:
    type: str = "duplicate"
    additional_copies: int = 0

    def to_dict(self) -> dict[str, Any]:
        return {"type": self.type, "additional_copies": self.additional_copies}


@dataclass
class DatagramReorderFault:
    type: str = "reorder"
    hold_ns: int = 0

    def to_dict(self) -> dict[str, Any]:
        return {"type": self.type, "hold_ns": self.hold_ns}


@dataclass
class DatagramCorruptFault:
    type: str = "payload-corrupt"
    bytes: int = 1

    def to_dict(self) -> dict[str, Any]:
        return {"type": self.type, "bytes": self.bytes}


@dataclass
class DatagramBandwidthFault:
    type: str = "bandwidth"
    bytes_per_second: int = 1
    burst_bytes: int = 1

    def to_dict(self) -> dict[str, Any]:
        return {
            "type": self.type,
            "bytes_per_second": self.bytes_per_second,
            "burst_bytes": self.burst_bytes,
        }


DatagramFaultKind = Union[
    DatagramDelayFault,
    DatagramLossFault,
    DatagramDuplicateFault,
    DatagramReorderFault,
    DatagramCorruptFault,
    DatagramBandwidthFault,
]

_DATAGRAM_FAULTS: dict[str, Any] = {
    "delay": DatagramDelayFault,
    "loss": DatagramLossFault,
    "duplicate": DatagramDuplicateFault,
    "reorder": DatagramReorderFault,
    "payload-corrupt": DatagramCorruptFault,
    "bandwidth": DatagramBandwidthFault,
}


def datagram_fault_from_dict(data: dict[str, Any]) -> DatagramFaultKind:
    """Decode a datagram fault by its `type` discriminator."""
    tag = data.get("type")
    cls = _DATAGRAM_FAULTS.get(tag) if isinstance(data, dict) else None
    if cls is None:
        raise EggchaosContractError(f"unknown datagram fault type: {tag!r}")
    accepted = {f for f in cls.__dataclass_fields__ if f != "type"}
    return cls(**{key: data[key] for key in accepted if key in data})


# ------------------------------------------------------------------- resources


@dataclass
class FaultSpec:
    id: str
    kind: StreamFaultKind
    probability: Optional[float] = None

    def to_dict(self) -> dict[str, Any]:
        return _omit_none(
            {"id": self.id, "probability": self.probability, "kind": self.kind.to_dict()}
        )


@dataclass
class DatagramFaultSpec:
    id: str
    kind: DatagramFaultKind
    probability: Optional[float] = None

    def to_dict(self) -> dict[str, Any]:
        return _omit_none(
            {"id": self.id, "probability": self.probability, "kind": self.kind.to_dict()}
        )


@dataclass
class ProxyCreate:
    name: str
    listen: str
    upstream: str
    enabled: Optional[bool] = None
    max_connections: Optional[int] = None
    connect_timeout_ms: Optional[int] = None
    seed: Optional[int] = None

    def to_dict(self) -> dict[str, Any]:
        return _omit_none(
            {
                "name": self.name,
                "listen": self.listen,
                "upstream": self.upstream,
                "enabled": self.enabled,
                "max_connections": self.max_connections,
                "connect_timeout_ms": self.connect_timeout_ms,
                "seed": self.seed,
            }
        )


@dataclass
class ProxyPatch:
    listen: Optional[str] = None
    upstream: Optional[str] = None
    enabled: Optional[bool] = None
    max_connections: Optional[int] = None
    connect_timeout_ms: Optional[int] = None

    def to_dict(self) -> dict[str, Any]:
        return _omit_none(
            {
                "listen": self.listen,
                "upstream": self.upstream,
                "enabled": self.enabled,
                "max_connections": self.max_connections,
                "connect_timeout_ms": self.connect_timeout_ms,
            }
        )


@dataclass
class DatagramProxyCreate:
    name: str
    listen: str
    upstream: str
    max_associations: Optional[int] = None
    association_idle_timeout_ms: Optional[int] = None
    max_queued_datagrams: Optional[int] = None
    max_queued_bytes: Optional[int] = None
    max_datagram_size: Optional[int] = None
    seed: Optional[int] = None
    upstream_faults: list[DatagramFaultSpec] = field(default_factory=list)
    downstream_faults: list[DatagramFaultSpec] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return _omit_none(
            {
                "name": self.name,
                "listen": self.listen,
                "upstream": self.upstream,
                "max_associations": self.max_associations,
                "association_idle_timeout_ms": self.association_idle_timeout_ms,
                "max_queued_datagrams": self.max_queued_datagrams,
                "max_queued_bytes": self.max_queued_bytes,
                "max_datagram_size": self.max_datagram_size,
                "seed": self.seed,
                "upstream_faults": [f.to_dict() for f in self.upstream_faults] or None,
                "downstream_faults": [f.to_dict() for f in self.downstream_faults] or None,
            }
        )


@dataclass
class DatagramProxyPatch:
    enabled: Optional[bool] = None
    listen: Optional[str] = None
    upstream: Optional[str] = None
    max_associations: Optional[int] = None
    association_idle_timeout_ms: Optional[int] = None

    def to_dict(self) -> dict[str, Any]:
        return _omit_none(
            {
                "enabled": self.enabled,
                "listen": self.listen,
                "upstream": self.upstream,
                "max_associations": self.max_associations,
                "association_idle_timeout_ms": self.association_idle_timeout_ms,
            }
        )


@dataclass
class ScenarioAction:
    """A scenario action in native wire form (V1 and V2 share the tags)."""

    type: str
    proxy: str
    direction: str
    faults: Optional[list[dict[str, Any]]] = None
    id: Optional[str] = None

    def to_dict(self) -> dict[str, Any]:
        return _omit_none(
            {
                "type": self.type,
                "proxy": self.proxy,
                "direction": self.direction,
                "faults": self.faults,
                "id": self.id,
            }
        )


@dataclass
class ScenarioV1:
    version: int = 1
    seed: int = 0
    events: list[dict[str, Any]] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return {"version": self.version, "seed": self.seed, "events": self.events}


@dataclass
class ScheduleV2:
    """A Scenario V2 source schedule in native JSON wire form."""

    seed: int
    execution_key: int
    version: int = 2
    isolation: Optional[str] = None
    cleanup: Optional[str] = None
    phases: list[dict[str, Any]] = field(default_factory=list)
    repeat: Optional[dict[str, Any]] = None

    def to_dict(self) -> dict[str, Any]:
        return _omit_none(
            {
                "version": self.version,
                "seed": self.seed,
                "execution_key": self.execution_key,
                "isolation": self.isolation,
                "cleanup": self.cleanup,
                "phases": self.phases,
                "repeat": self.repeat,
            }
        )
