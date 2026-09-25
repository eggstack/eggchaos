"""Asynchronous native control client sharing the sync models and errors.

The async surface reuses the synchronous transport on worker threads
(:func:`asyncio.to_thread`), so models, error mapping, auth, and wire
semantics are defined exactly once. No synchronous method depends on a
global event loop.
"""

from __future__ import annotations

import asyncio
from typing import Any, Optional

from .client import Client
from .models import (
    DatagramFaultKind,
    DatagramProxyCreate,
    DatagramProxyPatch,
    ProxyCreate,
    ProxyPatch,
    ScheduleV2,
    ScenarioV1,
    StreamFaultKind,
)


class AsyncClient:
    """Async wrapper around :class:`Client` with identical call semantics."""

    def __init__(
        self,
        base_url: str = "http://127.0.0.1:8475",
        token: Optional[str] = None,
        timeout: float = 10.0,
    ) -> None:
        self._client = Client(base_url=base_url, token=token, timeout=timeout)

    def __repr__(self) -> str:
        return f"AsyncClient({self._client!r})"

    async def __aenter__(self) -> AsyncClient:
        return self

    async def __aexit__(self, *exc: Any) -> None:
        await self.close()

    async def close(self) -> None:
        """Release the owned HTTP transport. Idempotent."""
        await asyncio.to_thread(self._client.close)

    async def _call(self, method: str, *args: Any, **kwargs: Any) -> Any:
        func = getattr(self._client, method)
        return await asyncio.to_thread(func, *args, **kwargs)

    async def health(self) -> dict[str, Any]:
        return await self._call("health")

    async def version(self) -> dict[str, Any]:
        return await self._call("version")

    async def metrics_text(self) -> str:
        return await self._call("metrics_text")

    async def reset(self) -> dict[str, Any]:
        return await self._call("reset")

    async def list_proxies(self) -> list[dict[str, Any]]:
        return await self._call("list_proxies")

    async def create_proxy(self, proxy: ProxyCreate) -> dict[str, Any]:
        return await self._call("create_proxy", proxy)

    async def get_proxy(self, name: str) -> dict[str, Any]:
        return await self._call("get_proxy", name)

    async def patch_proxy(self, name: str, patch: ProxyPatch) -> dict[str, Any]:
        return await self._call("patch_proxy", name, patch)

    async def delete_proxy(self, name: str) -> dict[str, Any]:
        return await self._call("delete_proxy", name)

    async def list_faults(self, proxy: str) -> dict[str, Any]:
        return await self._call("list_faults", proxy)

    async def add_fault(
        self, proxy: str, direction: str, fault_id: str, kind: StreamFaultKind,
        probability: Optional[float] = None,
    ) -> dict[str, Any]:
        return await self._call("add_fault", proxy, direction, fault_id, kind, probability)

    async def get_fault(self, proxy: str, fault_id: str) -> dict[str, Any]:
        return await self._call("get_fault", proxy, fault_id)

    async def patch_fault(
        self, proxy: str, fault_id: str,
        probability: Optional[float] = None, kind: Optional[StreamFaultKind] = None,
    ) -> dict[str, Any]:
        return await self._call("patch_fault", proxy, fault_id, probability, kind)

    async def delete_fault(self, proxy: str, fault_id: str) -> dict[str, Any]:
        return await self._call("delete_fault", proxy, fault_id)

    async def list_connections(self) -> list[dict[str, Any]]:
        return await self._call("list_connections")

    async def get_connection(self, connection_id: int) -> dict[str, Any]:
        return await self._call("get_connection", connection_id)

    async def kill_connection(self, connection_id: int) -> dict[str, Any]:
        return await self._call("kill_connection", connection_id)

    async def history(self) -> list[dict[str, Any]]:
        return await self._call("history")

    async def apply_scenario(
        self, scenario: ScenarioV1 | ScheduleV2 | dict[str, Any]
    ) -> dict[str, Any]:
        return await self._call("apply_scenario", scenario)

    async def validate_schedule(self, schedule: ScheduleV2 | dict[str, Any]) -> dict[str, Any]:
        return await self._call("validate_schedule", schedule)

    async def compile_schedule(self, schedule: ScheduleV2 | dict[str, Any]) -> dict[str, Any]:
        return await self._call("compile_schedule", schedule)

    async def get_scenario(self, run_id: int) -> dict[str, Any]:
        return await self._call("get_scenario", run_id)

    async def cancel_scenario(self, run_id: int) -> dict[str, Any]:
        return await self._call("cancel_scenario", run_id)

    async def list_datagram_proxies(self) -> list[dict[str, Any]]:
        return await self._call("list_datagram_proxies")

    async def create_datagram_proxy(self, proxy: DatagramProxyCreate) -> dict[str, Any]:
        return await self._call("create_datagram_proxy", proxy)

    async def get_datagram_proxy(self, name: str) -> dict[str, Any]:
        return await self._call("get_datagram_proxy", name)

    async def patch_datagram_proxy(
        self, name: str, patch: DatagramProxyPatch
    ) -> dict[str, Any]:
        return await self._call("patch_datagram_proxy", name, patch)

    async def delete_datagram_proxy(self, name: str) -> dict[str, Any]:
        return await self._call("delete_datagram_proxy", name)

    async def list_datagram_faults(self, proxy: str) -> dict[str, Any]:
        return await self._call("list_datagram_faults", proxy)

    async def add_datagram_fault(
        self, proxy: str, direction: str, fault_id: str, kind: DatagramFaultKind,
        probability: Optional[float] = None,
    ) -> dict[str, Any]:
        return await self._call(
            "add_datagram_fault", proxy, direction, fault_id, kind, probability
        )

    async def get_datagram_fault(self, proxy: str, fault_id: str) -> dict[str, Any]:
        return await self._call("get_datagram_fault", proxy, fault_id)

    async def patch_datagram_fault(
        self, proxy: str, fault_id: str,
        probability: Optional[float] = None, kind: Optional[DatagramFaultKind] = None,
    ) -> dict[str, Any]:
        return await self._call(
            "patch_datagram_fault", proxy, fault_id, probability, kind
        )

    async def delete_datagram_fault(self, proxy: str, fault_id: str) -> dict[str, Any]:
        return await self._call("delete_datagram_fault", proxy, fault_id)

    async def list_datagram_associations(self) -> list[dict[str, Any]]:
        return await self._call("list_datagram_associations")

    async def get_datagram_association(self, association_id: int) -> dict[str, Any]:
        return await self._call("get_datagram_association", association_id)

    async def kill_datagram_association(self, association_id: int) -> dict[str, Any]:
        return await self._call("kill_datagram_association", association_id)
