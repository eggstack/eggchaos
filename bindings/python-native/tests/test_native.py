"""Native embedding qualification: lifecycle, control, errors, parallelism."""

import subprocess
import sys
import threading

import pytest

from eggchaos_native import (
    Fault,
    NativeConflictError,
    NativeError,
    NativeLifecycleError,
    NativeNotFoundError,
    NativeValidationError,
    Service,
)


def test_context_manager_closes_on_normal_return_and_exception():
    with Service(seed=1) as chaos:
        assert chaos.health()["running"] is True
        assert chaos.closed() is False
    assert chaos.closed() is True
    with pytest.raises(NativeLifecycleError):
        chaos.health()
    service = Service(seed=1)
    with pytest.raises(RuntimeError):
        with service:
            raise RuntimeError("boom")
    assert service.closed() is True


def test_explicit_close_and_double_close_are_idempotent():
    service = Service(seed=2)
    service.close()
    service.close()
    assert service.closed() is True
    with pytest.raises(NativeLifecycleError):
        service.list_proxies()


def test_repeated_instances_have_independent_state():
    first = Service(seed=3)
    first.create_proxy(name="solo", listen="127.0.0.1:0", upstream="127.0.0.1:9")
    second = Service(seed=3)
    assert second.list_proxies() == []
    with pytest.raises(NativeNotFoundError):
        second.get_proxy("solo")
    first.close()
    second.close()


def test_stream_proxy_and_fault_crud():
    with Service(seed=4) as chaos:
        proxy = chaos.create_proxy(
            name="web", listen="127.0.0.1:0", upstream="127.0.0.1:9"
        )
        assert proxy["proxy"]["running"] is True
        assert proxy["proxy"]["name"] == "web"
        assert len(chaos.list_proxies()) == 1
        fault = chaos.set_fault(
            "web", Fault.latency("lag", direction="downstream", delay_ns=1_000_000)
        )
        assert fault["fault"]["id"] == "lag"
        assert fault["direction"] == "downstream"
        assert chaos.get_fault("web", "lag")["fault"]["id"] == "lag"
        patched = chaos.patch_fault("web", "lag", probability=0.25)
        assert patched["fault"]["probability"] == 0.25
        faults = chaos.list_faults("web")
        assert [f["id"] for f in faults["downstream"]] == ["lag"]
        assert chaos.remove_fault("web", "lag")["deleted"] is True
        with pytest.raises(NativeNotFoundError):
            chaos.get_fault("web", "lag")
        assert chaos.delete_proxy("web")["deleted"] is True


def test_datagram_proxy_and_fault_crud():
    with Service(seed=5) as chaos:
        proxy = chaos.create_datagram_proxy(
            name="dns", listen="127.0.0.1:0", upstream="127.0.0.1:9"
        )
        assert proxy["proxy"]["running"] is True
        fault = chaos.set_datagram_fault(
            "dns", Fault.datagram_loss("loss", direction="upstream", probability=0.5)
        )
        assert fault["fault"]["id"] == "loss"
        assert chaos.get_datagram_fault("dns", "loss")["fault"]["id"] == "loss"
        assert chaos.datagram_associations() == []
        assert chaos.remove_datagram_fault("dns", "loss")["deleted"] is True
        assert chaos.delete_datagram_proxy("dns")["deleted"] is True


def test_connections_history_and_reset():
    with Service(seed=6) as chaos:
        assert chaos.connections() == []
        assert chaos.history() == []
        assert "eggchaos_" in chaos.metrics_text()
        assert chaos.reset()["reset"] is True
        assert chaos.version()["api"] == "v1"


def test_schedule_validate_compile_apply_get_cancel():
    with Service(seed=7) as chaos:
        chaos.create_proxy(name="web", listen="127.0.0.1:0", upstream="127.0.0.1:9")
        schedule = {
            "version": 2,
            "seed": 7,
            "execution_key": 11,
            "isolation": "strict",
            "cleanup": "restore-initial",
            "phases": [
                {
                    "name": "probe",
                    "duration_ns": 1_000_000,
                    "actions": [
                        {
                            "type": "remove-fault",
                            "proxy": "web",
                            "direction": "downstream",
                            "id": "lag",
                        }
                    ],
                }
            ],
        }
        validated = chaos.schedule_validate(schedule)
        assert validated["event_count"] == 1
        assert len(validated["schedule_fingerprint"]) == 64
        compiled = chaos.schedule_compile(schedule)
        assert len(compiled["events"]) == 1
        run = chaos.scenario_apply(schedule)
        assert run["family"] == "v2"
        assert chaos.scenario_get(run["run_id"])["run_id"] == run["run_id"]
        cancelled = chaos.scenario_cancel(run["run_id"])
        assert cancelled["run_id"] == run["run_id"]
        v1 = chaos.scenario_apply({"version": 1, "seed": 3, "events": []})
        assert v1["family"] == "v1"
        assert chaos.scenario_get(v1["run_id"])["run_id"] == v1["run_id"]


def test_error_categories_and_bounds():
    with Service(seed=8) as chaos:
        with pytest.raises(NativeValidationError):
            chaos.create_proxy(
                name="bad name!", listen="127.0.0.1:0", upstream="127.0.0.1:9"
            )
        with pytest.raises(NativeNotFoundError):
            chaos.get_proxy("absent")
        with pytest.raises(NativeNotFoundError):
            chaos.scenario_get(999)
        chaos.create_proxy(name="dup", listen="127.0.0.1:0", upstream="127.0.0.1:9")
        with pytest.raises(NativeConflictError):
            chaos.create_proxy(
                name="dup", listen="127.0.0.1:0", upstream="127.0.0.1:9"
            )
        with pytest.raises(NativeValidationError):
            chaos.set_fault(
                "dup",
                Fault.latency("x", direction="sideways", delay_ns=1),
            )
        assert issubclass(NativeNotFoundError, NativeError)
        assert issubclass(NativeValidationError, NativeError)


def test_parallel_instances_have_independent_state():
    results = []
    lock = threading.Lock()

    def worker(index: int) -> None:
        with Service(seed=100 + index) as chaos:
            name = f"w{index}"
            chaos.create_proxy(name=name, listen="127.0.0.1:0", upstream="127.0.0.1:9")
            chaos.set_fault(
                name, Fault.latency("lag", direction="upstream", delay_ns=1)
            )
            count = len(chaos.list_proxies())
            with lock:
                results.append(count)

    threads = [threading.Thread(target=worker, args=(index,)) for index in range(4)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    assert results == [1, 1, 1, 1]


def test_interpreter_shutdown_without_close_exits_cleanly():
    code = (
        "from eggchaos_native import Service;"
        "s = Service(seed=9);"
        "s.create_proxy(name='x', listen='127.0.0.1:0', upstream='127.0.0.1:9')"
    )
    completed = subprocess.run(
        [sys.executable, "-c", code], capture_output=True, text=True, timeout=60
    )
    assert completed.returncode == 0, completed.stderr
