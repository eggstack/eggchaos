# M037 — Stream-Loss Native Contract and Cross-Language Propagation

Status: blocked  
Depends on: M036  
Role: native control/config/scenario/SDK/embed propagation

## Objective

Expose the M036 deterministic `StreamLoss` primitive consistently through the
existing native v1 control contract, schema-v1 configuration, CLI, Scenario
V1/V2 action payloads, OpenAPI, Python/TypeScript remote SDKs, safe embed
facade, and Python-native pilot without creating a second semantic authority.

M037 is contract propagation. The fault semantics remain owned by
`eggchaos-core`.

## Entry criteria

Do not begin implementation until M036 is closed and its closure freezes:

- exact `StreamLossConfig` public fields;
- `STREAM_LOSS_GRAIN_BYTES`;
- evidence field names and meanings;
- multi-loss composition behavior;
- golden decision vectors.

If M036 changes ADR 007, reconcile this plan before implementation.

## Scope

### In scope

- Add `stream-loss` to `eggchaos-protocol::FaultKindV1`.
- Preserve finite [0,1] validation for `loss_rate` and `correlation`.
- Propagate additive stream-loss evidence through native connection/history
  DTOs where those core counters are already exposed.
- Preserve legacy seven-entry activation arrays unchanged.
- Add schema-v1 TOML representation/defaults.
- Add native HTTP fault CRUD support through the existing shared mutation
  authority.
- Add CLI create/update/show support and JSON-first output.
- Add Scenario V1 and Scenario V2 stream-fault payload support by reusing the
  same `FaultKindV1`/core conversion authority.
- Update `api/openapi/eggchaos-v1.yaml` and keep drift checking mechanical.
- Regenerate/update Python sync/async and TypeScript control clients from the
  authoritative contract.
- Update `eggchaos-embed` and `eggchaos-native` coarse fault values only
  through the existing safe Rust facade/conversion path.
- Add cross-surface conformance fixtures proving HTTP, SDK, embed, and native
  Python describe the same fault.
- Update user/architecture documentation.
- Requalify existing datagram/native contract behavior for regressions.

### Non-goals

- No Toxiproxy `packet_loss` type; that is M038.
- No change to strict v2.12 compatibility behavior.
- No new HTTP routes or API version.
- No configurable stream-loss grain.
- No new scenario language construct; this is only a new stream fault variant.
- No SDK-owned loss simulation.
- No Python/TypeScript/PyO3 data-plane code.
- No generic C ABI or new language binding.
- No change to datagram loss schemas/semantics.
- No live-generation state migration.

## Affected surfaces

Expected surfaces include:

- `crates/eggchaos-protocol/src/stream.rs`;
- `crates/eggchaos-protocol/src/routes.rs` only if operation metadata fixtures
  require regeneration, not because routes change;
- `crates/eggchaos-server/src/native.rs` compatibility re-exports/adapters;
- `crates/eggchaos-server/src/config.rs`;
- shared stream fault mutation/control conversion authority;
- `crates/eggchaos-cli`;
- Scenario V1/V2 DTO/fixture paths;
- `api/openapi/eggchaos-v1.yaml`;
- `bindings/python-client`;
- `bindings/typescript-client`;
- `crates/eggchaos-embed`;
- `bindings/python-native`;
- contract/golden/conformance fixtures;
- `docs/configuration.md`;
- `docs/control-plane.md`;
- `docs/architecture.md`;
- `architecture/control-plane-cli.md`;
- `architecture/scenario-observability.md`;
- `architecture/verification-qualification.md`;
- binding READMEs as needed.

## Native wire shape

Preferred additive v1 representation:

```json
{
  "type": "stream-loss",
  "loss_rate": 0.2,
  "correlation": 0.4
}
```

The exact casing follows the existing `kebab-case` fault discriminator rule.

Both fields are required or explicitly defaulted by one documented rule. Prefer
explicit defaults of `0.0` only if that matches existing fault-DTO default
style and does not create ambiguous no-op configurations; otherwise require
both in native requests and let authoring helpers provide defaults.

The fixed 32 KiB grain is semantics metadata, not a mutable request field.

The OpenAPI description must explicitly state that this is deterministic
userspace stream-chunk loss, not IP packet loss.

## Native configuration

Add an authoring form consistent with existing fault configuration. Example
shape:

```toml
[[proxy.fault]]
id = "loss"
direction = "downstream"
type = "stream-loss"
loss_rate = 0.20
correlation = 0.40
```

Validation must flow through the protocol/core authority. Config parsing must
not implement independent clamping.

## CLI

Add fault-kind arguments without creating an alternate semantic parser.
Preferred form:

```text
eggchaos fault add <proxy> <id> \
  --direction downstream \
  --kind stream-loss \
  --loss-rate 0.20 \
  --correlation 0.40
```

Patch behavior must be explicit. If the existing CLI replaces whole fault-kind
payloads, keep that rule; do not add stream-loss-only partial semantics in the
CLI.

`--json` must return the canonical native DTO.

## Scenario V1/V2

Scenario actions already carry typed stream fault definitions. Add the new
variant through the same contract conversion.

Do not add:

- continuous interpolation of loss probability;
- per-event RNG overrides;
- packet predicates;
- packet-count scheduling.

A scheduled publication changes the whole plan generation exactly as it does
for existing stream faults. Per ADR 007, a new generation compiles fresh
stream-loss internal state.

Scenario V2 fingerprint/golden semantics must update only because the semantic
fault payload gains a new valid variant. Existing fixtures must remain
byte-for-byte stable.

## Evidence compatibility

Do not replace or extend the legacy seven-slot arrays.

If native evidence already serializes core stream evidence, add named fields
with defaults/optional handling chosen to preserve decoding of older recorded
documents where practical.

Required cross-language properties:

- Python/TypeScript generated models expose named stream-loss counters;
- old seven-entry arrays remain length seven;
- no SDK computes counters client-side;
- no payload bytes are added to evidence.

## OpenAPI and SDK rules

The existing M032 single-authority/drift rule remains in force.

M037 must:

1. update protocol DTOs first;
2. update/regenerate the checked-in OpenAPI artifact;
3. update both SDKs from that authority;
4. fail drift checks if one SDK omits the new union variant;
5. add live server tests creating/patching/reading stream loss from each SDK.

No server behavior may be changed merely to satisfy a generator limitation.

## Embed/native Python

The safe `eggchaos-embed` facade should expose stream loss using the same
protocol/core semantic values as HTTP. PyO3 conversion remains outside the
workspace and must not receive direct access to Tokio stream internals.

Required conformance:

- construct/add stream loss through embed;
- read it through HTTP and compare;
- construct/add through HTTP and read through embed;
- perform equivalent Python-native operation and compare canonical values;
- invalid probabilities map to the existing bounded error categories.

## Ordered work packages

### WP1 — Protocol DTO + conversion

Add the fault variant and additive evidence fields to the protocol authority.
Freeze JSON fixtures and validation boundaries.

### WP2 — Server/config/control propagation

Wire config and HTTP mutation/view paths through existing shared conversion and
ControlState authorities. No adapter-local state.

### WP3 — CLI propagation

Add parsing/help/human/JSON support and focused command tests.

### WP4 — Scenario propagation

Add the variant to Scenario V1/V2 DTO fixtures and prove existing fingerprints
remain stable.

### WP5 — OpenAPI drift update

Regenerate/update the checked-in contract and operation/schema drift fixtures.

### WP6 — Python/TypeScript SDK update

Regenerate/update typed unions, examples, package tests, and live conformance.

### WP7 — Embed/Python-native update

Expose the same fault through safe embed and the PyO3 pilot, then extend
HTTP/embed/native-Python conformance.

### WP8 — Documentation and exact-candidate closure

Update control/config/architecture docs and record M037 closure evidence.

## Required tests

At minimum:

- protocol JSON round-trip for `stream-loss`;
- finite-bound validation: -epsilon, 0, typical values, 1, >1, NaN/infinite
  where the parser can represent them;
- unknown-field rejection;
- config TOML round-trip;
- HTTP add/list/get/patch/remove;
- generation conflict behavior unchanged;
- CLI human and `--json` add/show/remove;
- Scenario V1 application with stream loss;
- Scenario V2 compile/apply with stream loss;
- existing Scenario V2 golden fixtures unchanged;
- new stream-loss Scenario V2 fingerprint fixture;
- OpenAPI parse/drift gate;
- Python sync + async model/live tests;
- TypeScript model/live tests;
- HTTP ↔ embed stream-loss conformance;
- Python-native lifecycle/conformance;
- evidence seven-slot arrays remain exactly length seven;
- datagram fault fixtures unchanged;
- existing native stream fault fixtures unchanged.

## Verification

Minimum exact-candidate commands:

```sh
./scripts/check.sh
./scripts/check_openapi.sh
./scripts/check_python_client.sh
./scripts/check_typescript_client.sh
./scripts/qualify_language_clients.sh
./scripts/check_python_native.sh
./scripts/qualify_python_native.sh
cargo test -p eggchaos-protocol --all-features
cargo test -p eggchaos-server --all-features
cargo test -p eggchaos-cli --all-features
cargo test -p eggchaos-embed --all-features
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
```

Also rerun the strict v2.12 regression to prove M037 has not silently exposed
`packet_loss` through that profile:

```sh
TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" \
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 \
  ./scripts/qualify_toxiproxy_v2_12.sh
```

M039 remains the full tranche qualification gate; M037 closure still requires
its own declared exact-candidate tests.

## Acceptance criteria

M037 closes only when:

- every native stream fault authoring/control surface supports `stream-loss`;
- all surfaces consume the M036 core semantic authority;
- OpenAPI describes the new fault and drift checks pass;
- Python/TypeScript SDKs expose and live-test it;
- embed/Python-native expose it without data-plane FFI;
- Scenario V1/V2 can publish it without changing schedule semantics;
- legacy evidence arrays remain unchanged;
- existing Scenario/datagram/native fixtures remain stable;
- strict Toxiproxy v2.12 still rejects `packet_loss`;
- all required gates pass on one exact candidate;
- closure evidence records contract changes and package/platform qualification.

Create
`plans/closure/M037-stream-loss-native-contract-and-cross-language-propagation-closure.md`.

## Stop/rejection conditions

Do not close if:

- one surface implements its own loss RNG/chunking/clamping;
- the fixed 32 KiB grain leaks as a mutable field on only some surfaces;
- OpenAPI and protocol disagree;
- one SDK hand-copies a divergent fault model;
- Scenario V2 existing fingerprints change unexpectedly;
- strict v2.12 accepts the post-v2.12 toxic;
- datagram loss is renamed/reused for stream loss;
- evidence arrays are resized.

## Follow-on activation

A clean M037 closure makes M038 ready.

M038 may add the Toxiproxy `packet_loss` spelling only by translating into
the M037/M036 native authority.
