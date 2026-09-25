"""Model unit tests: wire shapes, discriminator decoding, absence semantics."""

import json

import pytest

from eggchaos_client import (
    BandwidthFault,
    BlackholeFault,
    DatagramLossFault,
    DisconnectFault,
    EggchaosContractError,
    LatencyFault,
    LimitDataFault,
    ProxyCreate,
    SliceFault,
    SlowCloseFault,
    datagram_fault_from_dict,
    stream_fault_from_dict,
)


def test_stream_fault_wire_shapes_match_m032_contract():
    cases = [
        (LatencyFault(delay_ns=1, jitter_ns=2, max_buffer_bytes=3),
         {"type": "latency", "delay_ns": 1, "jitter_ns": 2, "max_buffer_bytes": 3}),
        (BandwidthFault(), {"type": "bandwidth"}),
        (BlackholeFault(), {"type": "blackhole", "close_after_ns": None}),
        (LimitDataFault(bytes=7), {"type": "limit-data", "bytes": 7}),
        (SlowCloseFault(delay_ns=9), {"type": "slow-close", "delay_ns": 9}),
        (SliceFault(average_size=10, variation=2, delay_ns=11),
         {"type": "slice", "average_size": 10, "variation": 2, "delay_ns": 11}),
        (DisconnectFault(), {"type": "disconnect", "after_ns": 0, "hard_reset": False}),
    ]
    for fault, expected in cases:
        assert fault.to_dict() == expected
        decoded = stream_fault_from_dict(json.loads(json.dumps(expected)))
        assert decoded.to_dict() == expected


def test_unset_optionals_are_omitted_so_server_defaults_apply():
    assert BandwidthFault().to_dict() == {"type": "bandwidth"}
    assert ProxyCreate(name="a", listen="127.0.0.1:0", upstream="127.0.0.1:1").to_dict() == {
        "name": "a",
        "listen": "127.0.0.1:0",
        "upstream": "127.0.0.1:1",
    }


def test_datagram_fault_decoding_and_unknown_tags():
    assert datagram_fault_from_dict({"type": "loss"}).to_dict() == {"type": "loss"}
    assert datagram_fault_from_dict(
        {"type": "duplicate", "additional_copies": 2}
    ).to_dict() == {"type": "duplicate", "additional_copies": 2}
    with pytest.raises(EggchaosContractError):
        stream_fault_from_dict({"type": "packet-loss"})
    with pytest.raises(EggchaosContractError):
        datagram_fault_from_dict({"type": "latency"})


def test_client_rejects_userinfo_and_non_http_schemes():
    from eggchaos_client import Client

    with pytest.raises(ValueError):
        Client(base_url="http://user:pass@127.0.0.1:8475")
    with pytest.raises(ValueError):
        Client(base_url="ftp://127.0.0.1:21")


def test_token_never_appears_in_repr():
    from eggchaos_client import AsyncClient, Client

    assert "s3cr3t" not in repr(Client(token="s3cr3t"))
    assert "s3cr3t" not in repr(AsyncClient(token="s3cr3t"))
