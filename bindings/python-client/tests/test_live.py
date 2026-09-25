"""Real-server integration: equivalent flows through sync and async clients.

Requires EGGCHAOS_ADMIN_URL (set by scripts/qualify_language_clients.sh).
Skipped otherwise so unit gates never need a server.
"""

import asyncio
import os

import pytest

from eggchaos_client import (
    AsyncClient,
    Client,
    DatagramLossFault,
    DatagramProxyCreate,
    EggchaosError,
    LatencyFault,
    ProxyCreate,
    ScheduleV2,
    ScenarioV1,
)

ADMIN_URL = os.environ.get("EGGCHAOS_ADMIN_URL")
AUTH_URL = os.environ.get("EGGCHAOS_AUTH_URL")
needs_server = pytest.mark.skipif(not ADMIN_URL, reason="EGGCHAOS_ADMIN_URL not set")
needs_auth_server = pytest.mark.skipif(not AUTH_URL, reason="EGGCHAOS_AUTH_URL not set")


def stream_flow(client: Client) -> None:
    assert client.health()["running"] is True
    assert client.version()["api"] == "v1"
    created = client.create_proxy(
        ProxyCreate(name="pytest", listen="127.0.0.1:0", upstream="127.0.0.1:9")
    )
    assert created["proxy"]["name"] == "pytest"
    assert len(client.list_proxies()) >= 1
    assert client.get_proxy("pytest")["name"] == "pytest"
    fault = client.add_fault(
        "pytest", "downstream", "lag", LatencyFault(delay_ns=1_000_000), probability=0.5
    )
    assert fault["fault"]["id"] == "lag"
    assert client.get_fault("pytest", "lag")["fault"]["id"] == "lag"
    assert client.patch_fault("pytest", "lag", probability=0.25)["fault"]["probability"] == 0.25
    faults = client.list_faults("pytest")
    assert {f["id"] for f in faults["downstream"]} >= {"lag"}
    assert isinstance(client.list_connections(), list)
    assert isinstance(client.history(), list)
    assert "eggchaos_" in client.metrics_text()
    run = client.apply_scenario(ScenarioV1(seed=1, events=[]))
    assert "run_id" in run
    assert client.get_scenario(run["run_id"])["run_id"] == run["run_id"]
    schedule = ScheduleV2(
        seed=7,
        execution_key=11,
        phases=[
            {
                "name": "probe",
                "duration_ns": 1_000_000,
                "actions": [
                    {
                        "type": "remove-fault",
                        "proxy": "pytest",
                        "direction": "downstream",
                        "id": "lag",
                    }
                ],
            }
        ],
    )
    assert "schedule_fingerprint" in client.validate_schedule(schedule)
    assert len(client.compile_schedule(schedule)["events"]) == 1
    assert client.delete_fault("pytest", "lag")["deleted"] is True
    assert client.delete_proxy("pytest")["deleted"] is True
    assert client.reset()["reset"] is True


def datagram_flow(client: Client) -> None:
    created = client.create_datagram_proxy(
        DatagramProxyCreate(name="pytest-dns", listen="127.0.0.1:0", upstream="127.0.0.1:9")
    )
    assert created["proxy"]["name"] == "pytest-dns"
    fault = client.add_datagram_fault(
        "pytest-dns", "upstream", "loss", DatagramLossFault(), probability=0.5
    )
    assert fault["fault"]["id"] == "loss"
    assert client.get_datagram_fault("pytest-dns", "loss")["fault"]["id"] == "loss"
    assert client.list_datagram_faults("pytest-dns")["upstream"]["faults"][0]["id"] == "loss"
    assert isinstance(client.list_datagram_associations(), list)
    with pytest.raises(EggchaosError) as excinfo:
        client.get_datagram_proxy("absent")
    assert excinfo.value.code == "not_found"
    assert excinfo.value.status == 404
    assert client.delete_datagram_fault("pytest-dns", "loss")["deleted"] is True
    assert client.delete_datagram_proxy("pytest-dns")["deleted"] is True


@needs_server
def test_sync_client_full_flow():
    with Client(base_url=ADMIN_URL) as client:
        stream_flow(client)
        datagram_flow(client)


@needs_server
def test_async_client_full_flow():
    async def run() -> None:
        async with AsyncClient(base_url=ADMIN_URL) as client:
            assert (await client.health())["running"] is True
            created = await client.create_proxy(
                ProxyCreate(name="pytest-a", listen="127.0.0.1:0", upstream="127.0.0.1:9")
            )
            assert created["proxy"]["name"] == "pytest-a"
            fault = await client.add_fault(
                "pytest-a", "upstream", "lag", LatencyFault(delay_ns=500), probability=1.0
            )
            assert fault["fault"]["id"] == "lag"
            assert (await client.get_fault("pytest-a", "lag"))["fault"]["id"] == "lag"
            schedule = ScheduleV2(
                seed=3,
                execution_key=5,
                phases=[
                    {
                        "name": "probe",
                        "duration_ns": 1_000_000,
                        "actions": [
                            {
                                "type": "remove-fault",
                                "proxy": "pytest-a",
                                "direction": "upstream",
                                "id": "lag",
                            }
                        ],
                    }
                ],
            )
            assert "schedule_fingerprint" in await client.validate_schedule(schedule)
            dgram = await client.create_datagram_proxy(
                DatagramProxyCreate(
                    name="pytest-a-dns", listen="127.0.0.1:0", upstream="127.0.0.1:9"
                )
            )
            assert dgram["proxy"]["name"] == "pytest-a-dns"
            assert (await client.metrics_text()).count("eggchaos_") >= 1
            assert (await client.reset())["reset"] is True
            assert (await client.delete_proxy("pytest-a"))["deleted"] is True
            assert (await client.delete_datagram_proxy("pytest-a-dns"))["deleted"] is True

    asyncio.run(run())


@needs_auth_server
def test_auth_failure_against_token_protected_server():
    with Client(base_url=AUTH_URL, token="wrong-token") as client:
        with pytest.raises(EggchaosError) as excinfo:
            client.health()
        assert excinfo.value.status == 403
        assert excinfo.value.code == "unauthorized"
    with Client(base_url=AUTH_URL, token="correct-token") as client:
        assert client.health()["running"] is True


@needs_server
def test_path_encoding():
    with Client(base_url=ADMIN_URL) as client:
        client.create_proxy(
            ProxyCreate(name="pytest", listen="127.0.0.1:0", upstream="127.0.0.1:9")
        )
        client.add_fault("pytest", "upstream", "a/b", LatencyFault(delay_ns=1), probability=1.0)
        assert client.get_fault("pytest", "a/b")["fault"]["id"] == "a/b"
        client.delete_proxy("pytest")
