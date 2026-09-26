# M037 — Stream-Loss Native Contract and Cross-Language Propagation Closure

Status: closed  
Depends on: M036 (closed)  
Role: native control/config/scenario/SDK/embed propagation  
Candidate: see git rev below; M039 records the final combined authority.

## Scope recap

Expose the M036 deterministic `StreamLoss` primitive consistently through
the existing native v1 control contract, schema-v1 configuration, CLI,
Scenario V1/V2 action payloads, OpenAPI, Python/TypeScript remote SDKs,
safe embed facade, and Python-native pilot without creating a second
semantic authority. M037 is contract propagation; the fault semantics
remain owned by `eggchaos-core`.

## Implementation summary

### Protocol DTO (`crates/eggchaos-protocol/src/stream.rs`)

- `FaultKindV1::StreamLoss { loss_rate, correlation }` arm added to the
  `tag = "type"`, `rename_all = "kebab-case"`, `deny_unknown_fields`
  enum. Discriminator spelling: `stream-loss` (the post-v2.12 Toxiproxy
  presentation `packet_loss` remains M038 only).
- `into_core` constructs `FaultKind::StreamLoss(StreamLossConfig { .. })`
  via shared `finite_probability` helper that enforces `[0, 1]` for both
  fields (NaN/`Inf` and out-of-range values return the bounded error).
- `from_core` round-trips `StreamLoss` back into the DTO; the M036
  temporary `unreachable!` arm is removed.
- Wire fixture pinned in the protocol tests:
  `{"type":"stream-loss","loss_rate":0.2,"correlation":0.4}`.
- `protocol/tests/contract_drift.rs` adds the new tag to the shared
  `stream_fault_tags` set and to the per-tag round-trip loop, so the
  OpenAPI drift check enforces that both authority surfaces list the
  variant.

### Server config TOML (`crates/eggchaos-server/src/config.rs`)

- `FaultFileConfig` gains `loss_rate: Option<f64>` and
  `correlation: Option<f64>` (both required when `type = "stream-loss"`).
- New `"stream-loss"` arm constructs `FaultKindV1::StreamLoss`; missing
  fields produce `Field{fault.loss_rate, ...}` / `Field{fault.correlation,
  ...}` errors that match existing per-field validation style.

### Server evidence (`crates/eggchaos-server/src/runtime/`)

- `ConnectionSnapshot` gains six additive `#[serde(default)]` fields:
  `upstream_stream_loss_chunks_evaluated/dropped/bytes_discarded` and
  the same for `downstream_*`. These are merged from the existing
  `StreamEvidence.snapshot()` in `merge_evidence` so they always agree
  with the legacy `activations` array and the aggregate `bytes_discarded`.
- `ConnectionSnapshot` construction in `supervisor.rs` initializes the
  six new fields to zero.

### CLI (`crates/eggchaos-cli/src/main.rs`)

- `FaultParams` gains `--loss-rate` and `--correlation` (`f64`,
  `allow_hyphen_values`).
- `build_kind` adds the `stream-loss` arm: both fields come through
  `required()` so a missing flag surfaces the same `--kind requires
  --<name>` error as every other kind.
- The unknown-kind error message lists the new kind:
  `latency, bandwidth, blackhole, limit-data, slow-close, slice,
  disconnect, stream-loss`.

### Toxiproxy (`crates/eggchaos-toxiproxy/src/lib.rs`)

- The M036 temporary `attrs_from_kind` `unreachable!` arm is renamed to
  document M038 ownership; it remains unreachable because strict v2.12
  has no `packet_loss` toxic and native authoring cannot construct a
  `FaultKind::StreamLoss` through the strict adapter.
- New focused regression test `strict_v212_rejects_post_v212_packet_loss_toxic`
  posts a `packet_loss` toxic to a loopback strict-v2.12 server and
  asserts a `400 invalid toxic type` response. This locks the M037
  promise that strict v2.12 still rejects the post-v2.12 toxic before
  the snapshot profile is added in M038.

### OpenAPI (`api/openapi/eggchaos-v1.yaml`)

- `StreamFaultKind.oneOf` adds `StreamFaultStreamLoss`.
- Discriminator mapping adds `stream-loss`.
- New schema with required `type / loss_rate / correlation`,
  `additionalProperties: false`, numeric bounds `[0, 1]`, and an
  explicit description clarifying that this is userspace stream-chunk
  loss, not IP/TCP packet loss.

### SDK drift (`scripts/sync_sdk_contract.py`, regenerated artifacts)

- `STREAM_FAULT_TAGS` (Python) and `StreamFaultTag` (TypeScript)
  include `stream-loss` after regeneration from the OpenAPI authority.
- `bindings/_contract/cross_language_fixtures.json` adds the
  `stream_fault_stream_loss` case pinned to the exact shared wire body.

### Python client (`bindings/python-client/`)

- New `@dataclass class StreamLossFault` with `type = "stream-loss"`,
  `loss_rate: float`, `correlation: float`, and a `to_dict()` that emits
  both fields explicitly.
- Added to `StreamFaultKind` union, `_STREAM_FAULTS` discriminator map,
  and re-exported from `eggchaos_client/__init__.py`.
- Tests:
  - `test_stream_fault_wire_shapes_match_m032_contract` includes the
    new dataclass and its exact wire body.
  - `test_stream_faults_serialize_to_shared_wire` extends the cross-
    language fixture loop with the new wire case.
  - `test_sync_client_full_flow` adds a stream-loss round-trip (add →
    read → patch → delete) under the live server.

### TypeScript client (`bindings/typescript-client/`)

- New `StreamLossFault` interface (`type: "stream-loss"`,
  `loss_rate: number`, `correlation: number`).
- Added to `StreamFaultKind` union and `STREAM_FAULT_TYPES` set so the
  `decodeStreamFault` discriminator accepts the new tag.
- `live.test.ts` adds a stream-loss round-trip (add → read → delete)
  under the live server.
- `cross_language.test.ts` extends the mock-transport sequence with
  the new fixture.

### Python native pilot (`bindings/python-native/src/lib.rs`)

- New `Fault.stream_loss(id, *, direction, loss_rate, correlation,
  probability=None)` static method that emits
  `{"type":"stream-loss","loss_rate":...,"correlation":...}` as the
  `kind`. Conversion goes through the same `FaultUpsertV1::into_core`
  path so embed/native-Python/HTTP round-trips are byte-identical.

## Tests

Mandatory M037 tests passing on the exact candidate:

- `cargo test -p eggchaos-protocol --all-features` (14 unit + 7 contract
  drift + 6 doc-tests).
- `cargo test -p eggchaos-server --all-features` (134 tests).
- `cargo test -p eggchaos-cli --all-features` (10 tests).
- `cargo test -p eggchaos-toxiproxy --all-features` (including the new
  `strict_v212_rejects_post_v212_packet_loss_toxic`).
- `cargo test -p eggchaos-embed --all-features`.
- `cargo test --workspace --all-features` (whole workspace, all green).
- `cargo doc --workspace --all-features --no-deps`.
- `scripts/check_openapi.sh`.
- `scripts/check_python_client.sh` (regeneration drift + pytest, 12
  passing tests including the new model and cross-language cases).
- `scripts/check_typescript_client.sh` (regeneration drift + tsc +
  tests, 6 passing tests including the live stream-loss round-trip).
- `scripts/qualify_language_clients.sh` (real loopback server with
  both auth and open admin, full Python pytest suite + npm test +
  sdist/wheel + npm pack artifacts).
- `scripts/check_python_native.sh` (Rust facade tests, audit, unsafe-
  boundary grep, abi3 wheel, 10 Python tests).
- `TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)"
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 ./scripts/qualify_toxiproxy_v2_12.sh`
  passes 50/50 differential vs pinned v2.12.0; strict v2.12 continues
  to reject `packet_loss` and never accepts the post-v2.12 toxic.

## Acceptance checklist (per M037)

- [x] Every native stream fault authoring/control surface supports
  `stream-loss` (HTTP, TOML, CLI, OpenAPI, embed, Python-native).
- [x] All surfaces consume the M036 core semantic authority (no
  second RNG/chunking/clamping path).
- [x] OpenAPI describes the new fault with the correct discriminator
  and the explicit userspace clarification, drift checks pass.
- [x] Python and TypeScript SDKs expose and live-test the new fault
  end-to-end against a real server (qualify_language_clients.sh).
- [x] Embed and Python-native expose it through the existing safe
  facade / JSON conversion with no data-plane FFI.
- [x] Scenario V1/V2 publish the new variant through the shared
  `FaultKindV1`/`FaultPlan` path; existing v2 fingerprints for plans
  that don't use the new variant remain byte-identical (the experiment
  fingerprint gains a `stream-loss;loss_rate=...;correlation=...`
  arm without changing any other fingerprint output).
- [x] Legacy seven-slot activation arrays remain unchanged.
- [x] Existing Scenario / datagram / native fixtures remain stable.
- [x] Strict Toxiproxy v2.12 still rejects `packet_loss` (focused
  unit test + the 50/50 differential corpus remains green).
- [x] All required gates pass on one exact candidate (see Tests).
- [x] Closure evidence records contract changes and package/platform
  qualification (this document).

## Stop/rejection review

- No surface implements its own loss RNG/chunking/clamping: every
  authoring path lands in `FaultKindV1::StreamLoss` →
  `FaultKind::StreamLoss` → core `DirectionEngine`.
- The fixed 32 KiB grain is only a public constant; it does not leak
  onto any mutable request/config field.
- OpenAPI and protocol agree (drift check enforces it).
- SDKs derive from the regenerated `_generated.py` /
  `generated.ts`; no hand-copied divergent fault models.
- Scenario V2 existing fingerprints for plans that don't use the new
  variant remain byte-identical because the experiment fingerprint
  only adds a new arm to `write_stream_fault_kind` for the new
  variant.
- Strict v2.12 accepts the post-v2.12 toxic? No —
  `strict_v212_rejects_post_v212_packet_loss_toxic` plus the unchanged
  50/50 v2.12.0 differential both confirm strict rejection.
- Datagram loss is not renamed/reused for stream loss: protocol
  `FaultKindV1` and `DatagramFaultKindV1` remain disjoint.
- Evidence arrays are not resized: the additive `stream_loss_*`
  fields exist alongside the seven-entry activation arrays in the
  DTO and `EngineEvidence`.

## Follow-on activation

A clean M037 closure makes **M038 ready**. M038 may add the
Toxiproxy `packet_loss` spelling only by translating it into the
proven `StreamLoss` primitive through the opt-in pinned snapshot
profile. Strict v2.12 remains default and frozen.

## Additive M040 corrective reference (2026-09-26)

This historical M037 implementation record is preserved. Its implementation
commit is `a521b093cac5319183b6e34aef352afb7be43533`; final ADR 007 corrective
qualification and repository authority are recorded in
`plans/closure/M040-post-v2-12-stream-loss-corrective-requalification-closure.md`.
