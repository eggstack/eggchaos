# M033 — Python and TypeScript Native Control SDKs — Closure

Status: closed
Exact qualification candidate: `429d459a7f1154907fe37e3ff961e577406eafef`
Depends on: M032 (closed at `ed05f68`), ADR 006 (accepted)

## Objective verdict

M033 builds and qualifies complete Python and TypeScript/Node remote
control SDKs over the exact M032 native `/v1` contract. Both cover all
36 operations (stream/datagram resources, Scenario V1/V2,
connection/association evidence, history, reset, auth, errors, metrics
text), derive from the same OpenAPI authority, build reproducible
package artifacts without publication, and add no FFI, no native
extension, and no daemon lifecycle management. No stop condition fired;
M034 is now ready.

## WP1 — Generator/toolchain choices

Full IDL codegen (openapi-generator targets) was evaluated and rejected
for this tranche: pathological dependency weight for a 36-operation API
and lossy `uint64`/discriminator fidelity versus the handwritten
alternative. Recorded decision per the plan: generated operation/method
tables plus a small handwritten HTTP transport in each language; the
M032 OpenAPI document remains the semantic source. Pinned toolchains:
Python `3.11`/`3.12` (CI matrix; qualified locally on 3.14.2),
`typescript@5.7.2` + `@types/node@22.10.2` (pinned via
`package-lock.json`), Node `20`/`22` (CI matrix; qualified locally on
v26.8.2). Regeneration command: `python3
scripts/sync_sdk_contract.py`; both language check scripts fail on any
regeneration diff.

## WP2/WP3 — Clients and facades

- `bindings/python-client/eggchaos_client/`: stdlib-only sync `Client`
  (`http.client`, timeouts, single reusable connection, context-manager
  cleanup) plus `AsyncClient` sharing the identical models/transport via
  `asyncio.to_thread` (no second semantics, no global-loop coupling).
  `models.py` holds typed dataclasses with discriminated
  `stream_fault_from_dict`/`datagram_fault_from_dict` decoders;
  `errors.py` holds `EggchaosError(status, code, detail)`,
  `EggchaosTransportError`, `EggchaosContractError`.
- `bindings/typescript-client/src/`: zero-dependency `EggchaosClient`
  over injectable global `fetch`, typed discriminated unions,
  `AbortSignal` per-request options plus `timeoutMs`,
  `EggchaosError`/`EggchaosTransportError`/`EggchaosContractError`,
  raw-text `metricsText()`. Model layer uses no Node-only primitives;
  the package targets Node 18+ and browsers (Node-only qualification so
  far — see limitations).

Separation holds: generated tables (`_generated.py`, `generated.ts`)
carry operations/methods/tags; facades carry config/auth/errors;
models are not duplicated into a second layer.

## WP4 — Contract coverage fixtures

`scripts/sync_sdk_contract.py` derives `bindings/_contract/
operations.json` (36 operations, 7 stream + 6 datagram + 4 action tags)
from `api/openapi/eggchaos-v1.yaml` and emits both languages'
generated tables deterministically. Drift tests assert table equality
and that every operation resolves to a real sync+async (Python) /
client (TS) method. `bindings/_contract/
cross_language_fixtures.json` (6 wire bodies) proves equivalent inputs
serialize to identical native JSON in both languages (Python stubbed
transport, TS mock fetch).

## WP5/WP6 — Real-server matrix and negative paths

`scripts/qualify_language_clients.sh` boots a loopback `eggchaos serve`
plus a token-protected sibling, then runs equivalent Python sync, Python
async, and TypeScript flows: stream/datagram proxy+fault CRUD,
connection/association evidence, Scenario V1 apply/get, V2
validate/compile, reset/history/metrics-text, 403 auth
failure + correct-token success, percent-encoded fault IDs (`a/b`),
transport-vs-native error distinction, and TS `AbortSignal`
cancellation. Python async cancellation rides task cancellation around
`to_thread` (documented; no hidden loop coupling).

## WP7 — Package builds

`python -m build` produces `eggchaos_client-0.1.0` sdist + pure-Python
wheel; `npm pack` produces `@eggstack/eggchaos-client-0.1.0.tgz`
(registry names remain owner release decisions; nothing published).
Generated sources are byte-stable across regeneration (CI diff gate).

## WP8 — CI and docs

`.github/workflows/ci.yml` gains a `language-clients` job
(ubuntu/macos × Python 3.11/3.12 × Node 20/22) running
`check_openapi.sh`, both language check scripts, and the cross-language
qualification; the Rust core gate is untouched by registry-network
steps. `docs/control-plane.md` and
`architecture/control-plane-cli.md` distinguish remote SDKs from
Toxiproxy compatibility and future in-process bindings; both SDK
READMEs carry pytest/Node quick-starts.

## Evidence (candidate `429d459`, darwin x86_64, rustc 1.89.0)

- `./scripts/check_python_client.sh`: pass (drift + 12 unit tests).
- `./scripts/check_typescript_client.sh`: pass (tsc clean + 6 tests).
- `./scripts/qualify_language_clients.sh`:
  `{"language_clients":"pass"}` (live Python sync/async + TS flows,
  sdist/wheel + tarball artifacts).
- `./scripts/check.sh`: pass (23/23 suites; Rust regressions green).
- Pinned Toxiproxy oracle: `{"translation":"pass",
  "oracle":"toxiproxy-server 2.12.0 (checksum verified)",
  "differential":"pass"}`.
- `./scripts/qualify_eggfetch.sh`: pass (9/9 suites).
- `git diff --exit-code` on regenerated SDK artifacts: clean.

Note: follow-up commit `0a324ef` removes only accidentally committed
build artifacts (`node_modules`, `build/`, `egg-info`, `__pycache__`)
and ignores them; no SDK/Rust source differs from the candidate.

## Limitations and generator notes

- Browser compatibility is claimed by construction (no Node-only
  model-layer primitive) but qualified on Node only.
- The handwritten transports intentionally omit SDK-side retries for
  mutating operations and omit client-side Scenario V2
  compilation/fingerprinting (server authority is called instead).
- `AsyncClient` shares sync I/O via `to_thread`; it does not integrate
  with `asyncio` transports natively. Direct async I/O was not
  justified for a control client.
- Unknown future discriminators raise `EggchaosContractError`/
  `EggchaosContractError` rather than degrading; SDK minor versions
  must be refreshed with the contract.

## Follow-on activation

M033 closure makes M034 ready. M034 now has its concrete user-facing
reference: the remote Python client vocabulary (models, errors, auth,
metrics-text, absence semantics) that the native embedding surface must
conform to.
