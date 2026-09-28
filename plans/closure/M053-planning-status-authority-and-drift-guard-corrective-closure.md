# M053 — Planning Status Authority and Drift-Guard Corrective Closure

Status: closed

Exact M053 implementation/evidence candidate:
`939000849220407f55c53c3667b8083b223d07e2`

Activation baseline (per plan): `a5cf5a2` (M052 closure).
M053 is the only commit on top of the M052 closure candidate.

Closure date: 2026-09-28

Depends on: M052 closed on exact candidate `a5cf5a2`.

Hosted run: [36460900207](https://github.com/eggstack/eggchaos/actions/runs/36460900207)
on candidate `9390008`, conclusion `success` — 14/14 jobs green (3
`check` + 1 `performance-provenance` + 8 `language-clients` + 2
`python-native`; the new `python3 scripts/check_planning_state.py
--check` step is green in all 8 `language-clients` jobs).

## Outcome

M053 makes `plans/registry.md` mechanically authoritative for current
milestone state. A stdlib-only checker parses the registry table,
validates it fail-closed, and generates/checks deterministic `Current
planning state` blocks in the four documents that previously carried
hand-duplicated status prose. The stale M049-ready/M000–M048-closed
wording is reconciled everywhere without touching historical
plans, archive, or closure evidence. There is no production Rust
change.

## What landed on `9390008`

`git show --stat 9390008`:

```text
 .github/workflows/ci.yml            |  6 ++++++
 AGENTS.md                           | 31 ++++++++++++++++++++++++++++---
 architecture/overview.md            | 37 ++++++++++++++++++++++++++++++-------
 plans/README.md                     | 26 ++++++++++++++++++++++++--
 plans/registry.md                   | 10 +++++-----
 plans/roadmap.md                    | 26 ++++++++++++++++++++++++--
 scripts/check.sh                    |  9 +++++++++
 scripts/check_planning_state.py     | 505 ++++++++++++++++++++++++++++++++++++
 scripts/tests/test_planning_state.sh | 89 +++++++++++++++++++++++++++++++++++
```

### Checker (`scripts/check_planning_state.py`, stdlib-only)

- Parses only the stable five-column registry Markdown table (no
  general Markdown parser, no dependencies, no network).
- Fails closed on: duplicate milestone IDs, duplicate plan paths,
  malformed status, missing plan file, out-of-order numeric IDs,
  unknown milestone dependencies, and numbered plan files with no
  registry row (reverse check over `plans/NNN-*.md`).
- Dependency cells may carry explicit external/historical
  prerequisites alongside milestones (`M019 + ADR 003`, `M002
  historical implementation`, `M049-M053`); every embedded `M###`
  token must resolve to a registered milestone.
- `--check` verifies all four generated blocks match and exits 1 on
  drift; it never rewrites. `--write` rewrites the delimited blocks
  deterministically (no timestamps, stable row order). `--fixture`
  runs the inline self-test.
- Registry vocabulary enforced: `active`, `blocked`, `closed`,
  `ready`.

### Regression test (`scripts/tests/test_planning_state.sh`)

POSIX `sh` + stdlib Python, well under one second. Covers: inline
fixture, real-tree `--check`, deliberate-drift negative test on temp
copies only (fresh block verifies, corrupted block fails, missing
block fails), and wiring assertions (`scripts/check.sh` invokes the
test script; the `language-clients` job invokes bare `--check`
independently; no dedicated planning-state CI job exists).

### Generated blocks

Delimited `<!-- BEGIN eggchaos:planning-state -->` … `<!-- END
eggchaos:planning-state -->` blocks now live in AGENTS.md,
`plans/README.md`, `plans/roadmap.md`, and
`architecture/overview.md`. Each renders highest closed milestone,
ready milestone(s), blocked chain, and execution order from the
registry snapshot. At close time the blocks read: highest closed
`M053`, ready `M054`, no blocked chain.

### Reconciled stale wording (no history rewritten)

- `plans/registry.md`: `Last reconciled` line, tranche paragraph
  (`M049 is ready…` → `M049–M052 closed; M053 ready`), and the
  dependency-ready view (`Ready: none / Blocked: none` →
  `Ready: M053 / Blocked: M054`) updated. All 55 milestone rows
  untouched in meaning.
- AGENTS.md planning-state paragraph (`M000–M048 closed, M049
  ready` → `M000–M052 closed, M053 ready`), `check.sh` gate
  description, focused-commands list, and planning-workflow
  instructions (registry authority + regenerate + guard steps).
- `architecture/overview.md`: `M000–M048 closed, M049 ready` →
  `M000–M052 closed, M053 ready` (both occurrences); M019/M041/
  M046–M048 historical authority sentences preserved verbatim in
  meaning.
- `plans/README.md`: tranche paragraph updated; status-rules
  vocabulary corrected to the enforced four states
  (`implemented-awaiting-evidence` / `superseded` were never used
  by any registry row).
- `plans/roadmap.md`: status line and tranche status sentence
  updated; chain order and non-regression constraints unchanged.

No file under `plans/archive/`, `plans/closure/`, or any numbered
plan was modified. No numbered plan `Status:` header was touched
(the registry is the authority).

### CI wiring (no new job)

- `scripts/check.sh` runs `sh scripts/tests/test_planning_state.sh`
  (fixture + `--check` + negative test + wiring assertion).
- `.github/workflows/ci.yml` `language-clients` matrix runs
  `python3 scripts/check_planning_state.py --check` as its own step,
  so removing either the local check or the CI step still trips the
  other. No dedicated job was added, per plan.

## Required verification (results on `9390008`)

| Command | Result |
| --- | --- |
| `python3 scripts/check_planning_state.py --check` | `OK: 55 milestones, 4 target documents match` |
| `python3 scripts/check_planning_state.py --fixture` | `OK: fixture tests pass` |
| `sh scripts/tests/test_planning_state.sh` | `{"planning_state":"pass"}` (includes deliberate-drift negative test) |
| Deliberate real-tree drift probe (corrupt AGENTS.md block → rerun guard → restore) | guard exits 1 on drift, exits 0 after restore |
| `./scripts/check.sh` | exit 0 (provenance guard + planning guard + fmt + clippy + workspace tests + doc) |
| `cargo test --workspace --all-features` | green (inside `check.sh`) |
| Hosted CI run `36460900207` on `9390008` | `success`, 14/14 jobs; planning `--check` green in all 8 `language-clients` jobs |

## Public-surface non-regression statement

M053 changes no production Rust code, no native route, no DTO, no
CLI command, no RNG/fault semantic, no benchmark workload, no
provenance schema, and no compatibility surface. `git diff --name-only`
on the candidate touches only the guard script, its regression test,
`check.sh`, the workflow, the registry, and current-state prose plus
generated blocks in the four summary documents.

## Successor activation

Closing M053 activates M054 (now `ready` in `plans/registry.md`).
M054 begins from this M053 closure candidate as its activation
baseline and will register its own exact implementation candidate
when it closes. M054 activates no automatic successor.
