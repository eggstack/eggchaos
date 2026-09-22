# Native control plane

The native API is versioned under `/v1` and defaults to loopback. A
non-loopback bind requires both an explicit public-admin opt-in and a bearer
token; failed authentication returns a bounded JSON error without echoing the
token. Request bodies are capped at 1 MiB and route state is changed through
`ControlState` generation publication.

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

## CLI command inventory

`eggchaos --admin <url> [--json] <command>`: `serve` (start from
schema-v1 TOML), `version`, `reset`; `proxy list|get|add|set|remove|
enable|disable`; `fault list|get|add|set|remove`;
`connection list|get|kill`. Every command emits one machine-readable JSON
document with `--json` and exits nonzero on failure.

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

## Metrics

`GET /metrics` exposes low-cardinality Prometheus text reconciled against
closed-connection evidence: accepted/completed/rejected totals, final
outcomes by coarse class, injected graceful/hard-reset request counts,
abortive-close results, byte flows, policy transition counts, per-proxy
connection and byte totals, fault activations by proxy, direction, and
fault type, live per-proxy connection gauges, policy generation gauges,
and queued-byte gauges. Labels never carry connection IDs, peer addresses,
scenario run IDs, arbitrary fault IDs, or hostnames. Per-proxy and
activation series are bounded with overflow buckets.

