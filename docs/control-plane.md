# Native control plane

The native API is versioned under `/v1` and defaults to loopback. A
non-loopback bind requires both an explicit public-admin opt-in and a bearer
token; failed authentication returns a bounded JSON error without echoing the
token. Request bodies are capped at 1 MiB and route state is changed through
`ControlState` generation publication.

The CLI accepts `--admin-token <token>` or `EGGCHAOS_ADMIN_TOKEN`; the command
line option takes precedence. The header is omitted when neither is set.
Tokens are redacted from native configuration/admin `Debug` output.

The EggServe leaf H1 runtime owns parsing, request-body bounds, and connection
lifecycle. Eggchaos owns only route dispatch and typed JSON conversion. The
CLI uses Eggfetch for control requests, including JSON mode and nonzero error
exit status.

## Route inventory

Proxies: `GET /v1/proxies`, `POST /v1/proxies`,
`GET /v1/proxies/{name}`, `PATCH /v1/proxies/{name}`,
`DELETE /v1/proxies/{name}`. Faults:
`GET /v1/proxies/{name}/faults`, `POST /v1/proxies/{name}/faults`,
`GET /v1/proxies/{name}/faults/{id}`,
`PATCH /v1/proxies/{name}/faults/{id}`,
`DELETE /v1/proxies/{name}/faults/{id}`. Connections:
`GET /v1/connections`, `GET /v1/connections/{id}`,
`DELETE /v1/connections/{id}` (terminate), `GET /v1/history`.
Service: `GET /v1/health`, `GET /v1/version`, `POST /v1/reset`,
`GET /metrics` (Prometheus text, no `/v1` prefix).

## Native v1 request and response schemas

Native mutation bodies use dedicated `/v1` DTOs. Fault duration values in
JSON are unsigned integer nanoseconds. The `kind` object is identical for
create, patch, and fault views:

```json
{"direction":"downstream","id":"latency","probability":1.0,"kind":{"type":"latency","delay_ns":1000000,"jitter_ns":0,"max_buffer_bytes":65536}}
```

Stable fault `type` values and attributes are: `latency` (`delay_ns` required,
`jitter_ns` defaults to 0, `max_buffer_bytes` to 65536), `bandwidth`
(`bytes_per_second` defaults to 1, `burst_bytes` to 65536), `blackhole`
(`close_after_ns` defaults to null), `limit-data` (`bytes` defaults to 1),
`slow-close` (`delay_ns` required), `slice` (`average_size` defaults to 1024,
`variation` and `delay_ns` to 0), and `disconnect` (`after_ns` defaults to 0,
`hard_reset` to false). Required sizes/rates must be non-zero; probability
defaults to 1 and must be finite in `0..=1`. Unknown properties are rejected.
Direction values are `upstream` and `downstream`.

Proxy create and patch use `connect_timeout_ms`; create also accepts `seed`.
For example:

```json
{"name":"redis","listen":"127.0.0.1:0","upstream":"127.0.0.1:6379","enabled":true,"connect_timeout_ms":5000,"seed":0}
```

Proxy response views contain explicit native fault views instead of core
`FaultKind` Serde output. Scenario documents use version 1, millisecond
`at_ms` scheduling, and nested `action` objects tagged by `type` (`set-plan`
or `remove-fault`); faults in `set-plan` use the same kind schema. Scenario run
status values are lowercase. These DTOs define the native v1 contract
independently of internal Rust enum layout. The DTO authority is the
`eggchaos-protocol` crate (re-exported by `eggchaos-server` for source
compatibility); the machine-readable contract is
`api/openapi/eggchaos-v1.yaml`, drift-checked by
`./scripts/check_openapi.sh` and the `NATIVE_OPERATIONS` inventory.

## CLI command inventory

`eggchaos --admin <url> [--json] <command>`: `serve` (start from
schema-v1 TOML), `version`, `reset`; `proxy list|get|add|set|remove|
enable|disable`; `fault list|get|add|set|remove`;
`connection list|get|kill`; `scenario apply|validate|compile|get|cancel`;
`history`; `metrics`.
JSON routes emit one machine-readable document with `--json` and exit nonzero
on failure. Metrics are Prometheus text in human mode and are wrapped as
`{"body":"..."}` with `--json`.

Proxy create requests accept `connect_timeout_ms` and `seed`; add faults via
the fault routes rather than embedding them in proxy create. Proxy views
expose fault arrays whose entries use the same kind schema. TOML keeps human
duration strings and accepts legacy type aliases at the config parser edge.

Fault IDs are opaque UTF-8 path components up to the core identity limit. The
CLI percent-encodes them; the native router splits the raw path first and
decodes a fault ID exactly once. Malformed escapes and decoded invalid UTF-8
return a bounded `400 invalid` response. Existing unreserved IDs retain their
usual spelling. Invalid schema-v1 configuration, including zero-valued
required capacities, returns a field error instead of panicking.

## Generations and policy snapshots

Fault updates are generation transitions. Existing streams drain bytes already
owned by the old generation before observing a newly published `LivePolicy`.
The transition does not capture payloads.

Each directional policy publishes one atomic snapshot per generation carrying
the plan, the policy generation, and the seed namespace that connection
engines compile their fault-local RNGs from. `GET` proxy views report each
plan together with the generation and seed namespace from the same snapshot,
so reads can never pair a plan with a generation it was not published with.
Manual control updates retain the current seed namespace. A stale-base
publication (compare-and-swap on the expected generation) fails with a
`conflict` error instead of silently overwriting a concurrent update.

Connection snapshots report the accepted policy generations and seed
namespaces alongside the currently observed generations, a pending transition
target while an old generation drains, per-direction byte counters, active
fault identities, and transition counts. No payload bytes are recorded
anywhere in evidence, metrics, or history.

## Scenarios

`POST /v1/scenarios/apply` starts an owned run and returns
`{run_id, seed, status}`; `GET /v1/scenarios/{run_id}` reports status,
applied count, failure detail, and a per-event generation trail;
`DELETE /v1/scenarios/{run_id}` cancels a running run. Runs are supervised
by the service and cancelled on shutdown; at most 32 run records are
retained. Events apply fail-fast against the live plans at fire time.

A scenario seed participates in deterministic engine decisions through the
published seed namespace, derived from `(scenario seed, run id, event
index)` only. Replay limits: the same document reproduces the same
namespaces and therefore the same fault decisions for the same connection
keys, but connection keys depend on accept order, so replay is exact for
policy state and per-key decisions, not for live connection timing.
Manual mutations between scenario events change the base a later event
applies to; a conflicting concurrent publication fails the run instead of
rolling state back.

Scenario v1 (`POST /v1/scenarios/apply` with `version: 1`) keeps this
contract and remains the compatibility surface. Scenario v2 wires the
M026 bounded source-language, compiler, and replay identity into the
owned run supervisor:

- `POST /v1/scenarios/apply` is version-aware: a `version: 2`
  schedule compiles server-side and starts an owned v2 run through
  the same supervisor JoinSet; `version: 1` keeps the existing path.
- `POST /v1/scenarios/validate` returns fingerprint, compiler
  version, event count, and schedule identity without creating a run.
- `POST /v1/scenarios/compile` returns the normalized compiled event
  tape plus fingerprint without creating a run.
- `GET`/`DELETE /v1/scenarios/{run_id}` inspect/cancel both versions;
  run IDs share one namespace.
- The CLI accepts v2 JSON or TOML schedule files (`scenario
  validate|compile|apply <file>`, TOML selected by extension) and
  forwards the semantic document to the server authority; the CLI
  never expands phases or derives fingerprints.

A v2 run captures one monotonic epoch at task entry and waits for
`epoch + offset` per event with `sleep_until`, so slow event
application never shifts later deadlines. Strict isolation (the
default) publishes each event against the generation last owned by
the run and fails fast on external conflict; live isolation rebuilds
from the live plan at fire time with an expected-generation guard. The
default `restore-initial` cleanup republishes each touched
resource's initial plan only while the run still owns its
generation — an externally moved resource records a cleanup conflict
and keeps its external state. Run evidence carries the schedule
fingerprint, execution key, compiler version, isolation/cleanup
policy, per-event scheduled/applied timing with lateness, resulting
generations, and the cleanup outcome; no payload bytes.

Deterministic policy/event identity is exact: the same schedule
replays identical seed namespaces regardless of daemon run order.
Live connection/datagram arrival timing is not replayed — connection
keys depend on accept order, and scheduler lateness is diagnostic
evidence, never an RNG input.

## Metrics

`GET /metrics` exposes low-cardinality Prometheus text reconciled against
closed-connection evidence: accepted/completed/rejected totals, final
outcomes by coarse class, injected graceful/hard-reset request counts,
abortive-close results, byte flows, policy transition counts, per-proxy
connection and byte totals, fault activations by proxy, direction, and
fault type, live per-proxy connection gauges, policy generation gauges,
and queued-byte gauges, plus coarse v2 schedule counters
(`eggchaos_schedule_v2_runs_total`, `eggchaos_schedule_v2_events_total`,
`eggchaos_schedule_v2_late_events_total`). Labels never carry connection
IDs, peer addresses, scenario run IDs, arbitrary fault IDs, hostnames,
schedule fingerprints, execution keys, or phase names. Per-proxy and
activation series are bounded with overflow buckets.

## Datagram resources

Datagrams use a separate `/v1/datagram-proxies` family: list/create/get/patch/delete proxies, manage directional faults at `/v1/datagram-proxies/{name}/faults[/{id}]`, and list/get/kill associations at `/v1/datagram-associations[/{id}]`. A datagram proxy create body uses `listen`, fixed `upstream`, `max_associations`, `association_idle_timeout_ms`, `max_queued_datagrams`, `max_queued_bytes`, `max_datagram_size`, `seed`, and optional `upstream_faults`/`downstream_faults`. Proxy patches accept listener/upstream changes, association bounds/idle timeout, and `enabled`; a running listener address change binds the replacement first, while a bind failure leaves the current listener serving.

Datagram fault DTOs are separate from stream `FaultKindV1`; `type` is one of `delay`, `loss`, `duplicate`, `reorder`, `payload-corrupt`, or `bandwidth`. Durations use unsigned integer nanoseconds. Association responses include addresses, counters, directional engine evidence, generations, and queue bounds; payload bytes are never captured. Association/client/run/fault IDs never become Prometheus labels. `POST /v1/reset` resets both TCP and UDP definitions: it clears plans, terminates active work, and re-enables listeners; the report separates failed TCP and datagram re-enables. Toxiproxy v2.12 remains TCP/stream-only.

The `datagram` CLI namespace includes `proxy list|get|add|enable|disable|remove`, `fault list|get|add|set|remove`, and `association list|get|kill`. JSON mode remains one document per operation. Scenario v1 adds `set-datagram-plan` and `remove-datagram-fault`; the action carries direction-explicit datagram faults and publishes the scenario-derived seed namespace with an expected-generation guard.

## Remote control SDKs

Python (`bindings/python-client`, `from eggchaos_client import Client,
AsyncClient`) and TypeScript (`bindings/typescript-client`,
`@eggstack/eggchaos-client`, `EggchaosClient`) remote clients drive the
complete native surface above — stream and datagram resources, Scenario
V1/V2, evidence/history/reset/metrics, bearer auth, and typed native
errors — without embedding Rust. Both derive from the exact M032
OpenAPI/protocol authority (`scripts/sync_sdk_contract.py`; drift
fails CI on regeneration diffs). They never manage the daemon
lifecycle: start `eggchaos serve` (or embed it) and pass the admin URL.

This is distinct from Toxiproxy compatibility (`docs/toxiproxy.md`),
which serves the legacy TCP subset for existing Toxiproxy clients, and
from in-process embedding: `eggchaos-native` (`bindings/python-native`)
runs the same service in the Python test process via the safe
`eggchaos-embed` facade — fixture-local startup/control with no admin
HTTP hop, but the same validation, defaults, bounds, and error
semantics. Remote clients remain the portability-first option; the
native module is for harnesses that must own lifecycle in-process.
