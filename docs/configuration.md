# Configuration

Native configuration is schema-versioned TOML. The canonical runtime model
is the typed `eggchaos-core::FaultPlan`; adapters must convert into it rather
than implementing their own validation.

The file/API surface is the schema-v1 TOML model plus the versioned native
control API. The supported native
contract is the Rust constructors in `eggchaos-core`: fault IDs are bounded,
probabilities are finite and within `0..=1`, slice variation is smaller than
average size, and required capacities/rates are non-zero.

The schema-v1 compiler validates required non-zero values and reports the
field name in a configuration error. Invalid values such as a zero bandwidth
burst are rejected without panicking. Admin bearer tokens remain accepted as
strings in TOML, but native configuration and admin debug formatting always
redact their values.

The `Direction` names are `upstream` (client to target) and `downstream`
(target to client). A plan is ordered, and a connection-local fault activation
decision is derived from the policy seed namespace, proxy identity, connection key,
direction, and fault identity. No process-global RNG is used. Each published
policy generation carries its own seed namespace: manual updates retain the
current namespace, Scenario V1 runs publish namespaces derived from
`(scenario seed, run id, event index)`, and Scenario V2 runs publish
namespaces derived from `(scenario seed, execution key, schedule
fingerprint, compiled event index)` with no run_id input; see
`docs/control-plane.md` for the replay limits this implies.

## Runtime and proxy bounds

The optional `[runtime]` table controls existing service bounds. Defaults
preserve the previous builder values:

| TOML key | Unit | Default | Accepted range |
| --- | --- | --- | --- |
| `global_connections` | active connections | `1024` | `1..=1000000` |
| `history` | retained closed records | `256` | `0..=1000000` |
| `relay_buffer_bytes` | bytes | `65536` | `1..=16777216` |
| `termination_grace_ms` | milliseconds | `5000` | `0..=300000` |

Each `[[proxy]]` may set `connect_timeout_ms` (default `5000`, range
`1..=300000`) and `seed` (default `0`). `max_connections` remains optional;
the service-wide connection limit bounds active sockets. Latency faults may set `max_buffer_bytes` from
1 through 67108864. Invalid or out-of-range values fail with a field-specific
configuration error. Omitting these keys keeps the defaults in the tables above.

Native HTTP fault duration attributes use integer nanoseconds. TOML retains
duration strings (`ms`, `us`, or `s`) because configuration files are the
human-authored surface; both inputs compile through the same typed
`FaultKindV1` conversion and then the core model.

The service half-close policy is intentionally not configurable in schema v1;
the runtime keeps its existing `Drain` default until the policy has a stable
operator-facing spelling and semantics.

## Datagram proxies

Schema v1 accepts an optional `[[datagram_proxies]]` collection. Its absence
preserves existing configuration behavior; adding it is backward-compatible
within schema v1. Each entry has a fixed UDP target and bounded association,
idle, datagram-size, scheduler-count/byte, and directional fault settings.
The optional `[runtime.datagram]` table sets global proxy, association,
history, per-association ingress-slot, and total ingress-byte limits.

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

Datagram fault `type` values and fields match the separate native datagram
DTO schema: `delay` (`delay_ns`, optional `jitter_ns`), `loss`, `duplicate`
(`additional_copies`), `reorder` (`hold_ns`), `payload-corrupt` (`bytes`), and
`bandwidth` (`bytes_per_second`, `burst_bytes`). All bounds are compiled
before listener activation, and unknown fields are rejected in the native
fault DTOs. Existing stream proxy/fault TOML fields are unchanged.
