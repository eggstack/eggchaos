# M061 — M060 Closure-Evidence and Plan-Status Consistency Corrective — Closure

Status: `closed` on exact implementation/closure candidate
`97913754c125864dbbccb4a269e5ee51102c3de8` (`9791375`, "M061 WP1-WP5:
reconcile M060 closure evidence + enforce registry/plan-header status").
Registration baseline was
`3ebee30cd33ea141a680acd5e6569a453fcdf124` (`3ebee30`).
Hosted ordinary CI run `37366364200`, `head_sha`
`97913754c125864dbbccb4a269e5ee51102c3de8`, conclusion `success`, 15/15 jobs.

## Objective verdict

M061 closes two defects in the M060 **closure layer**, not in the M060
implementation tranche:

1. M060's hosted-CI gate was closed with an unfilled `<HEAD>` placeholder
   even though no GitHub Actions run exists for the exact M060
   implementation candidate `e897cc4`.
2. M060's plan header still read `Status: ready` against a `closed`
   registry row, and the same divergence class existed across **24**
   registered numbered plans.

M060 remains a historical closed milestone and its implementation evidence
is preserved unchanged. M061 is the additive exact-head hosted
reconciliation authority for the planning/evidence gap, and it makes
registry ↔ plan-header status agreement a mechanical invariant so the gap
cannot reopen silently. No production, dependency, package, API, or
release-workflow behavior changed.

## Evidence lineage (WP1)

The lineage is recorded here verbatim so no reviewer can mistake a
descendant run for exact-SHA qualification.

| SHA | Role | Hosted CI |
| --- | --- | --- |
| `e897cc4767f0bb2c10feaae46232ab2e52666d90` | M060 implementation / local-qualification candidate | **None. `gh run list --commit e897cc4` returns no run.** |
| `1d3bb825cb43872891b0fcedb81d905218dfaffd` | M060 registry-row + closure-record commit (descendant of the candidate) | None on this SHA |
| `474220662ed1daff406b6b2639884580768ea9c3` | Later planning/closure descendant | Run `37028684309`, `success`, 15/15 |
| `3ebee30cd33ea141a680acd5e6569a453fcdf124` | Post-M060 documentation audit; M061 registration baseline | Run `37266737795`, `success`, 15/15 |
| `97913754c125864dbbccb4a269e5ee51102c3de8` | **M061 exact candidate (this closure)** | **Run `37366364200`, `success`, 15/15** |

Runs `37028684309` and `37266737795` are **descendant evidence only**.
Neither executed on `e897cc4`, and neither is described anywhere as
qualifying it. M061 does not rewrite Git history and does not reopen the
M060 implementation tranche.

## WP1 — Freeze the evidence lineage (done)

Verified at registration baseline `3ebee30` and re-verified at closure:

- M060 registry row = `closed`; M060 plan header was `ready` (the defect).
- M060 closure record named `e897cc4` as its exact closure candidate and
  promised a hosted run ID to be "appended below".
- No Actions run exists for `e897cc4` (empty `gh run list --commit`).
- Run `37028684309` is `success` 15/15 on `head_sha` `474220662ed1…`.
- Run `37266737795` is `success` 15/15 on `head_sha` `3ebee30cd33e…`.
- M058/M059 closure records verified unchanged (still byte-identical at
  closure; see "No production/release delta" below).

## WP2 — Reconcile M060 status metadata additively (done)

- `plans/060-post-v0-2-0-…-corrective.md` header now reads
  `Status: closed (…)` with its closure-record pointer and an explicit
  pointer to the M061 hosted-evidence reconciliation. The historical
  work-package body is untouched.
- The M060 closure record gained a clearly labelled
  **"M061 reconciliation and erratum (additive; supersedes no prior
  statement)"** section containing Erratum 1 (no hosted run on
  `e897cc4`), the corrected lineage table, and Erratum 2 (the
  plan-header divergence was systemic, not M060-only). The original WP6
  hosted-CI paragraph and its `<HEAD>` placeholder are preserved
  verbatim above it; the M060 final verdict gained one sentence pointing
  at Erratum 1.
- Net effect on that file: 72 insertions, 1 deletion, where the single
  deletion is a re-wrapped line of the preserved final-verdict sentence.

## WP2a — Plan-status census and bounded normalization (done)

WP3's audit step required classifying every registered numbered plan
before enabling the agreement rule on the live tree.

Result: **24 of 62** registered plans had a plan-header status that
disagreed with their registry row. Every one had registry status `closed`
and a header still reading `ready` (M026, M035, M036, M041, M042, M046,
M047, M048, M049, M055, M056, M057, M060 — 13) or `blocked` (M037, M038,
M039, M043, M044, M045, M050, M051, M052, M053, M054 — 11). The remaining
38 already agreed.

Disposition: **bounded normalization**, not the WP3 stop condition. All 24
were parseable (exactly one column-0 `Status:` line each, inside the header
block) and every one had an existing closure record, so the fix was a
mechanical one-line-per-file correction derived from the sole authority —
not a broad historical rewrite. The M024/M025/M058 single-line suffix form
was reused, with each plan's as-written status preserved in the suffix so
the correction is transparent rather than a silent retroactive relabel:

```text
Status: closed (header reconciled to the registry status by M061;
as written at registration: `ready`; evidence in `plans/closure/M0NN-…-closure.md`)
```

`git diff 3ebee30..9791375 -- plans/` proves the discipline: **24
insertions, 24 deletions** across the 24 plan files. No plan body,
objective, work package, acceptance criterion, or historical statement was
rewritten.

## WP3 — Registry ↔ plan-header status consistency is mechanical (done)

`scripts/check_planning_state.py` (stdlib only) now:

- reads each registered numbered plan and searches only the **top-level
  header block** (everything before the first `## ` heading) for a
  `Status:` declaration, so body prose can neither satisfy nor duplicate it;
- parses the leading status token, accepting an explanatory suffix after a
  valid token (`Status: closed (candidate \`x\`; evidence in …)`) and inline
  emphasis (`Status: **ready**`) — both forms already present in the live
  tree;
- normalizes **only** the documented vocabulary
  `active | blocked | closed | ready`;
- requires the parsed token to equal the registry row status;
- fails closed with four named, actionable error kinds rather than
  skipping: `plan-status-missing`, `plan-status-duplicate`,
  `plan-status-unparseable`, `plan-status-mismatch` (the mismatch message
  names the milestone, both statuses, and the path);
- keeps the **registry canonical**. The check asserts that plan metadata
  *follows* the registry; it never infers a registry status from a plan
  file. No second planning-status checker was added.

## WP4 — Regression coverage (done)

`--fixture` (in `scripts/check_planning_state.py`) gained a
registry ↔ plan-header agreement matrix: suffix acceptance, emphasis
acceptance, registry-`closed`/plan-`ready` mismatch, missing declaration,
duplicate declaration, out-of-vocabulary token, and proof that a body-only
`Status:` line reads as missing. Existing malformed-row, dependency,
ordering, uniqueness, determinism, and stale/missing block cases are
unchanged.

`scripts/tests/test_planning_state.sh` gained section 3b, which copies the
live plans directory into a temp tree (the real checkout is never
modified), asserts the live tree is clean, then drives one real plan
header through registry-`closed`/plan-`ready`, registry-`closed`/
plan-`blocked`, missing, duplicate, and unparseable declarations —
asserting both the named error kind and that the guard returns to clean
once the header is restored.

Dual wiring is intact and unchanged:

- `scripts/check.sh` via `sh scripts/tests/test_planning_state.sh`;
- bare `python3 scripts/check_planning_state.py --check` as its own step in
  the `language-clients` CI job.

No dedicated planning-state CI job was added (the existing structural
assertion against one remains green).

## WP5 — Registry and projections (done)

- M060's registry row keeps its implementation candidate `e897cc4` and its
  closure note, and gained an explicit **M061 reconciliation** clause
  stating that no Actions run exists for that SHA, that `37028684309` and
  `37266737795` are descendant evidence only, and that M061 supplies the
  additive exact-head hosted reconciliation.
- An M061 narrative paragraph was added to the registry prose, and the
  "Dependency-ready view" / release-evidence-order lines updated.
- The four canonical planning-state blocks (`AGENTS.md`,
  `plans/README.md`, `plans/roadmap.md`, `architecture/overview.md`) were
  regenerated with `python3 scripts/check_planning_state.py --write`; no
  hand edits between the markers.
- `architecture/tooling-distribution.md` rows for
  `scripts/check_planning_state.py` and
  `scripts/tests/test_planning_state.sh` were updated so the script catalog
  stays source-accurate about the extended guard. This is the only file
  changed outside the plan's nominal affected-surface list; the recorded
  reason is that leaving the catalog stale about a guard this milestone
  changed would recreate exactly the drift class the milestone exists to
  remove. The eight-guard `check.sh` inventory recorded at `3ebee30` is
  otherwise untouched.
- `AGENTS.md` and `plans/README.md` gained one clause each recording that a
  numbered plan's `Status:` header is a checked projection, that the
  registry must be changed first, and the four new error kinds.

## WP6 — Exact-head qualification and closure (done)

### Local gate on the exact candidate `9791375`

| Command | Result |
| --- | --- |
| `python3 scripts/check_planning_state.py --check` | `OK: 62 milestones, 4 target documents match` |
| `python3 scripts/check_planning_state.py --fixture` | `OK: fixture tests pass` |
| `sh scripts/tests/test_planning_state.sh` | `{"planning_state":"pass"}` (fixture + real tree + block drift + plan-header agreement matrix + wiring) |
| `python3 scripts/check_release_state_docs.py --check` | `{"release_state_docs":"pass"}` |
| `sh scripts/tests/test_release_state_docs.sh` | `{"release_state_docs":"pass"}` (fixture + real tree + 6 deliberate-drift negatives + wiring) |
| `python3 scripts/check_version_coherence.py --check` | `{"version_coherence": "pass", "workspace_version": "0.2.0", "workspace_crates": 8}` |
| `sh scripts/tests/test_release_tag_version.sh` | `{"release_tag_version_guard":"pass","release_workflow_gate":"pass"}` |
| `./scripts/check.sh` | exit 0 (fmt / clippy / test / doc + the cheap guard suite) |
| `./scripts/check_openapi.sh` | `{"openapi":"pass","paths":21,"operations":36}` |
| `git diff --check` | clean |

### No production / release behavior delta

`git diff --name-only 3ebee30..9791375 --` over
`crates/**`, `Cargo.toml`, `Cargo.lock`, `bindings/**`, `api/**`,
`rust-toolchain.toml`, `benchmarks/**`, `fuzz/**`, `.github/workflows/**`,
`docs/**`, `SECURITY.md`, and the M059/M060 guard scripts returns
**nothing**. No `.github/workflows/` file changed at all, so no trigger,
permission, action pin, artifact, attestation, checksum, or `release-contract`
DAG edge moved. Workspace version stays `0.2.0`, MSRV stays 1.89, OpenAPI
stays 21 paths / 36 operations, and the generated SDK contract tables are
untouched.

Changed-file inventory for the corrective delta `d3e9675..9791375`
(31 files, +431 −41):

- `AGENTS.md`, `plans/README.md`, `plans/registry.md`,
  `architecture/tooling-distribution.md` — planning/prose authority and
  the generated-block carriers;
- `plans/061-…-corrective.md` — plan header kept at `ready` for the
  candidate commit (WP5 prescribes `ready` while implementing);
- `plans/closure/M060-…-closure.md` — the additive erratum section;
- `plans/NNN-…md` × 24 — one status line each (WP2a);
- `scripts/check_planning_state.py`, `scripts/tests/test_planning_state.sh`
  — the guard and its regression test.

The M061 registration delta `3ebee30..d3e9675` (6 files, +401 −10) is the
plan file plus the same four generated-block carriers and the registry row.

**M058 and M059 closure records are byte-identical** to their pre-M061
forms (`git diff --stat 3ebee30..9791375 -- plans/closure/M058-… plans/closure/M059-…`
is empty). `plans/closure/` shows exactly one changed file repo-wide: the
M060 record.

### Hosted CI on the exact candidate

- Run: **`37366364200`** — <https://github.com/eggstack/eggchaos/actions/runs/37366364200>
- Workflow: `CI`; event: `push`
- Exact `head_sha`: `97913754c125864dbbccb4a269e5ee51102c3de8`
  (byte-identical to the local candidate; asserted programmatically)
- Conclusion: **`success`** — 15/15 jobs

| Job | Conclusion |
| --- | --- |
| `api-gate` | success |
| `check (ubuntu-latest)` | success |
| `check (macos-latest)` | success |
| `check (windows-latest)` | success |
| `performance-provenance` | success |
| `language-clients (ubuntu-latest, 3.11, 20)` | success |
| `language-clients (ubuntu-latest, 3.11, 22)` | success |
| `language-clients (ubuntu-latest, 3.12, 20)` | success |
| `language-clients (ubuntu-latest, 3.12, 22)` | success |
| `language-clients (macos-latest, 3.11, 20)` | success |
| `language-clients (macos-latest, 3.11, 22)` | success |
| `language-clients (macos-latest, 3.12, 20)` | success |
| `language-clients (macos-latest, 3.12, 22)` | success |
| `python-native (ubuntu-latest, 3.12)` | success |
| `python-native (macos-latest, 3.12)` | success |

The eight `language-clients` legs each execute the bare
`python3 scripts/check_planning_state.py --check` independently of
`scripts/check.sh`, so the new registry ↔ plan-header rule is exercised on
the hosted matrix on the exact candidate — the requirement that a
single local wiring could not have proven on its own.

#### Infrastructure note (recorded for accuracy, not as a code failure)

The first push of `9791375` met a saturated GitHub hosted-runner queue.
Attempt 1 finished with 11 jobs green and four jobs
(`check (macos-latest)`, `check (ubuntu-latest)`,
`python-native (ubuntu-latest, 3.12)`, `python-native (macos-latest, 3.12)`)
`cancelled` with **zero steps executed** — they were never dispatched and
were killed by the `check` job's 25-minute bound, not by any test
assertion. Those four legs were re-run on the **same** `head_sha` via
`gh run rerun --failed`; every one then passed. Run `37366364200` is a
single run ID on a single exact SHA whose final conclusion is `success`
15/15. No ancestor or descendant SHA is used as M061 closure evidence.

## Invariants preserved

- Published `v0.2.0` tag and its `b6a277d` target; all eight published Rust
  crate `0.2.0` artifacts.
- Workspace version `0.2.0`, MSRV 1.89, `unsafe_code = "forbid"`.
- All public Rust APIs, crate features, native `/v1` contract (21 paths /
  36 operations), CLI inventory, SDK operation tables, Python-native
  visible API, RNG v1, scenario V1/V2 semantics, stream/datagram fault
  semantics, and Toxiproxy compatibility profiles.
- M059 dependency/action/security/provenance hardening.
- The release workflow contract, `release-contract` DAG, five binary
  targets, SHA-256 sidecars, and Sigstore attestations.
- The M060 release-state documentation guard and its six drift classes.
- The `3ebee30` architecture/tooling deep-dive corrections, including the
  eight-guard `check.sh` inventory.
- `plans/registry.md` as the **sole** hand-maintained status authority.
- Historical closure lineage: every correction in this milestone is
  additive, labelled, and explicit. Nothing was relabeled retroactively
  and no historical fact was deleted or obscured.

## Limitations

- The agreement rule is deliberately narrow: it compares the registry row
  to the plan's own top-level `Status:` token. It does not lint prose, does
  not check ordering of *other* header fields, and does not validate
  closure-record contents.
- A plan can still satisfy the guard with an explanatory suffix, so the
  suffix text itself is unvalidated prose. The guard constrains the status
  token, which is the value that can diverge silently.
- Hosted evidence is ordinary CI only. No release `workflow_dispatch` run
  was performed, because M061 changes no release-workflow behavior; this
  is the plan's declared outcome, not a gap.
- The three re-run legs in the hosted run required `gh run rerun --failed`
  because of a transient hosted-runner queue condition. The re-run reused
  the same run ID and the same `head_sha`, so exact-head provenance is
  intact, but the wall-clock duration of that run should not be read as a
  performance measurement.

## Final verdict

M061 is **closed** on exact candidate `9791375` with green hosted run
`37366364200` (`success`, 15/15, exact `head_sha` match).

The M060 documentation cleanup line is now fully reconciled: M060's
implementation evidence is preserved and explicitly labeled, its
unfulfilled hosted-CI obligation is recorded as an erratum rather than
papered over, and exact-head hosted evidence is restored for the
planning/evidence layer. `plans/registry.md` remains the sole status
authority and the numbered-plan `Status:` headers are now mechanically
constrained to follow it.

`v0.2.0` remains the published Eggchaos release; M058 remains the
published-release authority; M059 remains the pre-publication hardening
authority. M061 activates **no automatic successor**. Any patch release,
new feature tranche, dependency migration, language-registry publication,
or runtime work requires a new evidence-backed milestone.