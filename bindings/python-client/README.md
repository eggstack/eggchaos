# eggchaos-client (Python)

Python client for Eggchaos's native `/v1` API and Prometheus `/metrics`
endpoint. It uses only the Python standard library. It does not manage the
service lifecycle: start `eggchaos serve --config eggchaos.toml` separately,
then pass the admin URL to the client.

## Install

The client is not currently published to PyPI. From the repository root,
install the source package with:

```sh
python -m pip install ./bindings/python-client
```

## Quick start (pytest)

```python
from eggchaos_client import Client, LatencyFault, ProxyCreate

def test_latency_downstream():
    with Client(base_url="http://127.0.0.1:8475") as client:
        assert client.health()["running"] is True
        client.create_proxy(ProxyCreate(
            name="redis", listen="127.0.0.1:16379", upstream="127.0.0.1:6379",
        ))
        client.add_fault(
            "redis", "downstream", "lag",
            LatencyFault(delay_ns=200_000_000), probability=1.0,
        )
        # ... run workload through 127.0.0.1:16379 ...
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

The token is sent in the `Authorization: Bearer` header and is excluded
from `repr`, errors, and serialized models. Loopback listeners do not require
a token by default. A non-loopback listener requires both `public_admin = true`
and a configured token. See [admin configuration](../../docs/configuration.md#admin-listener).

## Errors

- `EggchaosError(status, code, detail)`: the server returned a native
  error envelope such as `not_found`, `conflict`, or `invalid`.
- `EggchaosTransportError`: the connection, read, or request timed out
  before the server answered. Mutating calls are not retried implicitly.
- `EggchaosContractError`: the response used a discriminator unknown to
  this SDK, which may indicate a newer server contract.

## Notes

- Optional request fields left as `None` are omitted from the JSON
  body, so server defaults apply. Client-side defaults never override
  the wire contract.
- `metrics_text()` returns raw Prometheus text, never JSON.
- Fault/resource identifiers are percent-encoded exactly once.
- Unknown future fault `type` tags raise `EggchaosContractError`
  instead of silently mis-decoding.
