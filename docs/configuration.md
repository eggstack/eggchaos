# Configuration

Eggchaos reads schema-versioned TOML. Set `version = 1`; omitted optional fields use the documented defaults. The same validation rules apply to file configuration and native API requests.

## Admin listener

The default listener is `127.0.0.1:8475` and is unauthenticated unless you configure a token. To bind beyond loopback, set `public_admin = true` and a non-empty `auth_token`:

```toml
[admin]
bind = "0.0.0.0:8475"
public_admin = true
auth_token = "render-this-value-from-your-secret-store"
```

Eggchaos reads `auth_token` as a literal TOML string. It does not expand environment variables. Do not commit a real token. The admin listener uses HTTP, so protect non-loopback access with a trusted TLS terminator and network controls. The CLI's `--admin-token` option and `EGGCHAOS_ADMIN_TOKEN` variable supply credentials for client requests; they do not configure the server listener.

## Stream proxy example

```toml
version = 1
seed = 7

[admin]
bind = "127.0.0.1:8475"

[[proxy]]
name = "redis"
listen = "127.0.0.1:16379"
upstream = "127.0.0.1:6379"

[[proxy.fault]]
id = "lag"
direction = "downstream"
probability = 1.0
type = "latency"
delay = "200ms"
jitter = "50ms"
```

The proxy listens on `listen` and forwards to the fixed `upstream`. Directions are `upstream` (client to target) and `downstream` (target to client). Faults run in the order listed. Stream probabilities are deterministic per connection and default to `1.0`.

TOML duration strings use `ms`, `us`, or `s`. Native HTTP requests use unsigned integer nanoseconds. Both surfaces compile into the same validated fault model. See [control-plane schemas](control-plane.md#native-v1-request-and-response-schemas) for fault types and API defaults.

## Runtime and proxy bounds

The optional `[runtime]` table controls service-wide limits. Defaults preserve the service builder values.

| TOML key | Unit | Default | Accepted range |
| --- | --- | --- | --- |
| `global_connections` | active connections | `1024` | `1..=1000000` |
| `history` | retained closed records | `256` | `0..=1000000` |
| `relay_buffer_bytes` | bytes | `65536` | `1..=16777216` |
| `termination_grace_ms` | milliseconds | `5000` | `0..=300000` |

Each `[[proxy]]` may set `connect_timeout_ms` (default `5000`, range `1..=300000`), `seed` (default `0`), `enabled` (default `true`), and `max_connections` (optional). The global connection limit still applies. Latency faults may set `max_buffer_bytes` from `1` through `67108864` bytes. Invalid values fail configuration with a field-specific error.

The service half-close policy is not configurable in schema v1. The runtime uses its `Drain` policy.

## Datagram proxies

Use `[[datagram_proxies]]` to configure fixed-target UDP proxies. The collection is optional. The optional `[runtime.datagram]` table sets global proxy, association, history, ingress-slot, and ingress-byte limits.

```toml
[runtime.datagram]
max_proxies = 128
max_associations = 4096
history = 1024
ingress_per_association = 16
max_ingress_queue_bytes = 67108864

[[datagram_proxies]]
name = "dns"
listen = "127.0.0.1:0"
upstream = "127.0.0.1:5353"
max_associations = 256
association_idle_timeout_ms = 60000
max_datagram_size = 65507
max_queued_datagrams = 1024
max_queued_bytes = 4194304
seed = 0
upstream_faults = [{ id = "loss", probability = 0.05, kind = { type = "loss" } }]
```

Each client address has a bounded association with its own connected upstream socket. The listener address may use port `0` to request an available port; the actual bound address is returned by the control API.

Datagram fault types are separate from stream faults:

| Type | Fields |
| --- | --- |
| `delay` | `delay_ns`, optional `jitter_ns` |
| `loss` | none |
| `duplicate` | `additional_copies` |
| `reorder` | `hold_ns` |
| `payload-corrupt` | `bytes` |
| `bandwidth` | `bytes_per_second`, `burst_bytes` |

Datagram probability is evaluated per datagram. Durations use integer nanoseconds in the fault object. Bounds are validated before listener startup, and invalid values fail with a field-specific error. Existing stream proxy settings remain unchanged.
