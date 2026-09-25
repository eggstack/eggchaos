# M032 — Native Protocol Contract Extraction and OpenAPI Foundation — Closure

Status: closed
Exact qualification candidate: `ed05f68e07bfc4cca62317c5b1757bb3966da6ec`
Depends on: M031 (closed at `fa189b9`), ADR 006 (accepted)

## Objective verdict

M032 extracts the stable native `/v1` wire contract from
`eggchaos-server` into the narrow publishable `eggchaos-protocol` crate,
preserves server behavior through compatibility re-exports plus
server-side adapters, and freezes a mechanically drift-checked OpenAPI
contract at `api/openapi/eggchaos-v1.yaml`. No foreign-language SDK, no
FFI, and no native route-semantics change were introduced. No stop
condition fired; M033 is now ready.

## WP1 — Inventory and frozen contract

The baseline inventory was taken from `admin.rs::route` (36 operations
across 21 paths) and `native.rs`/`native_v2.rs` DTOs. Pre/post JSON
equality is proven by byte-exact fixture assertions carried over
verbatim from the server suite into the protocol suite (same expected
strings for every stream/datagram fault variant, proxy create/patch,
Scenario V1/V2 documents), plus the new
`crates/eggchaos-protocol/tests/fixtures/*.json` golden corpus
(`stream_faults.json` 7/7, `datagram_faults.json` 6/6, `proxy.json`,
`scenario_v1.json`, `scenario_v2.json`, `errors.json`).

Note: fixtures were frozen as moved-over assertions plus new golden
files rather than as a separate pre-move commit; equivalence rests on
identical expected strings and a green suite, not on a pre-move commit
hash.

## WP2 — `eggchaos-protocol` crate

New workspace crate with dependencies only on `eggchaos-core`,
`eggchaos-experiment`, `serde`, `serde_json`, `toml` (plus dev-only
`serde_yaml` for the drift test). Modules: `common` (error envelope,
health/version, media-type/version constants), `stream` (V1 stream,
proxy, Scenario V1 DTOs with core-only conversions), `scenario_v2`
(V2 DTOs converting to `eggchaos-experiment` types), `routes`
(`NATIVE_OPERATIONS`, 36 entries, plus `OPENAPI_CONTRACT_VERSION`).

The crate opens no sockets, owns no runtime/tasks, and does not depend
on EggServe, CLI, Toxiproxy, EggFetch, EggReplay, or EggProbe.
`unsafe_code` remains denied.

## WP3 — Server compatibility adapters

`eggchaos-server` consumes the protocol crate. `native.rs` is now
re-exports plus server-side assembly (`proxy_request_into_spec`,
`fault_upsert_into_runtime`, `fault_patch_into_runtime`,
`scenario_v1_into_runtime`, `datagram_proxy_request_into_spec`,
`datagram_fault_patch_into_parts`, `runtime_admission_limits`,
`runtime_datagram_limits`, view `From` impls); `native_v2.rs` is pure
re-exports. `admin.rs` renders errors through the shared
`ErrorEnvelopeV1`. All pre-existing `eggchaos_server::*` DTO imports
(CLI, config, tests) keep compiling; only two CLI/config call sites
moved from removed methods to the public limit adapters. No second
state store exists; `ControlState` remains the single authority.

## WP4 — Route/operation metadata

`NATIVE_OPERATIONS` is the machine-readable inventory. The live proof
is `crates/eggchaos-server/tests/native_route_inventory.rs`, which
exercises all 36 operations against a real loopback admin listener and
fails on any `"route not found"` envelope.

## WP5 — OpenAPI generation/check

`api/openapi/eggchaos-v1.yaml` (OpenAPI 3.0.3, chosen for mature Python
and TypeScript generator support) covers all 21 paths / 36 operations
with bearer-auth modeling, JSON media types, Prometheus text for
`/metrics`, fault/action discriminators, defaults, integer duration
units, and the bounded error envelope. Single-authority direction is
plan option 3: `contract_drift.rs` proves the checked-in document
against protocol DTO serialization and `NATIVE_OPERATIONS`
(operation-set equality, operationId equality, discriminator-tag
equality for stream/datagram/scenario actions, required-field
equality, bearer scheme). `./scripts/check_openapi.sh` runs the
protocol suite, the live inventory test, and a YAML shape assertion.

## WP6 — Golden and negative contract tests

Protocol suite (27 tests): exact wire fixtures, round trips for all 7
stream and 6 datagram fault variants through core conversion,
Scenario V1/V2 JSON semantic equivalence (V2 JSON→compile fingerprint
equals itself across canonical re-parse), unknown-field rejection,
malformed-discriminator rejection, validation boundaries, error
envelopes, name-rule parity between `validate_proxy_name` and
`ProxySpec::validate` on a shared corpus, and `RuntimeConfigV1` bound
agreement with the server adapters.

## WP7 — Package/release integration

`release-smoke.sh` lists `eggchaos-protocol` in `cargo package --list`
and proves publish order
`core->experiment/eggfetch->protocol->server/toxiproxy/cli` from
`cargo metadata` (`{"order_proof":"pass",...}` on the candidate).
`cargo deny` (advisories/licenses/bans/sources) and `cargo audit` are
clean with the added `serde_yaml` dev-dependency. New publish order is
recorded in `AGENTS.md` and `architecture/tooling-distribution.md`.

## WP8 — Documentation/planning reconciliation

`docs/control-plane.md`, `docs/architecture.md` (unchanged semantics),
`architecture/control-plane-cli.md`, `architecture/overview.md`,
`architecture/tooling-distribution.md`, and `AGENTS.md` identify the new
ownership boundary. Registry/roadmap updated in the closure commit;
M033 unblocked.

## Evidence (exact candidate `ed05f68`, darwin x86_64, rustc 1.89.0)

- `./scripts/check.sh`: pass (fmt, clippy `-D warnings`, workspace
  tests 23/23 suites ok, doc).
- `./scripts/check_openapi.sh`: pass
  (`{"openapi":"pass","paths":21,"operations":36}`).
- `./scripts/release-smoke.sh`: pass incl. artifact smoke and order
  proof (ran on the identical pre-commit tree).
- Pinned Toxiproxy oracle
  (`TOXIPROXY_SERVER=... EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1
  ./scripts/qualify_toxiproxy_v2_12.sh`):
  `{"translation":"pass","oracle":"toxiproxy-server 2.12.0 (checksum
  verified)","differential":"pass"}` (50/50 differential observations,
  0 failures).
- `./scripts/qualify_eggfetch.sh`: pass (identical pre-commit tree).
- Existing stream/datagram/Scenario V2 golden traces: unchanged and
  green (no fixture edits outside the move).

## Limitations

- `ConnectionSnapshot`/`ClosedConnection`/`ResetReport` remain
  server-owned runtime views serialized directly; the OpenAPI document
  describes their exact observed shapes but the DTO move did not extend
  to them (explicitly permitted: runtime-only views stay in the
  server).
- Externally-tagged enums (`ConnectionOutcome`, `ResetResult`) are
  documented descriptively rather than as exhaustive `oneOf` schemas;
  the drift test pins only the fully-owned DTO discriminators.
- No foreign SDK or FFI code was introduced (per non-goals).

## Follow-on activation

M032 closure makes M033 ready. M033 must generate/derive from the exact
`api/openapi/eggchaos-v1.yaml` + `eggchaos-protocol` authority closed
here (contract version `1.0.0`, candidate `ed05f68`); it must not
reconstruct schemas independently.
