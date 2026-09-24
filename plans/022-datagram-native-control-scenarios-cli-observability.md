# M022 — Datagram Native Control, Scenarios, CLI, and Observability

Status: closed
Depends on: M021
Role: native operator/control surface for datagram runtime

## Objective

Expose the proven M020/M021 datagram engine and fixed-target UDP runtime through explicit native resources: versioned API DTOs, TOML configuration, CLI commands, scenario actions, association inspection/control, and Prometheus/evidence surfaces.

The UDP/datagram resource family must remain distinct from the existing TCP `/v1/proxies` contract and must not change Toxiproxy v2.12 compatibility.

## Baseline

M017 made the native TCP wire contract explicit instead of serializing internal Rust state directly. M022 follows the same rule for datagrams.

ADR 003 rejects adding `transport = "udp"` conditionals to the existing TCP proxy model. Stream and datagram plans have different fault semantics, probability semantics, lifecycle resources, and evidence. The operator surface should expose that distinction instead of hiding it behind optional fields.

M021 must be closed before this plan becomes ready so the public contract is based on proven runtime behavior rather than speculative socket semantics.

## Scope

### In scope

- Explicit v1 datagram proxy/fault/association DTOs.
- Native CRUD over the authoritative M021 runtime.
- A separate schema-v1 TOML datagram proxy collection with validated limits.
- JSON-first CLI commands under a datagram namespace.
- Datagram-specific scenario actions and replay evidence.
- Association list/get/kill operations.
- Prometheus metrics and bounded retained history/evidence.
- Secure-admin behavior identical to existing native control policy.
- Reset semantics defined explicitly for stream and datagram resources.

### Non-goals

- No Toxiproxy UDP API.
- No overloading existing stream `FaultKindV1` with datagram variants.
- No protocol-aware DNS/QUIC parsing.
- No arbitrary destination/routing/SOCKS configuration.
- No new datagram fault algorithms beyond M020.
- No lower-layer packet claims.
- No general scenario-language redesign beyond the minimum explicit datagram actions.

## Affected surfaces

- `crates/eggchaos-server/src/native.rs`.
- `crates/eggchaos-server/src/admin.rs`.
- `crates/eggchaos-server/src/config.rs`.
- `crates/eggchaos-server/src/runtime/`.
- `crates/eggchaos-server/src/scenario.rs`.
- `crates/eggchaos-cli/src/`.
- `docs/configuration.md`, `docs/control-plane.md`, `docs/architecture.md`.
- `architecture/control-plane-cli.md` and `architecture/scenario-observability.md`.
- Verification/fuzz fixtures for native JSON/TOML transitions.

## Native resource model

Use a sibling route family:

```text
GET    /v1/datagram-proxies
POST   /v1/datagram-proxies
GET    /v1/datagram-proxies/{name}
PATCH  /v1/datagram-proxies/{name}
DELETE /v1/datagram-proxies/{name}

GET    /v1/datagram-proxies/{name}/faults
POST   /v1/datagram-proxies/{name}/faults
GET    /v1/datagram-proxies/{name}/faults/{id}
PATCH  /v1/datagram-proxies/{name}/faults/{id}
DELETE /v1/datagram-proxies/{name}/faults/{id}

GET    /v1/datagram-associations
GET    /v1/datagram-associations/{id}
DELETE /v1/datagram-associations/{id}
```

Exact response envelopes should follow established native conventions, including bounded errors, status codes, path-identifier validation, auth, and JSON content type.

At minimum add explicit DTO concepts equivalent to:

```text
DatagramProxyRequestV1
DatagramProxyPatchV1
DatagramProxyViewV1
DatagramFaultKindV1
DatagramFaultUpsertV1
DatagramFaultPatchV1
DatagramFaultViewV1
DatagramAssociationViewV1
DatagramRuntimeConfigV1
```

Internal datagram Rust enums must not become the wire schema accidentally through blanket Serde exposure.

## Config model

Keep the native config schema version explicit. Prefer a sibling collection such as:

```toml
version = 1

[[datagram_proxies]]
name = "dns"
listen = "127.0.0.1:0"
upstream = "127.0.0.1:5353"
max_associations = 256
association_idle_timeout_ms = 60000
max_datagram_size = 65535
max_queued_datagrams = 1024
max_queued_bytes = 4194304
```

The implementation must decide and document whether adding this collection is backward-compatible within schema version 1 or requires a schema version increment. Do not silently reinterpret existing fields.

Invalid numeric bounds, zero capacities, invalid listen/upstream addresses, and unsupported combinations fail before listener activation. File config and HTTP DTO conversion must converge on the same validation/compile authority.

## CLI

Add a coherent namespace rather than intermixing UDP flags with TCP commands. Illustrative surface:

```text
eggchaos datagram proxy list --json
eggchaos datagram proxy add dns --listen 127.0.0.1:0 --upstream 127.0.0.1:5353
eggchaos datagram fault add dns loss --direction upstream --probability 0.05
eggchaos datagram fault add dns delay --direction downstream --delay 100ms --jitter 25ms
eggchaos datagram fault remove dns <id>
eggchaos datagram association list --json
eggchaos datagram association kill <id>
```

The CLI remains a thin Eggfetch-backed HTTP adapter. It must not directly manipulate runtime state or open UDP sockets.

Every `--json` operation emits exactly one stable JSON document and exits nonzero on failure.

## Scenario model

Do not reuse stream `ScenarioAction::SetPlan { faults: Vec<FaultSpec> }` for datagram plans.

Add explicit actions equivalent to:

```text
SetDatagramPlan {
    proxy,
    direction,
    faults: Vec<DatagramFaultSpec>
}

RemoveDatagramFault {
    proxy,
    direction,
    id
}
```

Scenario seed namespaces must participate in M020's datagram RNG derivation. Scenario evidence must distinguish stream versus datagram actions unambiguously.

Concurrent manual mutation continues to use generation/CAS conflict rules rather than silently overwriting a stale base.

## Association observability

List/get responses must expose safe bounded metadata sufficient to diagnose replay:

- association ID/key;
- proxy identity;
- client and fixed target addresses;
- lifecycle state/last-activity summary;
- accepted/current upstream/downstream policy generations and seed namespaces;
- packet/byte counts;
- configured-loss, queue-overflow, oversize, and administrative-discard counts;
- duplicate/corruption/reorder activations;
- queue count/byte current/high-water values;
- bounded active fault identity lists;
- RNG version.

No payload bytes are recorded.

Association kill is explicit operator termination and must not be reported as configured packet loss.

## Metrics

Prometheus labels must remain low-cardinality. Prefer bounded proxy + direction + fault-type dimensions. Do not label by association ID, peer address, payload, run ID, fault ID, hostname, or arbitrary target text.

At minimum expose counters/gauges for active/total associations, ingress/egress datagrams/bytes, configured loss, overflow, oversize, administrative discard, fault activations, and queued/high-water resources where cardinality remains bounded.

Metric names must clearly distinguish datagram resources from the existing TCP connection/byte metrics.

## Reset semantics

The existing native reset operation must be reconciled explicitly.

Acceptable choices are either:

- reset both stream and datagram proxy definitions atomically with one documented generation transaction; or
- introduce an explicit resource-scoped reset while preserving the existing stream behavior.

The chosen behavior must be deterministic, documented, tested, and reflected in the v1 contract. Do not accidentally leave datagram listeners alive while claiming a global reset.

## Ordered work packages

### WP1 — Freeze DTO and config contracts

Define public v1 datagram DTOs, resource naming, validation/defaults, reset semantics, and config-version decision before wiring routes.

### WP2 — ControlState/runtime authority

Add typed datagram proxy/fault/association operations that mutate the single M021 runtime authority. Preserve bind-before-success, generation publication, rollback, cancellation/join, and bounded history behavior.

### WP3 — Native HTTP routes

Wire the sibling route family through EggServe with existing auth, loopback/public-admin, body-size, path-ID, and bounded error conventions.

### WP4 — TOML configuration

Compile the datagram proxy collection through the same typed validation authority and start it through the runtime, with exact JSON/TOML round-trip/default fixtures.

### WP5 — CLI

Add the datagram command namespace as a thin native HTTP client, including machine-readable list/get/mutation output and auth support.

### WP6 — Scenarios

Add explicit datagram scenario actions, seed namespace derivation, expected-generation conflict behavior, cancellation, bounded run evidence, and replay fields.

### WP7 — Metrics, association history, and inspection

Expose internal M021 observability through bounded snapshots/history and low-cardinality Prometheus metrics.

### WP8 — Documentation and compatibility guard

Update docs/architecture indexes, route/CLI inventory, examples, and explicitly state that Toxiproxy v2.12 remains stream/TCP-only.

## Verification

Minimum:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
./scripts/qualify_eggfetch.sh
TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 ./scripts/qualify_toxiproxy_v2_12.sh
cargo audit --deny warnings
cargo deny check advisories licenses bans sources
```

Required focused evidence:

- every datagram DTO JSON fixture round-trips exactly;
- TOML defaults/bounds are stable and invalid input is panic-free;
- CRUD controls actual listener/runtime state rather than a shadow map;
- restart-class listen/upstream mutation has documented rollback;
- fault add/patch/remove publishes the expected directional generation;
- association list/get/kill reflects live and final state;
- scenario mutation changes subsequent datagram decisions while queued old-generation data retains prior decisions;
- stale scenario/manual generations conflict rather than overwrite;
- CLI JSON contracts and auth work;
- metric labels remain bounded;
- global/resource reset behavior matches docs;
- all stream/Toxiproxy/Eggfetch regressions remain green.

## Acceptance criteria

M022 closes only when:

- M021 is closed;
- datagram proxies/faults/associations are explicit native resources;
- DTOs are decoupled from internal Rust enum layout;
- file config, admin API, CLI, and scenarios converge on one runtime authority;
- scenario actions are transport-explicit;
- auth/security/body/path limits match existing native policy;
- observability distinguishes all destructive/drop classes and captures no payload;
- metrics remain low-cardinality;
- Toxiproxy v2.12 behavior is unchanged;
- full workspace and regression gates pass.

Create `plans/closure/M022-datagram-native-control-scenarios-cli-observability-closure.md`.

## Stop/rejection conditions

Do not close if:

- UDP is represented by optional fields inside the existing TCP DTO without a clear compatibility reason;
- HTTP/config/CLI paths maintain shadow runtime state;
- scenario actions serialize stream `FaultSpec` for datagram behavior;
- metric labels are association/client/fault-ID cardinality;
- reset claims disagree with live listeners;
- Toxiproxy compatibility changes to accommodate UDP;
- public schema behavior is inferred rather than fixture-tested.

## Follow-on activation

On clean closure, M023 becomes `ready`.
