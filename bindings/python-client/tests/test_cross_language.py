"""Cross-language fixtures: equivalent SDK inputs serialize to the exact
shared native JSON wire bodies."""

import json
from pathlib import Path

from eggchaos_client import (
    BandwidthFault,
    Client,
    DatagramLossFault,
    DatagramProxyCreate,
    LatencyFault,
    ProxyCreate,
    ScheduleV2,
    ScenarioV1,
    StreamLossFault,
)

FIXTURES = json.loads(
    (Path(__file__).resolve().parent.parent.parent / "_contract" / "cross_language_fixtures.json")
    .read_text(encoding="utf-8")
)["cases"]


class _StubResponse:
    status = 200

    def getheader(self, _name):
        return "application/json"

    def read(self):
        return b"{}"


class _StubConnection:
    def __init__(self):
        self.calls = []

    def request(self, method, path, body=None, headers=None):
        self.calls.append((method, path, json.loads(body) if body else None))

    def getresponse(self):
        return _StubResponse()

    def close(self):
        pass


def _captured(build):
    client = Client(base_url="http://127.0.0.1:9")
    stub = _StubConnection()
    client._connection = stub
    build(client)
    assert len(stub.calls) == 1
    return stub.calls[0]


def _wire(name):
    return next(case["wire"] for case in FIXTURES if case["name"] == name)


def test_proxy_create_serializes_to_shared_wire():
    method, path, body = _captured(
        lambda client: client.create_proxy(
            ProxyCreate(name="redis", listen="127.0.0.1:0", upstream="127.0.0.1:6379")
        )
    )
    assert (method, path) == ("POST", "/v1/proxies")
    assert body == _wire("proxy_create")


def test_stream_faults_serialize_to_shared_wire():
    method, path, body = _captured(
        lambda client: client.add_fault(
            "redis",
            "downstream",
            "lag",
            LatencyFault(delay_ns=200_000_000, jitter_ns=0, max_buffer_bytes=65536),
            probability=0.5,
        )
    )
    assert (method, path) == ("POST", "/v1/proxies/redis/faults")
    assert body == _wire("stream_fault_latency")

    _, _, body = _captured(
        lambda client: client.add_fault("redis", "upstream", "cap", BandwidthFault())
    )
    assert body == _wire("stream_fault_bandwidth_defaults")

    _, _, body = _captured(
        lambda client: client.add_fault(
            "redis",
            "upstream",
            "loss",
            StreamLossFault(loss_rate=0.25, correlation=0.1),
        )
    )
    assert body == _wire("stream_fault_stream_loss")


def test_datagram_fault_serializes_to_shared_wire():
    method, path, body = _captured(
        lambda client: client.add_datagram_fault(
            "dns", "upstream", "loss", DatagramLossFault(), probability=0.25
        )
    )
    assert (method, path) == ("POST", "/v1/datagram-proxies/dns/faults")
    assert body == _wire("datagram_fault_loss")


def test_scenario_documents_serialize_to_shared_wire():
    _, _, body = _captured(
        lambda client: client.apply_scenario(
            ScenarioV1(
                seed=7,
                events=[
                    {
                        "at_ms": 25,
                        "action": {
                            "type": "remove-fault",
                            "proxy": "cache",
                            "direction": "upstream",
                            "id": "delay",
                        },
                    }
                ],
            )
        )
    )
    assert body == _wire("scenario_v1_apply")

    _, _, body = _captured(
        lambda client: client.validate_schedule(
            ScheduleV2(
                seed=7,
                execution_key=11,
                isolation="strict",
                cleanup="restore-initial",
                phases=[],
            )
        )
    )
    assert body == _wire("schedule_v2_validate")


def test_datagram_proxy_create_omits_absent_fields():
    _, _, body = _captured(
        lambda client: client.create_datagram_proxy(
            DatagramProxyCreate(name="dns", listen="127.0.0.1:0", upstream="127.0.0.1:9")
        )
    )
    assert body == {"name": "dns", "listen": "127.0.0.1:0", "upstream": "127.0.0.1:9"}
