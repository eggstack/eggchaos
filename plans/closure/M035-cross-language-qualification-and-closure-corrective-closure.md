# M035 — Cross-Language Qualification and Closure Corrective — Closure

- Milestone: M035 (`plans/035-cross-language-qualification-and-closure-corrective.md`).
- Candidate commit: `a710cd65317ccab5bab86b37491c1640a9a85183`
  (`M035 implement cross-language qualification corrective`).
- Implementation commits: `d43bf8f` (registration/baseline, planning only)
  → `a710cd6` (all M035 code, script, CI, and doc changes; no other
  commits in between).
- Baseline: `ca53ac892206f17ec12a81494f229e74a6b23d56` (M035 plan §Baseline).

## What changed (WP1–WP7)

- WP1/WP2 — Hosted SDK false-red fixed: `scripts/qualify_language_clients.sh`
  and `scripts/qualify_python_native.sh` now use status-preserving child
  cleanup (captured qualification status, `set +e` inside cleanup, guarded
  `wait` reaping of known PIDs so expected SIGTERM never leaks exit 143,
  temp removal, exit with the original status). New regression
  `scripts/tests/test_cleanup_traps.sh` pins the trap shape on both scripts
  and proves pass→0 / fail→nonzero with child reaping and temp cleanup.
- WP3/WP4 — Native-Python portability + hosted gate: target selection in
  `check_python_native.sh` / `qualify_python_native.sh` derives from OS +
  architecture (Apple targets only on Darwin; native-host elsewhere;
  `EGGCHAOS_NATIVE_TARGET` override for intentional cross builds, never
  runtime-qualified without a matching import);
  `build_python_native_artifacts.sh` only cross-builds Apple wheels on a
  Darwin host. New dedicated `python-native` CI job
  (`[ubuntu-latest, macos-latest] × python 3.12`, pinned
  `maturin==1.9.5`) runs `check_python_native.sh` +
  `qualify_python_native.sh`; the `language-clients` job additionally runs
  the cleanup-trap regression.
- WP5/WP6 — One datagram fault mutation authority: `ControlState` now owns
  `add/get/list/update/remove_datagram_fault` in core/runtime types
  (same-direction duplicate, cross-direction ID uniqueness, patch
  non-emptiness after DTO conversion, plan reconstruction,
  generation-guarded publication). `NativeAdmin` and `eggchaos-embed` only
  convert DTOs (`datagram_fault_upsert_into_runtime`,
  `datagram_fault_patch_into_runtime`) and map errors; neither contains
  plan reconstruction or publication logic anymore. New conformance test
  `datagram_facade_and_http_admin_agree_on_mutation_and_conflicts` plus a
  new embed `list_datagram_faults` prove HTTP/embed equivalence across
  create/get/list/patch/delete, both conflict classes, empty-patch
  rejection, and post-delete not-found.
- WP7 — Planning/docs reconciled: `plans/registry.md`, `plans/README.md`,
  `plans/roadmap.md`, and `AGENTS.md` already carried the
  `M032→M033→M034 closed, M035 sole ready` handoff from registration and
  were verified free of stale ready/blocked language;
  `architecture/tooling-distribution.md`,
  `architecture/verification-qualification.md`,
  `architecture/server-runtime.md` now describe the new jobs, regression,
  and shared authority; `bindings/python-native/README.md` lists
  hosted runtime-qualified vs built-only vs unqualified platforms.
- Incidental repair (required to run the declared fuzz gate): the
  `native_control_json` fuzz target called the pre-M032
  `RuntimeConfigV1::limits()` assembler, which no longer exists, so the
  fuzz workspace did not build; replaced with the current `validate()`
  entry (same no-panic-over-arbitrary-bytes intent) and refreshed the
  stale `fuzz/Cargo.lock` (missing `eggchaos-experiment` /
  `eggchaos-protocol` packages). No fault/control semantics changed.

## Commands executed and results (all on the exact candidate)

Local host: Darwin x86_64 interpreter / `aarch64-apple-darwin` Rust host,
Python 3.14, maturin from PATH. Every command below exited 0 unless noted.

- `./scripts/check.sh` → 0 (fmt, clippy `-D warnings`, full workspace tests,
  doc).
- `sh scripts/tests/test_cleanup_traps.sh` → 0
  (`{"cleanup_traps":"pass"}`).
- `./scripts/check_openapi.sh` → 0 (`{"openapi":"pass","operations":36}` —
  operation inventory unchanged).
- `./scripts/check_python_client.sh` → 0; `check_typescript_client.sh` → 0.
- `./scripts/qualify_language_clients.sh` → 0
  (`{"language_clients":"pass"}`; previously exited 143 after the same
  marker — the false-red is gone).
- Negative-path probe: temporarily injected a failing Python client test,
  re-ran the script → exit 1 (genuine failures still propagate; no
  masking). Probe removed afterwards.
- `./scripts/check_python_native.sh` → 0 (`{"python_native":"pass"}`).
- `./scripts/qualify_python_native.sh` → 0
  (`{"python_native_qualify":"pass"}` incl. conformance + overhead table).
- `./scripts/benchmark_datagram.sh` → 0 (`datagram_budget` + `matched_budget`
  pass; M024 budgets retained).
- `./scripts/qualify_eggfetch.sh` → 0.
- `TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)"
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1
  ./scripts/qualify_toxiproxy_v2_12.sh` → 0; `translation:pass`,
  `differential:pass` vs pinned `toxiproxy-server 2.12.0`
  (checksum-verified; 50 passed / 0 failed with the 4 declared
  normalizations).
- `./scripts/release-smoke.sh` → 0 (incl. order-proof
  `core->experiment/eggfetch->protocol->server/toxiproxy/cli->embed` and
  artifact smoke); `./scripts/release-artifact-smoke.sh` → 0.
- `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh` → pass, 9/9 targets
  (`plan_json`, `datagram_plan_json`, `datagram_transitions`,
  `native_config`, `native_control_json`, `fault_evidence_json`,
  `policy_transitions`, `toxiproxy_attributes`, `scenario_v2`).

## Platforms / environments

- Local: Darwin host as above (all gates green).
- Hosted exact-candidate CI: workflow run
  `https://github.com/eggstack/eggchaos/actions/runs/36168309919`
  on `a710cd6`, conclusion `success` — all 13 jobs green:
  - `check` on `ubuntu-latest`, `macos-latest`, `windows-latest`;
  - `language-clients` on `[ubuntu,macos] × python 3.11/3.12 × node 20/22`
    (8/8 green — the previously false-red job);
  - `python-native` on `ubuntu-latest` and `macos-latest` (Python 3.12),
    i.e. Linux x86_64 and hosted macOS ARM64 native-host wheels
    import/runtime-smoked with remote/native conformance green.

## External oracle / versions

- Toxiproxy v2.12.0 oracle via `fetch_toxiproxy_v2_12.sh` (pinned SHA-256,
  version asserted by the script); strict mode required and passed.
- `cargo audit --deny warnings` + `cargo deny check advisories licenses bans
  sources` green in CI and release-smoke on the candidate.
- maturin `1.9.5` pinned in the `python-native` CI job, matching the plan.

## Evidence artifacts / fixtures

- `scripts/tests/test_cleanup_traps.sh` (new regression).
- `crates/eggchaos-embed/tests/facade.rs::datagram_facade_and_http_admin_agree_on_mutation_and_conflicts`
  (new conformance) + new embed `list_datagram_faults`.
- Wheel/sdist/tarball artifacts built during hosted + local language and
  native qualification (no publication).
- Local gate transcripts retained on the qualifying host (`/tmp/m035-*.log`).

## Unresolved warnings / findings

- None medium-or-higher. Local Python 3.14 runs emit a pre-existing
  `pytest-asyncio` fixture-scope deprecation warning (test tooling only,
  not eggchaos code).

## Limitations (explicit non-claims)

- Windows native-Python wheels remain unqualified (no hosted gate, no
  import smoke) — `bindings/python-native/README.md` lists Windows as
  unqualified.
- macOS cross-arch wheels are built-only unless a matching interpreter
  import-smokes them.
- No generic C ABI, Node native addon, JNI, P/Invoke, cgo, UniFFI, or WASM
  was added (per plan non-goals).
- Native `/v1` JSON/OpenAPI behavior and the 36-operation inventory are
  unchanged; datagram deterministic traces and generation CAS semantics
  are unchanged (existing golden-trace suites green).
- M032–M034 historical closure records and candidates are untouched.

## Acceptance-criteria verdict

All M035 acceptance criteria are met: both qualification scripts return
zero after successful child cleanup on hosted Linux and macOS while
injected failures propagate nonzero; the exact-candidate hosted matrix
(13/13 jobs, incl. the new native-Python gate) is green; stale
ready/blocked narrative is absent from all current-state documents;
native scripts are host-aware with truthful per-platform support claims;
one server-side datagram fault mutation authority serves HTTP and embed
with CRUD-plus-conflict conformance; all listed Rust/SDK/binding/
Toxiproxy/EggFetch/datagram/package/security/performance gates are green
on `a710cd6`; no handwritten unsafe or generic ABI was introduced.

## Registry transition

`plans/registry.md`: M035 `ready` → `closed` (evidence: this record, exact
candidate `a710cd6`, hosted run `36168309919`). `plans/README.md`,
`plans/roadmap.md`, and `AGENTS.md` current-state text updated to match.

## Next activation / follow-on rules

M035 activates no automatic successor. ADR 006 and its corrective
successor are fully reconciled. No milestone was blocked on M035
(registry `Blocked: none`), so no further status transitions are
unblocked by this closure. Future language SDKs reuse the M032 OpenAPI
contract; future native bindings require concrete consumer demand; a
generic C ABI still requires a separate ADR plus multi-consumer evidence.
