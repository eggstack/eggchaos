# eggchaos-client (Python)

Remote native control client for eggchaos. It drives the versioned `/v1`
JSON contract (plus Prometheus text on `/metrics`) of an explicitly
selected admin endpoint. Derived from the M032 OpenAPI authority
(`api/openapi/eggchaos-v1.yaml`); the operation table in
`eggchaos_client/_generated.py` is produced by
`scripts/sync_sdk_contract.py` — do not edit it by hand.

No native extension, no FFI, no daemon management: start `eggchaos`
separately (for example `eggchaos serve --config eggchaos.toml`), then
point the client at its admin URL.

## Install

```sh
pip install eggchaos-client
```

The client depends only on the Python standard library.

## Quick start (pytest)

```python
from eggchaos_client import Client, LatencyFault, ProxyCreate

def test_latency_downstream():
    with Client(base_url="http://127.0.0.1:8475") as client:
        assert client.health()["running"] is True
        client.create_proxy(ProxyCreate(
            name="redis", listen="127.0.0.1:0", upstream="127.0.0.1:6379",
        ))
        client.add_fault(
            "redis", "downstream", "lag",
            LatencyFault(delay_ns=200_000_000), probability=0.5,
        )
        # ... run workload through the proxy listener ...
        client.delete_fault("redis", "lag")
        client.delete_proxy("redis")
```

Async usage mirrors the sync surface:

```python
from eggchaos_client import AsyncClient

async with AsyncClient(base_url="http://127.0.0.1:8475") as client:
    await client.health()
```

## Authentication

```python
Client(base_url="http://host:8475", token="admin-secret")
```

The token travels only in the `Authorization: Bearer` header and never
appears in `repr`, errors, or logged models. Loopback listeners without
a configured token accept unauthenticated requests; non-loopback
listeners require the token.

## Errors

- `EggchaosError(status, code, detail)` — the server answered with a
  native error envelope (`not_found`, `conflict`, `invalid`, ...).
- `EggchaosTransportError` — connect/read/timeout failure; the server
  never answered. Mutating calls never retry implicitly.
- `EggchaosContractError` — a response used an unknown discriminator
  (newer server contract than this SDK).

## Notes

- Optional request fields left as `None` are omitted from the JSON
  body, so server defaults apply. Client-side defaults never override
  the wire contract.
- `metrics_text()` returns raw Prometheus text, never JSON.
- Fault/resource identifiers are percent-encoded exactly once.
- Unknown future fault `type` tags raise `EggchaosContractError`
  instead of silently mis-decoding.
