# M057 — M056 Closure-Evidence Reconciliation and Exact-Head Requalification — Closure

Status: `closed` on exact qualification/evidence candidate
`818e5674f2efaf96ec8effda81cef1dfa7a48614` (commit `plans:
register M057 exact-head evidence reconciliation`). Registration
baseline was `c2a721cd35b4383be99d4ba744a59d810ebcafe1`.

## Objective verdict

M057 reconciles the M056 exact-candidate hosted-CI evidence gap
without changing the release workflow implementation. M056 remains
the implementation milestone for the shared `release-contract` DAG;
this record makes M057 the **final exact-head qualification
authority** for that DAG. M056 closure history is preserved verbatim
and its original hosted-CI gap stands as historical fact.

## Baseline and equivalence (WP1)

At registration:

- M056 implementation candidate:
  `b6f00950b94057934773385f679d8f576e8abd40`;
- M056 closure commit: `c2a721cd35b4383be99d4ba744a59d810ebcafe1`;
- M057 candidate (this closure): `818e5674f2efaf96ec8effda81cef1dfa7a48614`.

`git diff b6f0095..818e567 --name-only` reports only planning /
documentation / closure-evidence paths:

```text
AGENTS.md
architecture/overview.md
plans/057-m056-closure-evidence-reconciliation-and-exact-head-requalification.md
plans/README.md
plans/closure/M056-release-workflow-contract-gate-corrective-closure.md
plans/registry.md
plans/roadmap.md
```

`git diff b6f0095..818e567 -- .github/workflows/release.yml
scripts/check_release_tag_version.sh
scripts/tests/test_release_tag_version.sh
scripts/check_version_coherence.py --exit-code` is empty
(`IMPLEMENTATION-EQUIVALENT`). No package manifest, lockfile,
production Rust, test, API, fault, RNG, config, SDK, binding, or
compatibility file changed. File hashes on the candidate:

- `.github/workflows/release.yml`:
  `30bd31935f6b9aefc1417c7c1d31e5b8f46f8f81f4707427d121e34f407c1cf8`
- `scripts/check_release_tag_version.sh`:
  `b09a574809621a34e76a36a5ea64de16b27abd0654b70a11e18bc3d28ec28a1e`
- `scripts/tests/test_release_tag_version.sh`:
  `cd4551f02fcdc3fde1ba1dd8d39afb37bc1bc854174714f165d0272afb36f7ad`
- `scripts/check_version_coherence.py`:
  `56d509a2d12a444d231bf1a28a2e0c5e070802e15db5ca4fd3fb3e98f51ea0a3`

M057 is therefore qualification-only. No stop condition tripped: no
release workflow, guard, package manifest, or production file had to
change to obtain a clean run.

## Release DAG on the candidate (WP2 structural facts)

Parsed from `.github/workflows/release.yml` on the exact candidate:

```text
release-contract
       |
       +------> qualify
       |
       +------> artifacts (5-target matrix)
```

- exactly one `release-contract:` job
  (`grep -c '^  release-contract:'` → `1`);
- `release-contract` has no `needs:` (root of the DAG);
- `qualify` declares `needs: [release-contract]`;
- `artifacts` declares `needs: [release-contract]`;
- `scripts/check_release_tag_version.sh` is invoked exactly once
  (`grep -c` → `1`); the in-`qualify` second invocation remains
  removed per M056;
- artifact matrix unchanged (5 targets):
  `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
  `x86_64-apple-darwin`, `aarch64-apple-darwin`,
  `x86_64-pc-windows-msvc`, with tag-derived naming
  `eggchaos-v${GITHUB_REF_NAME#v}-<triple>` preserved;
- no automated publication action (`cargo publish`, `gh release
  create`, `create-release`, `action-gh-release`, `git tag ` all
  absent).

## Local guards on the exact candidate (WP2)

All commands run on `818e567` with a clean tree:

| Command | Result |
| --- | --- |
| `python3 scripts/check_planning_state.py --check` | `OK: 58 milestones, 4 target documents match` |
| `python3 scripts/check_version_coherence.py --check` | `{"version_coherence": "pass", "workspace_version": "0.2.0", "workspace_crates": 8}` |
| `sh scripts/tests/test_release_tag_version.sh` | five negative tests `ok` (release-contract removed; artifacts-needs dropped; qualify-needs dropped; release-contract gained needs; guard invoked twice) + `{"release_tag_version_guard":"pass","release_workflow_gate":"pass"}` |
| `sh scripts/tests/test_version_coherence.sh` | fixture self-test `ok`, deliberate-mismatch negative `ok`, `{"version_coherence_guard":"pass"}` |
| `./scripts/check.sh` | exit `0` (provenance Tier A, planning-state, version-coherence, release-tag + workflow gate, fmt, `clippy -D warnings`, workspace tests, doc) |
| `./scripts/release-smoke.sh` | `{"artifact_smoke":"pass"}` + `{"order_proof":"pass","workspace_version":"0.2.0","order":"core->experiment/eggfetch->protocol->server/toxiproxy/cli->embed"}`; no publication |
| `./scripts/release-artifact-smoke.sh` | `{"artifact_smoke":"pass"}` |

## Hosted CI on the exact candidate (WP3)

Run
[36490497114](https://github.com/eggstack/eggchaos/actions/runs/36490497114)
(`CI` workflow) on candidate `818e5674f2efaf96ec8effda81cef1dfa7a48614`:
conclusion `success`, status `completed` — 14/14 jobs green:

- 3 `check`: `check (macos-latest)`, `check (ubuntu-latest)`,
  `check (windows-latest)` — all `success`;
- 1 `performance-provenance` — `success`;
- 8 `language-clients` (`ubuntu/macos × 3.11/3.12 × 20/22`) — all
  `success`;
- 2 `python-native` (`ubuntu-latest, 3.12`; `macos-latest, 3.12`) —
  both `success`.

No ancestor or successor SHA is substituted: the run's `headSha` is
exactly the M057 candidate. The M056 gap (no hosted run on
`b6f0095`) is **not** retroactively filled and is preserved below as
historical fact; M057 provides the additive exact-head qualification
for the unchanged DAG instead.

## Release-workflow dispatch evidence (WP4)

A dedicated `workflow_dispatch` of the release workflow **was**
executed against the exact candidate:

- Run
  [36630812771](https://github.com/eggstack/eggchaos/actions/runs/36630812771)
  (`Release qualification`, `workflow_dispatch` on `main`),
  `headSha` `818e5674f2efaf96ec8effda81cef1dfa7a48614`,
  conclusion `success`, status `completed`.
- Observed DAG behavior:
  - `release-contract` → `completed`/`success` first;
  - `qualify` and all five `artifacts` matrix legs
    (`ubuntu-22.04 × x86_64/aarch64`, `macos-14 ×
    x86_64-apple-darwin/aarch64-apple-darwin`,
    `windows-2022 × x86_64-pc-windows-msvc`) started only after the
    gate succeeded and all finished `completed`/`success`;
  - both expensive branches overlapped after the gate, preserving
    valid-input parallelism;
  - no publication action occurs (none exists in the workflow).
- No deliberately invalid tag was created; the mismatched-tag bypass
  remains proven by the five local structural negative tests above.

## Unpublished statement

No tag was created, moved, rewritten, or deleted (`git tag --list`
reports only the immutable historical `v0.1.0`). No crates.io / PyPI
/ npm publication. No GitHub release. No workflow artifact upload
beyond the local release-smoke / release-artifact-smoke runs and the
dispatch run's own CI artifacts. The unreleased `0.2.0` development
baseline is intact across the workspace (`version = "0.2.0"`, all
crates `version.workspace = true`).

## Historical records preserved

- `plans/closure/M055-post-v0-1-0-release-state-and-v0-2-0-development-baseline-closure.md`
  is unchanged.
- `plans/closure/M056-release-workflow-contract-gate-corrective-closure.md`
  is unchanged, including its §`Hosted CI and workflow-dispatch
  (evidence gap)` statement that hosted CI was not run on `b6f0095`.
- No numbered plan `Status:` header was rewritten; `plans/registry.md`
  remains the sole hand-maintained status authority.
- M057 supersedes only the **final hosted qualification authority**
  for the M056 release-contract DAG, not M056's implementation
  ownership. The repository line reads: `M055
  release-state/version baseline -> M056 release-contract
  implementation -> M057 exact-head qualification authority`.

## Acceptance criteria verdict

| # | Criterion | Verdict |
| - | --------- | ------- |
| 1 | M056 release DAG implementation unchanged from the qualified state. | pass — WP1 equivalence proof above. |
| 2 | Release structural regression passes on the exact candidate. | pass — five negatives reject + structural assertions green. |
| 3 | Version/planning coherence checks pass. | pass — version-coherence + planning-state green. |
| 4 | `check.sh`, release smoke, and artifact smoke pass. | pass — all exit 0 / `pass`. |
| 5 | Normal hosted CI green on the exact M057 candidate SHA. | pass — run `36490497114`, 14/14 on `818e567`. |
| 6 | Closure record names the exact CI run and 14-job result. | pass — this file. |
| 7 | No ancestor/successor SHA substituted for exact-candidate evidence. | pass — `headSha` equals candidate for both cited runs. |
| 8 | M056 closure history unchanged; original hosted-CI gap preserved. | pass — M056 closure file verbatim. |
| 9 | Registry/roadmap identify M057 — not M056 — as final hosted qualification authority. | pass — see Planning reconciliation below. |
| 10 | No tag, registry publication, GitHub release, or production/API change. | pass — unpublished statement + WP1. |

## Planning reconciliation (this closure change)

- `plans/registry.md`: M057 row `ready` → `closed` with this
  closure reference, exact candidate `818e567`, and hosted run
  `36490497114` (14/14); M056 row note extended to name M057 as the
  closed final hosted qualification authority; `Last reconciled`
  line, M056/M057 prose, dependency-ready view (`Ready: none`,
  `Blocked: none`, `Completed work: M000–M057`), and execution-order
  paragraphs updated.
- `plans/README.md`, `plans/roadmap.md`: M057 prose closed out
  (final qualification authority; no successor); generated `Current
  planning state` blocks regenerated via
  `python3 scripts/check_planning_state.py --write` (highest closed
  `M057`, no ready milestone, execution order empty).
- `AGENTS.md`, `architecture/overview.md`: generated blocks
  regenerated the same way.
- `sh scripts/tests/test_planning_state.sh` green after
  reconciliation; `python3 scripts/check_planning_state.py --check`
  reports all four target documents match.
- No file under `plans/archive/`, no numbered plan `Status:`
  header, and no prior closure record modified.

## Follow-on rule

M057 activates no automatic feature successor. After clean closure,
the M055/M056/M057 line reads:
`M055 release-state/version baseline -> M056 release-contract
implementation -> M057 exact-head qualification authority`. The
repository remains an unreleased 0.2.0 development baseline until the
owner separately authorizes publication.
