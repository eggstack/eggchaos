# M006 — Toxiproxy v2.12.0 Compatibility

Status: blocked  
Depends on: M005  
Parallel with: M007  
Successor gate: M008

## Objective

Add an optional compatibility adapter that lets existing Toxiproxy v2.12.0 clients drive eggchaos for the declared supported surface, while keeping native eggchaos types/semantics authoritative.

Compatibility must be demonstrated against an actual v2.12.0 oracle, not inferred from README/source alone.

## User-visible outcome

A test suite that currently provisions Toxiproxy proxies/toxics over its HTTP API can point at eggchaos compatibility mode and exercise the v2.12.0 proxy/toxic model for the supported behavior.

The project publishes a precise compatibility matrix rather than an unqualified “drop-in replacement” claim.

## Preconditions

M005 is closed:

- native live mutation works;
- generation/evidence semantics are stable;
- native admin/runtime APIs are sufficient;
- compatibility code can translate rather than become a second state authority.

## Compatibility target

Primary target: Shopify Toxiproxy v2.12.0, published 2025-03-18.

The compatibility baseline is `plans/reference/toxiproxy-parity.md`.

The v2.12.0 toxic set is exactly:

- `latency`;
- `bandwidth`;
- `slow_close`;
- `timeout`;
- `reset_peer`;
- `slicer`;
- `limit_data`.

Current Toxiproxy `main` additions such as `packet_loss` are explicitly out of scope for M006.

## Crate/boundary

Implement in `eggchaos-toxiproxy`.

The adapter owns:

- compatibility route definitions;
- Toxiproxy JSON DTOs/defaults;
- compatibility error/status translation;
- toxic <-> native fault translation;
- version response;
- compatibility metric aliases if supported;
- test/oracle normalization.

It must not own:

- listener execution;
- fault engine state;
- native registry truth;
- connection relay.

The adapter calls the same native server command/mutation API as the native admin plane.

## Server exposure mode

Decide one explicit integration approach:

1. mount compatibility routes on the same EggServe admin listener; or
2. expose a dedicated compatibility listener.

Prefer same listener if route separation is clean and no security regression results.

Toxiproxy traditionally listens on port 8474, but eggchaos should not assume or force that port. Allow configuration.

Compatibility admin remains loopback by default.

Do not weaken native public-admin authentication rules merely because Toxiproxy clients expect an unauthenticated local API. If compatibility is exposed non-loopback, document and enforce the eggchaos security model.

## Proxy JSON behavior

Match v2.12.0 externally observed behavior for:

- `name`;
- `listen`;
- `upstream`;
- `enabled`;
- returned toxics;
- port 0 resolving to actual listener port;
- update/restart behavior;
- delete behavior.

Exact absent/default field behavior and status codes must be captured from the oracle.

Do not guess undocumented JSON null/omission rules.

## Toxic JSON behavior

Support:

- optional/default name;
- type;
- stream default downstream;
- toxicity default 1.0;
- attributes;
- update/remove/get/list.

Translation examples:

### latency

Toxiproxy:

```json
{"type":"latency","attributes":{"latency":1000,"jitter":100}}
```

Translate milliseconds into the native `Latency` config and compatibility jitter semantics.

### bandwidth

Toxiproxy `rate` is KB/s. Determine the exact unit convention from oracle/source tests and freeze translation. Do not casually interchange SI kB and KiB if observable.

Native burst policy must be selected so differential behavior is acceptably close; document if exact implementation differs while sustained throughput matches.

### timeout

Toxiproxy timeout=0 means passage stops indefinitely until toxic removal/change.

Map to native blackhole semantics and preserve intentional discarded byte accounting internally.

### reset_peer

Map to native reset request.

Compatibility claim is platform-qualified. If the runtime cannot apply a true reset on a transport/platform, return the supported API behavior but document the resulting `intent compatible` level; do not falsify evidence.

### slicer

Translate average size, variation, and microsecond delay. Random byte segmentation need not reproduce Toxiproxy's exact RNG sequence; it must reproduce documented distribution/range and external semantics.

### limit_data

Exact forwarded-byte boundary is expected.

## Required routes

Implement and oracle-test:

```text
GET    /proxies
POST   /proxies
POST   /populate
GET    /proxies/{proxy}
POST   /proxies/{proxy}
DELETE /proxies/{proxy}

GET    /proxies/{proxy}/toxics
POST   /proxies/{proxy}/toxics
GET    /proxies/{proxy}/toxics/{toxic}
POST   /proxies/{proxy}/toxics/{toxic}
DELETE /proxies/{proxy}/toxics/{toxic}

POST   /reset
GET    /version
GET    /metrics
```

If the actual v2.12.0 server exposes additional relevant aliases/behaviors required by clients, add them only after oracle evidence and update the parity reference.

## /populate semantics

Capture exact behavior using v2.12.0.

At minimum investigate:

- unchanged proxy definition idempotence;
- listen difference;
- upstream difference;
- enabled difference;
- existing toxics when a proxy is replaced or unchanged;
- partial invalid list behavior;
- returned status/body.

Implement the observed semantics through the native mutation authority.

## Differential oracle harness

Pin the oracle to v2.12.0 by exact container digest, release binary checksum, or source/tag build recorded in qualification metadata.

Do not use `latest`.

Create a machine-readable case corpus, e.g.:

```text
qualification/toxiproxy-v2.12/
  cases/*.json
  expected/
  runner/
  README.md
```

For each case run the same client actions against:

- official Toxiproxy v2.12.0;
- eggchaos compatibility mode.

Normalize non-deterministic values:

- actual ephemeral ports;
- timestamps;
- timing tolerances;
- platform-specific error text.

Do not normalize away semantically meaningful status codes, JSON fields, byte counts, enabled state, direction, or toxic defaults.

## Differential case families

Required:

### API/defaults

- create minimal proxy;
- create with enabled=false;
- duplicate name;
- get/list/delete not-found;
- port 0;
- update listen;
- update upstream;
- enable/disable;
- malformed JSON;
- missing fields;
- invalid stream/toxicity/type/attributes.

### Populate

- create collection;
- identical repeat;
- one changed target;
- add/remove definitions as actual semantics dictate.

### Toxics

For every v2.12 toxic:

- default name/stream/toxicity;
- explicit upstream/downstream;
- add/get/list/update/remove;
- edge attribute values;
- toxicity 0;
- toxicity 1;
- active connection update/removal.

### Behavior

- echo byte preservation for preserving toxics;
- direction isolation;
- latency timing;
- bandwidth sustained rate;
- timeout/blackhole;
- reset outcome;
- slice behavior;
- exact limit-data boundary;
- slow close;
- half-close interaction;
- reset endpoint.

### Metrics/version

- version payload;
- metric endpoint content type/format;
- documented Toxiproxy metric names/labels where compatibility claim is made.

## Client-library smoke tests

In addition to raw HTTP corpus, run at least two representative existing client surfaces if practical:

1. the Go client from the pinned Toxiproxy v2.12.0 repository/tag or a known compatible release;
2. one independent client such as `toxiproxy-rust`, Python, or Java chosen for CI cost and current compatibility.

The smoke should:

- populate/create;
- add downstream latency;
- update/remove;
- disable/enable;
- reset.

Record exact client versions.

Do not make a language ecosystem a required production dependency.

## Compatibility levels

Publish per row:

- API shape compatible;
- behaviorally compatible;
- intent compatible;
- not supported.

A timing tolerance must be numeric in the qualification harness. Do not label a test “close enough” manually.

## Security

Compatibility JSON must obey the same body/count/name limits as native API after translation.

Do not echo secrets or arbitrary request payloads in errors/logs.

Loopback defaults remain.

## Tests/commands

M006 should provide one repeatable qualification command, for example:

```sh
./scripts/qualify_toxiproxy_v2_12.sh
```

It should:

- verify the pinned oracle identity;
- build eggchaos;
- run corpus;
- emit machine-readable summary;
- fail nonzero on unexpected divergence.

Routine Rust unit/integration tests still run independently.

## Acceptance criteria

M006 closes only when:

- all required v2.12 routes are implemented through the native state authority;
- seven v2.12 toxics map to native faults;
- defaults/status/error behavior has oracle evidence;
- differential corpus passes within declared comparators/tolerances;
- active update/removal behavior is tested;
- at least two client surfaces are smoke-tested or a closure record explicitly blocks on an unavailable second client (in which case do not claim broad client compatibility);
- current-main `packet_loss` remains out of the v2.12 claim;
- compatibility matrix is published and truthful about reset/platform differences;
- closure evidence is committed.

## Stop/rejection conditions

Stop/revise if:

- compatibility requires a second independent proxy/fault registry;
- exact API behavior cannot be determined because the pinned oracle is unavailable;
- adapter needs to corrupt native semantics to mimic an undocumented quirk that can instead be isolated in translation;
- reset parity is claimed without observing the transport result;
- comparison harness normalizes away meaningful divergences;
- `latest` is used as oracle.

## Closure evidence

Create `plans/closure/M006-toxiproxy-v2-12-compatibility-closure.md` with:

- candidate SHA;
- oracle identity/checksum/tag;
- corpus summary artifact;
- client versions/smoke results;
- divergence/intent-compatibility table;
- platform results;
- acceptance verdict.

Then update registry:

- M006 -> `closed`;
- M008 remains blocked until M007 is also closed.
