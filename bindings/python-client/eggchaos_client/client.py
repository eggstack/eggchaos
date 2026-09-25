"""Synchronous native control client over HTTP (standard library only)."""

from __future__ import annotations

import http.client
import json
import reprlib
import urllib.parse
from typing import Any, Optional

from . import _generated
from .errors import EggchaosError, EggchaosTransportError
from .models import (
    DatagramFaultKind,
    DatagramProxyCreate,
    DatagramProxyPatch,
    FaultSpec,
    ProxyCreate,
    ProxyPatch,
    ScheduleV2,
    ScenarioV1,
    StreamFaultKind,
    datagram_fault_from_dict,
    stream_fault_from_dict,
)

_OPERATIONS = {(op["method"], op["path"]) for op in _generated.OPERATIONS}


class Client:
    """Remote control client for one eggchaos admin endpoint.

    Usage::

        with Client(base_url="http://127.0.0.1:8475", token="secret") as client:
            client.create_proxy(ProxyCreate(name="redis", listen="127.0.0.1:0",
                                            upstream="127.0.0.1:6379"))
    """

    def __init__(
        self,
        base_url: str = "http://127.0.0.1:8475",
        token: Optional[str] = None,
        timeout: float = 10.0,
    ) -> None:
        parsed = urllib.parse.urlsplit(base_url)
        if parsed.scheme not in ("http", "https"):
            raise ValueError("base_url must use http or https")
        if parsed.username or parsed.password:
            raise ValueError("base_url must not embed userinfo; pass token= instead")
        if not parsed.hostname:
            raise ValueError("base_url must include a host")
        self._base_url = base_url.rstrip("/")
        self._scheme = parsed.scheme
        self._host = parsed.hostname
        self._port = parsed.port or (443 if parsed.scheme == "https" else 80)
        self._token = token
        self._timeout = timeout
        self._connection: Optional[http.client.HTTPConnection] = None

    def __repr__(self) -> str:
        # The token never appears in repr/debug output.
        return f"Client(base_url={self._base_url!r}, timeout={self._timeout!r})"

    def __enter__(self) -> Client:
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()

    def close(self) -> None:
        """Release the owned HTTP transport. Idempotent."""
        connection, self._connection = self._connection, None
        if connection is not None:
            connection.close()

    # ------------------------------------------------------------ transport

    def _connect(self) -> http.client.HTTPConnection:
        if self._connection is None:
            if self._scheme == "https":
                self._connection = http.client.HTTPSConnection(
                    self._host, self._port, timeout=self._timeout
                )
            else:
                self._connection = http.client.HTTPConnection(
                    self._host, self._port, timeout=self._timeout
                )
        return self._connection

    def _request(
        self, method: str, path: str, body: Optional[dict[str, Any]] = None
    ) -> Any:
        headers = {"Accept": "application/json"}
        if self._token is not None:
            headers["Authorization"] = f"Bearer {self._token}"
        payload: Optional[str] = None
        if body is not None:
            headers["Content-Type"] = "application/json"
            payload = json.dumps(body)
        try:
            connection = self._connect()
            connection.request(method, path, body=payload, headers=headers)
            response = connection.getresponse()
            raw = response.read()
            status = response.status
            content_type = response.getheader("Content-Type") or ""
        except (OSError, http.client.HTTPException) as error:
            self._connection = None
            raise EggchaosTransportError(f"transport failure: {error}") from error
        if status in (200, 201, 202):
            if "application/json" in content_type or raw.startswith((b"{", b"[")):
                try:
                    return json.loads(raw.decode("utf-8")) if raw else None
                except (UnicodeDecodeError, json.JSONDecodeError) as error:
                    raise EggchaosTransportError(f"invalid response encoding: {error}") from error
            return raw.decode("utf-8", errors="replace")
        if status == 204 or not raw:
            raise EggchaosError(status, "empty", "empty error response")
        try:
            envelope = json.loads(raw.decode("utf-8"))
            error = envelope.get("error", {})
            raise EggchaosError(status, str(error.get("code", "unknown")), str(error.get("detail", error.get("message", ""))))
        except (UnicodeDecodeError, json.JSONDecodeError, AttributeError):
            detail = reprlib.repr(raw.decode("utf-8", errors="replace"))
            raise EggchaosError(status, "unparseable", detail) from None

    @staticmethod
    def _quote(value: str) -> str:
        return urllib.parse.quote(value, safe="")

    # ---------------------------------------------------------------- service

    def health(self) -> dict[str, Any]:
        """GET /v1/health."""
        return self._request("GET", "/v1/health")

    def version(self) -> dict[str, Any]:
        """GET /v1/version."""
        return self._request("GET", "/v1/version")

    def metrics_text(self) -> str:
        """GET /metrics as raw Prometheus text (never JSON)."""
        headers = {}
        if self._token is not None:
            headers["Authorization"] = "Bearer [REDACTED]"
        try:
            connection = self._connect()
            request_headers = (
                {"Authorization": f"Bearer {self._token}"} if self._token is not None else {}
            )
            connection.request("GET", "/metrics", headers=request_headers)
            response = connection.getresponse()
            raw = response.read()
            if response.status != 200:
                raise EggchaosError(response.status, "metrics", "metrics request failed")
            return raw.decode("utf-8")
        except (OSError, http.client.HTTPException) as error:
            self._connection = None
            raise EggchaosTransportError(f"transport failure: {error}") from error

    def reset(self) -> dict[str, Any]:
        """POST /v1/reset."""
        return self._request("POST", "/v1/reset")

    # ------------------------------------------------------------------ proxies

    def list_proxies(self) -> list[dict[str, Any]]:
        """GET /v1/proxies."""
        return self._request("GET", "/v1/proxies")

    def create_proxy(self, proxy: ProxyCreate) -> dict[str, Any]:
        """POST /v1/proxies."""
        return self._request("POST", "/v1/proxies", proxy.to_dict())

    def get_proxy(self, name: str) -> dict[str, Any]:
        """GET /v1/proxies/{name}."""
        return self._request("GET", f"/v1/proxies/{self._quote(name)}")

    def patch_proxy(self, name: str, patch: ProxyPatch) -> dict[str, Any]:
        """PATCH /v1/proxies/{name}."""
        return self._request("PATCH", f"/v1/proxies/{self._quote(name)}", patch.to_dict())

    def delete_proxy(self, name: str) -> dict[str, Any]:
        """DELETE /v1/proxies/{name}."""
        return self._request("DELETE", f"/v1/proxies/{self._quote(name)}")

    # ------------------------------------------------------------------- faults

    def list_faults(self, proxy: str) -> dict[str, Any]:
        """GET /v1/proxies/{name}/faults."""
        return self._request("GET", f"/v1/proxies/{self._quote(proxy)}/faults")

    def add_fault(
        self, proxy: str, direction: str, fault_id: str, kind: StreamFaultKind,
        probability: Optional[float] = None,
    ) -> dict[str, Any]:
        """POST /v1/proxies/{name}/faults."""
        body: dict[str, Any] = {
            "direction": direction,
            "id": fault_id,
            "kind": kind.to_dict(),
        }
        if probability is not None:
            body["probability"] = probability
        return self._request("POST", f"/v1/proxies/{self._quote(proxy)}/faults", body)

    def get_fault(self, proxy: str, fault_id: str) -> dict[str, Any]:
        """GET /v1/proxies/{name}/faults/{id}."""
        return self._request(
            "GET", f"/v1/proxies/{self._quote(proxy)}/faults/{self._quote(fault_id)}"
        )

    def patch_fault(
        self, proxy: str, fault_id: str,
        probability: Optional[float] = None, kind: Optional[StreamFaultKind] = None,
    ) -> dict[str, Any]:
        """PATCH /v1/proxies/{name}/faults/{id}."""
        body: dict[str, Any] = {}
        if probability is not None:
            body["probability"] = probability
        if kind is not None:
            body["kind"] = kind.to_dict()
        return self._request(
            "PATCH", f"/v1/proxies/{self._quote(proxy)}/faults/{self._quote(fault_id)}", body
        )

    def delete_fault(self, proxy: str, fault_id: str) -> dict[str, Any]:
        """DELETE /v1/proxies/{name}/faults/{id}."""
        return self._request(
            "DELETE", f"/v1/proxies/{self._quote(proxy)}/faults/{self._quote(fault_id)}"
        )

    # --------------------------------------------------------------- connections

    def list_connections(self) -> list[dict[str, Any]]:
        """GET /v1/connections."""
        return self._request("GET", "/v1/connections")

    def get_connection(self, connection_id: int) -> dict[str, Any]:
        """GET /v1/connections/{id}."""
        return self._request("GET", f"/v1/connections/{int(connection_id)}")

    def kill_connection(self, connection_id: int) -> dict[str, Any]:
        """DELETE /v1/connections/{id}."""
        return self._request("DELETE", f"/v1/connections/{int(connection_id)}")

    def history(self) -> list[dict[str, Any]]:
        """GET /v1/history."""
        return self._request("GET", "/v1/history")

    # ---------------------------------------------------------------- scenarios

    def apply_scenario(self, scenario: ScenarioV1 | ScheduleV2 | dict[str, Any]) -> dict[str, Any]:
        """POST /v1/scenarios/apply (V1 document or V2 schedule)."""
        body = scenario.to_dict() if hasattr(scenario, "to_dict") else scenario
        return self._request("POST", "/v1/scenarios/apply", body)

    def validate_schedule(self, schedule: ScheduleV2 | dict[str, Any]) -> dict[str, Any]:
        """POST /v1/scenarios/validate."""
        body = schedule.to_dict() if hasattr(schedule, "to_dict") else schedule
        return self._request("POST", "/v1/scenarios/validate", body)

    def compile_schedule(self, schedule: ScheduleV2 | dict[str, Any]) -> dict[str, Any]:
        """POST /v1/scenarios/compile."""
        body = schedule.to_dict() if hasattr(schedule, "to_dict") else schedule
        return self._request("POST", "/v1/scenarios/compile", body)

    def get_scenario(self, run_id: int) -> dict[str, Any]:
        """GET /v1/scenarios/{id}."""
        return self._request("GET", f"/v1/scenarios/{int(run_id)}")

    def cancel_scenario(self, run_id: int) -> dict[str, Any]:
        """DELETE /v1/scenarios/{id}."""
        return self._request("DELETE", f"/v1/scenarios/{int(run_id)}")

    # ----------------------------------------------------------------- datagrams

    def list_datagram_proxies(self) -> list[dict[str, Any]]:
        """GET /v1/datagram-proxies."""
        return self._request("GET", "/v1/datagram-proxies")

    def create_datagram_proxy(self, proxy: DatagramProxyCreate) -> dict[str, Any]:
        """POST /v1/datagram-proxies."""
        return self._request("POST", "/v1/datagram-proxies", proxy.to_dict())

    def get_datagram_proxy(self, name: str) -> dict[str, Any]:
        """GET /v1/datagram-proxies/{name}."""
        return self._request("GET", f"/v1/datagram-proxies/{self._quote(name)}")

    def patch_datagram_proxy(self, name: str, patch: DatagramProxyPatch) -> dict[str, Any]:
        """PATCH /v1/datagram-proxies/{name}."""
        return self._request(
            "PATCH", f"/v1/datagram-proxies/{self._quote(name)}", patch.to_dict()
        )

    def delete_datagram_proxy(self, name: str) -> dict[str, Any]:
        """DELETE /v1/datagram-proxies/{name}."""
        return self._request("DELETE", f"/v1/datagram-proxies/{self._quote(name)}")

    def list_datagram_faults(self, proxy: str) -> dict[str, Any]:
        """GET /v1/datagram-proxies/{name}/faults."""
        return self._request("GET", f"/v1/datagram-proxies/{self._quote(proxy)}/faults")

    def add_datagram_fault(
        self, proxy: str, direction: str, fault_id: str, kind: DatagramFaultKind,
        probability: Optional[float] = None,
    ) -> dict[str, Any]:
        """POST /v1/datagram-proxies/{name}/faults."""
        body: dict[str, Any] = {
            "direction": direction,
            "id": fault_id,
            "kind": kind.to_dict(),
        }
        if probability is not None:
            body["probability"] = probability
        return self._request(
            "POST", f"/v1/datagram-proxies/{self._quote(proxy)}/faults", body
        )

    def get_datagram_fault(self, proxy: str, fault_id: str) -> dict[str, Any]:
        """GET /v1/datagram-proxies/{name}/faults/{id}."""
        return self._request(
            "GET",
            f"/v1/datagram-proxies/{self._quote(proxy)}/faults/{self._quote(fault_id)}",
        )

    def patch_datagram_fault(
        self, proxy: str, fault_id: str,
        probability: Optional[float] = None, kind: Optional[DatagramFaultKind] = None,
    ) -> dict[str, Any]:
        """PATCH /v1/datagram-proxies/{name}/faults/{id}."""
        body: dict[str, Any] = {}
        if probability is not None:
            body["probability"] = probability
        if kind is not None:
            body["kind"] = kind.to_dict()
        return self._request(
            "PATCH",
            f"/v1/datagram-proxies/{self._quote(proxy)}/faults/{self._quote(fault_id)}",
            body,
        )

    def delete_datagram_fault(self, proxy: str, fault_id: str) -> dict[str, Any]:
        """DELETE /v1/datagram-proxies/{name}/faults/{id}."""
        return self._request(
            "DELETE",
            f"/v1/datagram-proxies/{self._quote(proxy)}/faults/{self._quote(fault_id)}",
        )

    def list_datagram_associations(self) -> list[dict[str, Any]]:
        """GET /v1/datagram-associations."""
        return self._request("GET", "/v1/datagram-associations")

    def get_datagram_association(self, association_id: int) -> dict[str, Any]:
        """GET /v1/datagram-associations/{id}."""
        return self._request("GET", f"/v1/datagram-associations/{int(association_id)}")

    def kill_datagram_association(self, association_id: int) -> dict[str, Any]:
        """DELETE /v1/datagram-associations/{id}."""
        return self._request("DELETE", f"/v1/datagram-associations/{int(association_id)}")

    # ------------------------------------------------------------------ decoding

    @staticmethod
    def decode_stream_fault(data: dict[str, Any]) -> StreamFaultKind:
        """Decode a stream fault response by discriminator."""
        return stream_fault_from_dict(data)

    @staticmethod
    def decode_datagram_fault(data: dict[str, Any]) -> DatagramFaultKind:
        """Decode a datagram fault response by discriminator."""
        return datagram_fault_from_dict(data)

    @staticmethod
    def decode_fault_spec(data: dict[str, Any]) -> FaultSpec:
        """Decode a stream fault spec response."""
        return FaultSpec(
            id=data["id"],
            kind=stream_fault_from_dict(data["kind"]),
            probability=data.get("probability"),
        )
