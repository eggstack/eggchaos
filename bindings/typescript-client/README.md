# @eggstack/eggchaos-client (TypeScript)

TypeScript client for Eggchaos's native `/v1` API and Prometheus `/metrics`
endpoint. It has no runtime dependencies. It does not manage the service
lifecycle: start `eggchaos serve --config eggchaos.toml` separately, then
pass the admin URL to the client.

## Install

The client is not currently published to npm. To build it from a repository
checkout:

```sh
cd bindings/typescript-client
npm ci
npm run build
```

The built package works on Node 18+ (global `fetch`) and in browsers. Its
model layer uses no Node-only primitives. In a consuming project, install the
built checkout with `npm install /path/to/eggchaos/bindings/typescript-client`.

## Quick start

```ts
import { EggchaosClient } from "@eggstack/eggchaos-client";

const client = new EggchaosClient({ baseUrl: "http://127.0.0.1:8475" });

await client.createProxy({
  name: "redis",
  listen: "127.0.0.1:16379",
  upstream: "127.0.0.1:6379",
});
await client.addFault("redis", "downstream", "lag", {
  type: "latency",
  delay_ns: 200_000_000,
}, 1.0);
// ... run workload through the proxy listener ...
await client.deleteFault("redis", "lag");
await client.deleteProxy("redis");
```

Injectable transport and cancellation:

```ts
const controller = new AbortController();
const client = new EggchaosClient({
  baseUrl: "http://127.0.0.1:8475",
  token: "admin-secret", // only if the server has a configured token
  timeoutMs: 5_000,
  fetch, // custom implementation for tests
});
await client.health({ signal: controller.signal });
```

## Errors

- `EggchaosError(status, code, detail)`: the server returned a native
  error envelope.
- `EggchaosTransportError`: connection, read, or request timeout before
  the server answered. Mutating calls are not retried implicitly.
- `EggchaosContractError`: the response used a discriminator unknown to
  this SDK, which may indicate a newer server contract.

When configured, the token is sent in the `Authorization: Bearer` header
and is excluded from errors and serialized models. A non-loopback server
also requires `public_admin = true`. See [admin configuration](../../docs/configuration.md#admin-listener).

## Notes

- Optional fields left `undefined` are omitted from request bodies, so
  server defaults apply.
- `metricsText()` returns raw Prometheus text, never JSON.
- Fault/resource identifiers are percent-encoded exactly once.
