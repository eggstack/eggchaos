# M054 — Post-M048 Corrective Tranche Exact-Head Qualification Closure

Status: closed

Exact M054 qualification/evidence candidate:
`3b4fd9f17c71234d6bd0c73c79741db636f74a92`

Activation baseline (per plan): `9390008` (M053 closure).
The delta `9390008..3b4fd9f` is planning/docs only (M053 closure
record, registry row flip, regenerated current-state blocks); zero
production Rust changed, so the candidate carries the identical
M049–M053 implementation already qualified by the M053 hosted run.

Closure date: 2026-09-28

Depends on: M049, M050, M051, M052, M053 closed.

Hosted run: [36461914295](https://github.com/eggstack/eggchaos/actions/runs/36461914295)
on candidate `3b4fd9f`, conclusion `success` — 14/14 jobs green (3
`check` + 1 `performance-provenance` + 8 `language-clients` + 2
`python-native`, including the M048 provenance Tier B job and the
M053 planning-state guard steps).

## Outcome

M054 qualifies the combined M049–M053 corrective tranche on one
exact head and reconciles all current-state documentation. No
production feature was added, no threshold retuned, no benchmark
redesigned, no release/tag action taken. Every declared gate is
green; the two transient first-attempt failures below reproduced
green on retry against the unchanged candidate with identical code
(one shared-runner timing flake, one stale-oracle bind race) and
are recorded as noise, not regressions.

## Qualification matrix (all evidence generated on `3b4fd9f`, clean tree)

### Tier 1 — deterministic local gate

| Command | Result |
| --- | --- |
| `./scripts/check.sh` | exit 0 (`{"bench_provenance":"pass"}`, `{"planning_state":"pass"}`, fmt/clippy/workspace-tests/doc green) |
| `./scripts/check_openapi.sh` | `{"openapi":"pass","paths":21,"operations":36}` |
| `./scripts/check_python_client.sh` | `{"python_client":"pass"}` (12 passed) |
| `./scripts/check_typescript_client.sh` | `{"typescript_client":"pass"}` |

### Tier 2 — compatibility/integration (mandatory oracles)

| Command | Result |
| --- | --- |
| `TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 ./scripts/qualify_toxiproxy_v2_12.sh` | `{"translation":"pass","differential":"pass"}`, 50/50 vs pinned `toxiproxy-server 2.12.0` (checksum verified) |
| `TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh)" EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1 ./scripts/qualify_toxiproxy_post_v2_12.sh` | `{"translation":"pass","differential":"pass"}`, 12/12 exact + stochastic/correlation probes vs source-build `40f7fd31` |
| `./scripts/qualify_eggfetch.sh` | exit 0 (all suites green, incl. 152-test integration file) |
| `./scripts/qualify_language_clients.sh` | `{"language_clients":"pass"}` |
| `./scripts/qualify_python_native.sh` | `{"python_native_qualify":"pass"}` (11 conformance tests + native/remote latency probes) |

Post-v2.12 note: the first attempt failed with `ConnectError ...
127.0.0.1:18748 Connection refused` (oracle bind race against a
stale oracle process). Retry on the unchanged candidate passed
12/12 (`DIFFERENTIAL_SUMMARY ... "failed":0`, oracle
`oracle_dropped=61 eggchaos_dropped=69` within the stochastic
intent band, `correlation ... oracle_gap=0.4992 eggchaos_gap=0.5210`).
No code changed between attempts.

### Tier 3 — fuzz/security/package

| Command | Result |
| --- | --- |
| `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh` | `{"fuzz":"pass","targets":9}` (all 9 targets × 10k runs, `--sanitizer none` under pinned 1.89 per release guidance) |
| `./scripts/release-smoke.sh` | `{"artifact_smoke":"pass"}`, `{"order_proof":"pass",...,"order":"core->experiment/eggfetch->protocol->server/toxiproxy/cli->embed"}` |
| `./scripts/release-artifact-smoke.sh` | `{"artifact_smoke":"pass"}` |
| `cargo audit` / `cargo deny` | via hosted CI (green on the exact-head run) |

### Tier 4 — performance non-regression (smoke, same topology)

- Datagram (`./scripts/benchmark_datagram.sh`, macOS arm64, clean tree,
  candidate `3b4fd9f`): built-in budgets report `datagram_budget: pass`
  and `matched_budget: pass`; `matched_empty_over_bare_throughput:
  0.9365`, `matched_windowed_empty_over_bare: 1.0721`,
  `matched_empty_over_bare_p95: 0.982`.
- Stream (`./scripts/benchmark.sh`, two runs): absolute times were
  environment-dominated (host load average 10–16 during the runs, box
  13–17 min post-boot with Spotlight/trustd settling), with all cases
  including the bare `eggress-relay` baseline slowed ~5–10× uniformly.
  Within-run no-fault vs bare-relay ratios bracket the M047 baseline
  ratio on both sides (run 1: 0.536, M047 baseline artifact: 0.785,
  run 2: 1.191) — run-to-run noise at microsecond scale in both
  directions on the same candidate, i.e. no systematic no-fault
  overhead. Corroborated by construction: the tranche contains no
  hot-path change (serde validation routing + verbatim module moves
  + new private modules only).
- No budget retuned, no new retained artifact claimed as
  exact-candidate evidence (smoke outputs stayed in temp).

### Tier 5 — hosted exact-head

Run `36461914295` on `3b4fd9f`: `success`, 14/14. First attempt
tripped the M048 Tier B short-datagram budget run (`FAIL: wrapped
short datagram run failed (budget or harness)` on shared Linux);
identical code had passed Tier B on `9390008` (run `36460900207`)
minutes earlier and the local full datagram budget passed, so the
job was rerun via `gh run rerun --failed` and went green without
any code change — recorded as shared-runner timing noise. Final
run conclusion `success` covers all ordinary jobs including the
M048 `performance-provenance` job and the M053 planning-state
guard steps in `check` and all 8 `language-clients` jobs.

## Public/capability non-regression census (M048 baseline `4febd52` → `3b4fd9f`)

- Workspace crate list: unchanged (8 crates: cli, core, eggfetch,
  embed, experiment, protocol, server, toxiproxy).
- `git diff 4febd52..3b4fd9f --stat -- crates/eggchaos-cli
  crates/eggchaos-protocol docs/control-plane.md
  api/openapi/eggchaos-v1.yaml`: empty — CLI inventory, wire DTOs,
  OpenAPI contract, and control-plane docs untouched.
- No `pub fn/struct/enum/const/type` line removed in
  `crates/eggchaos-core/src/plan.rs`; `eggchaos-embed` diff is
  signature-preserving delegation plus formatting (all
  `EmbeddedService` signatures and `EmbedError` categories intact,
  per M051 evidence).
- All 36 native operations present (Tier 1 OpenAPI gate).
- Strict Toxiproxy v2.12 remains default (`packet_loss` rejected as
  unknown); post-v2.12 `packet_loss` remains opt-in profile-gated
  (`accepts_packet_loss`), verified by both oracle differentials.
- Scenario V1/V2 response shapes unchanged; per-family capacity
  `SCENARIO_FAMILY_CAPACITY = 32` preserved (M050 registry).
- Python/TypeScript/Python-native operation coverage green (Tier 2).
- Eggfetch `Dialer` adapter behavior green (Tier 2); fixed-target
  TCP/UDP boundary untouched (no listener changes in tranche diff).
- No `eggreplay-*`/`eggprobe-*` dependency, no `cdylib`/C-ABI
  surface: `grep` over `crates/*/Cargo.toml` clean.

## Planning reconciliation (this closure change)

- `plans/registry.md`: M054 row `ready` → `closed` with this
  closure reference; `Last reconciled` line, tranche paragraph,
  dependency-ready view (`Ready: none`, `Blocked: none`), and
  `Completed work: M000–M054` updated.
- `plans/README.md`, `plans/roadmap.md`: tranche prose closed out;
  generated `Current planning state` blocks regenerated via
  `python3 scripts/check_planning_state.py --write` (highest closed
  `M054`, no ready milestone, execution order empty).
- `AGENTS.md`, `architecture/overview.md`: generated blocks
  regenerated the same way.
- `sh scripts/tests/test_planning_state.sh` green after
  reconciliation; `python3 scripts/check_planning_state.py --check`
  reports all four target documents match.
- No file under `plans/archive/`, no numbered plan `Status:`
  header, and no prior closure record modified (registry remains
  the sole hand-maintained authority).

## Acceptance mapping (M054 § Acceptance criteria)

1. M049–M053 exact-candidate closure evidence: yes (closure files
   `M049`–`M053`, candidates `bbe3b43`/`b2891a3`/`68cc957`/
   `a5cf5a2`/`9390008`).
2. Full local check green: yes (`./scripts/check.sh` exit 0).
3. Both mandatory oracle qualifications green: yes (50/50 +
   12/12, pinned oracles).
4. Eggfetch/language-client/Python-native green: yes.
5. Bounded fuzz + release/package smoke green: yes.
6. Hosted exact-head CI green: yes (`36461914295`, 14/14).
7. No material performance regression: yes (Tier 4 disposition
   above; budgets pass, stream tracks bare relay within noise).
8. Public/capability census clean: yes (census above).
9. Planning-state guard green on final tree: yes (verified after
   `--write`; re-verified by `check.sh` before commit).
10. This record identifies the authoritative exact candidate:
    `3b4fd9f`.

## Verdict

M054 is closed on exact candidate `3b4fd9f`. The M049–M053
corrective tranche is fully qualified with no API, capability,
compatibility, determinism, or performance regression.

## Follow-on rule

M054 activates no automatic successor. Scenario enumeration,
egress chaining, new fault models, additional language bindings,
and downstream EggReplay/EggProbe adapters remain separate
feature decisions and must be planned independently.
