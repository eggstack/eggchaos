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
current namespace, while scenario runs publish namespaces derived from
`(scenario seed, run id, event index)`; see `docs/control-plane.md` for the
replay limits this implies.

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
