# @eggstack/eggchaos-client (TypeScript)

Remote native control client for eggchaos. It drives the versioned `/v1`
JSON contract (plus Prometheus text on `/metrics`) of an explicitly
selected admin endpoint. Derived from the M032 OpenAPI authority
(`api/openapi/eggchaos-v1.yaml`); `src/generated.ts` is produced by
`scripts/sync_sdk_contract.py` — do not edit it by hand.

No native addon, no FFI, no daemon management: start `eggchaos`
separately (for example `eggchaos serve --config eggchaos.toml`), then
point the client at its admin URL.

## Install

```sh
npm install @eggstack/eggchaos-client
```

No runtime dependencies; works on Node 18+ (global `fetch`) and in
browsers. The model layer uses no Node-only primitives.

## Quick start

```ts
import { EggchaosClient } from "@eggstack/eggchaos-client";

const client = new EggchaosClient({ baseUrl: "http://127.0.0.1:8475" });

await client.createProxy({
  name: "redis",
  listen: "127.0.0.1:0",
  upstream: "127.0.0.1:6379",
});
await client.addFault("redis", "downstream", "lag", {
  type: "latency",
  delay_ns: 200_000_000,
}, 0.5);
// ... run workload through the proxy listener ...
await client.deleteFault("redis", "lag");
await client.deleteProxy("redis");
```

Injectable transport and cancellation:

```ts
const controller = new AbortController();
const client = new EggchaosClient({
  baseUrl,
  token: "admin-secret",
  timeoutMs: 5_000,
  fetch, // custom implementation for tests
});
await client.health({ signal: controller.signal });
```

## Errors

- `EggchaosError(status, code, detail)` — native error envelope.
- `EggchaosTransportError` — connect/read/timeout failure. Mutating
  calls never retry implicitly.
- `EggchaosContractError` — unknown discriminator (newer contract).

Tokens travel only in the `Authorization: Bearer` header and never
appear in errors or serialized models.

## Notes

- Optional fields left `undefined` are omitted from request bodies, so
  server defaults apply.
- `metricsText()` returns raw Prometheus text, never JSON.
- Fault/resource identifiers are percent-encoded exactly once.
