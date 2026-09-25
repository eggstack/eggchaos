"""Contract drift: the SDK operation registry must equal the shared
snapshot derived from the M032 OpenAPI authority."""

import json
from pathlib import Path

from eggchaos_client import _generated

SNAPSHOT = (
    Path(__file__).resolve().parent.parent.parent
    / "_contract"
    / "operations.json"
)


def test_generated_table_matches_shared_snapshot():
    snapshot = json.loads(SNAPSHOT.read_text(encoding="utf-8"))
    assert [dict(op) for op in _generated.OPERATIONS] == snapshot["operations"]
    assert list(_generated.STREAM_FAULT_TAGS) == snapshot["stream_fault_tags"]
    assert list(_generated.DATAGRAM_FAULT_TAGS) == snapshot["datagram_fault_tags"]
    assert list(_generated.SCENARIO_ACTION_TAGS) == snapshot["scenario_action_tags"]


def test_client_covers_every_snapshot_operation():
    from eggchaos_client import AsyncClient, Client

    snapshot_ids = {op["operation_id"] for op in _generated.OPERATIONS}
    assert set(_generated.OPERATION_METHODS) == snapshot_ids
    for operation_id, method in _generated.OPERATION_METHODS.items():
        assert callable(getattr(Client, method, None)), operation_id
        assert callable(getattr(AsyncClient, method, None)), operation_id
