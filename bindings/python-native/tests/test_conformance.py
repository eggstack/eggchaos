"""Remote/native conformance: the same representative scenario through the
M033 remote client (loopback daemon) and the M034 native module must agree
on native views and deterministic identifiers.

Requires EGGCHAOS_ADMIN_URL (remote daemon) and the installed
eggchaos_native wheel. Run via scripts/qualify_python_native.sh.
"""

import os

import pytest

from eggchaos_native import Fault, Service

ADMIN_URL = os.environ.get("EGGCHAOS_ADMIN_URL")
needs_remote = pytest.mark.skipif(not ADMIN_URL, reason="EGGCHAOS_ADMIN_URL not set")


def _remote_flow(client) -> dict:
    from eggchaos_client import LatencyFault, ProxyCreate

    assert client.health()["running"] is True
    created = client.create_proxy(
        ProxyCreate(name="conf", listen="127.0.0.1:0", upstream="127.0.0.1:9")
    )
    fault = client.add_fault(
        "conf", "downstream", "lag", LatencyFault(delay_ns=5_000_000), probability=1.0
    )
    schedule = {
        "version": 2,
        "seed": 21,
        "execution_key": 22,
        "isolation": "strict",
        "cleanup": "restore-initial",
        "phases": [
            {
                "name": "probe",
                "duration_ns": 1_000_000,
                "actions": [
                    {
                        "type": "remove-fault",
                        "proxy": "conf",
                        "direction": "downstream",
                        "id": "lag",
                    }
                ],
            }
        ],
    }
    validated = client.validate_schedule(schedule)
    compiled = client.compile_schedule(schedule)
    proxy = client.get_proxy("conf")
    client.delete_fault("conf", "lag")
    client.delete_proxy("conf")
    return {
        "proxy_seed": created["proxy"]["seed"],
        "fault": fault["fault"],
        "fingerprint": validated["schedule_fingerprint"],
        "compiled_action": compiled["events"][0]["action"],
        "proxy_faults": proxy["downstream_faults"],
    }


def _native_flow() -> dict:
    with Service(seed=0) as chaos:
        assert chaos.health()["running"] is True
        created = chaos.create_proxy(
            name="conf", listen="127.0.0.1:0", upstream="127.0.0.1:9"
        )
        fault = chaos.set_fault(
            "conf",
            Fault.latency("lag", direction="downstream", delay_ns=5_000_000, probability=1.0),
        )
        schedule = {
            "version": 2,
            "seed": 21,
            "execution_key": 22,
            "isolation": "strict",
            "cleanup": "restore-initial",
            "phases": [
                {
                    "name": "probe",
                    "duration_ns": 1_000_000,
                    "actions": [
                        {
                            "type": "remove-fault",
                            "proxy": "conf",
                            "direction": "downstream",
                            "id": "lag",
                        }
                    ],
                }
            ],
        }
        validated = chaos.schedule_validate(schedule)
        compiled = chaos.schedule_compile(schedule)
        proxy = chaos.get_proxy("conf")
        chaos.remove_fault("conf", "lag")
        chaos.delete_proxy("conf")
        return {
            "proxy_seed": created["proxy"]["seed"],
            "fault": fault["fault"],
            "fingerprint": validated["schedule_fingerprint"],
            "compiled_action": compiled["events"][0]["action"],
            "proxy_faults": proxy["downstream_faults"],
        }


@needs_remote
def test_remote_and_native_views_conform():
    import sys

    sys.path.insert(0, os.environ.get("EGGCHAOS_CLIENT_PATH", "bindings/python-client"))
    from eggchaos_client import Client

    with Client(base_url=ADMIN_URL) as client:
        remote = _remote_flow(client)
    native = _native_flow()
    # Timing-independent equality: identities, fault views, fingerprint.
    assert remote["proxy_seed"] == native["proxy_seed"] == 0
    assert remote["fault"] == native["fault"]
    assert remote["fingerprint"] == native["fingerprint"]
    assert remote["compiled_action"] == native["compiled_action"] == "remove-fault"
    assert remote["proxy_faults"] == native["proxy_faults"]
    # Bound addresses differ (separate listeners); names and shapes agree.
    assert remote["fault"]["id"] == "lag"
