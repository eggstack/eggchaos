# M048 — Hosted Performance-Provenance Qualification and CI Integration Closure

Status: closed

Exact M048 implementation/evidence candidate:
`ab61ac7809b9260e44a827567065e1479445f2f7`

Hosted run: [36331806587](https://github.com/eggstack/eggchaos/actions/runs/36331806587)
on candidate `ab61ac7`, conclusion `success` — 14/14 jobs green (3
`check` + 1 `performance-provenance` + 8 `language-clients` + 2
`python-native`; see § "Hosted exact-head evidence").

Closure date: 2026-09-27

Depends on: M047 closed on exact candidate `493fb03` (closure commit
`9b4a2eb548ae9dc40dbfb770236707f553f1d157`).

## Outcome

M048 moves the two M047 provenance regressions from local-only
qualification into ordinary hosted protection at bounded CI cost. It
adds:

- the cheap M047 Git-state contract on `ubuntu-latest` and
  `macos-latest` (Tier A), gated `runner.os != 'Windows'` because the
  script is POSIX `sh`;
- one shortened M047 artifact-level stream/probe/datagram qualification
  on `ubuntu-latest` only (Tier B), wired into a dedicated
  `performance-provenance` Linux CI job;
- one structural guard
  (`scripts/tests/test_ci_provenance_integration.sh`) that runs in the
  `language-clients` matrix so removing either of the Tier A/B jobs
  still trips an independent check.

No production runtime, API, fault semantics, RNG, benchmark workload,
threshold, schema v1, or historical-artifact change. M047 schema v1
and M046 historical provenance map are preserved verbatim. The
ordinary three-platform CI matrix (3 `check` + 8 `language-clients` +
2 `python-native`) is unchanged except for the additive Tier A step
inside `check` (Linux + macOS only) and the additive
`test_ci_provenance_integration.sh` step inside `language-clients`.

## What landed on `ab61ac7`

`git show --stat ab61ac7`:

```text
.github/workflows/ci.yml                        | 30 ++++++++++
AGENTS.md                                       |  5 +-
architecture/tooling-distribution.md            | 32 ++++++++--
architecture/verification-qualification.md      | 41 +++++++++++--
qualification/performance/README.md             | 47 +++++++++++++++
scripts/check.sh                                |  8 +++
scripts/tests/test_ci_provenance_integration.sh | 79 +++++++++++++++++++++++++
7 files changed, 231 insertions(+), 11 deletions(-)
```

- `.github/workflows/ci.yml`
  - `check` job: new `M048 Tier A: bench provenance Git-state contract`
    step (`if: runner.os != 'Windows'`,
    `run: sh scripts/tests/test_bench_provenance.sh`) immediately after
    the `cargo install cargo-deny` step so failures surface in seconds
    before `cargo fmt`/`clippy`/`test`/etc.
  - new `performance-provenance` job
    (`timeout-minutes: 12`, `ubuntu-latest` only, `Swatinem/rust-cache`):
    one step,
    `M048 Tier B: bench provenance artifact qualification` →
    `sh scripts/tests/test_bench_provenance_artifacts.sh`.
  - `language-clients` job: new step
    `sh scripts/tests/test_ci_provenance_integration.sh` after the
    existing `test_fetch_toxiproxy_post_v2_12_contract.sh` step and
    before the SDK/native checks. This intentionally lives in
    `language-clients`, not in `check` or `performance-provenance`, so
    de-integrating either Tier A or Tier B still trips an independent
    structural check.
- `scripts/check.sh`: prepends `sh scripts/tests/test_bench_provenance.sh`
  before `cargo fmt --check` so the local developer gate exercises the
  same cheap contract.
- `scripts/tests/test_ci_provenance_integration.sh`: new structural
  guard (`set -eu`, POSIX `sh`, no YAML parser dependency). Asserts
  `.github/workflows/ci.yml` still references both
  `test_bench_provenance.sh` and `test_bench_provenance_artifacts.sh`,
  that Tier A is guarded with a `runner.os` condition (Windows
  intentionally has no POSIX-shell coverage), that Tier B lives in a
  dedicated job outside the `check` matrix, and that the dedicated job
  is `ubuntu-latest` with its own `timeout-minutes`. Prints
  `{"ci_provenance_integration":"pass"}` on success.
- `architecture/verification-qualification.md`: §5 expanded with the
  M048 hosted gate description and the `M048` bullet point. §6 retains
  the canonical commands including `sh
  scripts/tests/test_bench_provenance.sh` and `sh
  scripts/tests/test_bench_provenance_artifacts.sh`.
- `architecture/tooling-distribution.md`: §1 script catalog gains a row
  for the new structural guard and the existing two provenance-test
  rows are updated to call out their CI placement. §2 describes the
  new `performance-provenance` job and the Tier A wiring inside
  `check`.
- `qualification/performance/README.md`: new "M048 hosted
  qualification and CI ownership" section explaining Tier A/Tier
  B/structural guard/local check.sh ownership, plus a local-run block.
- `AGENTS.md`: "Commands" section now names Tier A inside
  `./scripts/check.sh` (full gate), the dedicated `performance-provenance`
  CI job, the `language-clients` structural guard, and a Tier A / Tier
  B / guard entry in the focused-runs block.

No `crates/`, `fuzz/`, `benchmarks/src/`, `qualification/performance/*.json`,
`plans/reference/`, or ADR change.

## Local pre-push qualification (run on the candidate `ab61ac7`)

- `sh scripts/tests/test_bench_provenance.sh` → `{"bench_provenance":"pass"}`
  in ~7s (clean tree on `ab61ac7`; runs in seconds; depends only on
  Git + stdlib Python).
- `sh scripts/tests/test_bench_provenance_artifacts.sh` →
  `{"bench_provenance_artifacts":"pass"}` in ~60s on the clean
  candidate (the test asserts `authoritative=true` for the clean tree
  and exercises the `EGGCHAOS_BENCH_REQUIRE_CLEAN=1` canonical guard).
  On the dirty pre-commit tree this same script prints
  `{"bench_provenance_artifacts":"pass"}` again, but with
  `authoritative=false` and the require-clean refusal branch
  (asserted by the test).
- `sh scripts/tests/test_ci_provenance_integration.sh` →
  `{"ci_provenance_integration":"pass"}` (static pattern checks only).
- `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`
  → no error (the YAML has been validated).

## Hosted exact-head evidence (run 36331806587 on `ab61ac7`)

Per-job start/finish times (UTC) and conclusion:

| Job | Started | Finished | Duration | Conclusion |
| --- | --- | --- | --- | --- |
| `performance-provenance` (new Tier B) | 16:06:01 | 16:08:16 | 2m 15s | success |
| `check (ubuntu-latest)` (Tier A added) | 16:11:17 | 16:16:08 | 4m 51s | success |
| `check (macos-latest)` (Tier A added) | 16:07:44 | 16:11:55 | 4m 11s | success |
| `check (windows-latest)` (Tier A gated off) | 16:07:58 | 16:12:31 | 4m 33s | success |
| `language-clients (ubuntu-latest, 3.11, 20)` | 16:06:28 | 16:07:38 | 1m 10s | success |
| `language-clients (ubuntu-latest, 3.11, 22)` | 16:07:02 | 16:07:57 | 0m 55s | success |
| `language-clients (ubuntu-latest, 3.12, 20)` | 16:10:24 | 16:11:32 | 1m 8s | success |
| `language-clients (ubuntu-latest, 3.12, 22)` | 16:10:41 | 16:11:37 | 0m 56s | success |
| `language-clients (macos-latest, 3.11, 20)` | 16:11:27 | 16:12:35 | 1m 8s | success |
| `language-clients (macos-latest, 3.11, 22)` | 16:08:45 | 16:09:43 | 0m 58s | success |
| `language-clients (macos-latest, 3.12, 20)` | 16:10:05 | 16:11:28 | 1m 23s | success |
| `language-clients (macos-latest, 3.12, 22)` | 16:10:37 | 16:11:58 | 1m 21s | success |
| `python-native (ubuntu-latest, 3.12)` | 16:09:45 | 16:11:08 | 1m 23s | success |
| `python-native (macos-latest, 3.12)` | 16:06:27 | 16:08:09 | 1m 42s | success |

14/14 green. The Tier A step is visible by name in the
`check (ubuntu-latest)` and `check (macos-latest)` logs as
"M048 Tier A: bench provenance Git-state contract"; the Tier B step
is visible by name in the `performance-provenance` log as "M048
Tier B: bench provenance artifact qualification"; the structural
guard is visible by name in every `language-clients` log as
"Run sh scripts/tests/test_ci_provenance_integration.sh".

## Runtime-cost review (vs M047 baseline run `36219464594`)

| Job | M047 baseline (run 36219464594) | M048 (run 36331806587) | Delta |
| --- | --- | --- | --- |
| `check (ubuntu-latest)` | 4m 52s | 4m 51s | −1s |
| `check (macos-latest)` | 4m 12s | 4m 11s | −1s |
| `check (windows-latest)` | 4m 23s | 4m 33s | +10s (Windows skips Tier A; within noise) |
| New `performance-provenance` (Linux) | n/a | 2m 15s | +2m 15s |

Acceptance targets from M048 §10:

- **Tier A adds only a small amount of time to Linux/macOS existing
  jobs.** Both `check (ubuntu-latest)` and `check (macos-latest)`
  finished within ~1 s of the M047 baseline; the Tier A step is
  scheduled before the existing `cargo fmt` step and gates on a
  per-step cached checkout, so it ran in seconds (sub-second typical
  on warm caches, ≤10 s observed). ✅
- **Tier B does not push an existing job near the 25-minute limit.**
  Tier B lives in a dedicated `performance-provenance` job with its
  own 12-minute timeout; no existing job absorbs the work. ✅
- **A dedicated job stays within its tighter timeout.** 2m 15s of the
  12-minute bound; ~9m 45s of headroom. ✅
- **No matrix multiplication of release benchmark work.** Tier B
  appears exactly once across the entire CI graph (Ubuntu only); the
  `language-clients` and `python-native` matrices are unchanged. ✅

The Tier B cold build will dominate the first time the cache key for
the `benchmarks` workspace misses on a fresh branch; that path is
bounded by the 12-minute timeout (release-mode compilation of three
benchmark binaries + the two wrapper runs + the bypass + the
authoritative-guard pass). On warm caches the job finished in 2m 15s
on the candidate.

## Acceptance criteria — all met

- [x] `test_bench_provenance.sh` runs and passes in hosted Linux CI
  (`check (ubuntu-latest)` log line "M048 Tier A: bench provenance
  Git-state contract" succeeded in run 36331806587).
- [x] the same cheap contract runs and passes in hosted macOS CI
  (`check (macos-latest)` log line "M048 Tier A: bench provenance
  Git-state contract" succeeded in run 36331806587).
- [x] `test_bench_provenance_artifacts.sh` runs and passes on one
  hosted Linux job/path (`performance-provenance` log line "M048 Tier
  B: bench provenance artifact qualification" succeeded in run
  36331806587; conclusion `success`).
- [x] hosted clean checkout exercises the authoritative-clean branch
  (the test asserts `authoritative=true` for a clean tree; the run
  passes with no warning, so the Tier B canonical path took the
  clean branch).
- [x] dirty/refusal behavior remains covered by the
  disposable-repo/artifact tests (Tier A matrix includes
  tracked-unstaged, staged, untracked source, and `--exclude`-clean
  branches; Tier B includes the dirty-tree refuse branch when run
  pre-commit).
- [x] a structural guard protects against accidental future removal
  of the CI integration
  (`scripts/tests/test_ci_provenance_integration.sh` runs in
  `language-clients` and verifies both `test_bench_provenance.sh`
  and `test_bench_provenance_artifacts.sh` are still referenced,
  that Tier A is guarded, that Tier B is in a dedicated job, and
  that the dedicated job is Linux-only with its own timeout).
- [x] the ordinary 13-job-equivalent repository matrix remains green
  (3 `check` + 8 `language-clients` + 2 `python-native` all
  `success`), plus the new `performance-provenance` job.
- [x] exact candidate/run IDs are recorded above
  (`ab61ac7` / `36331806587`).
- [x] no benchmark thresholds/workloads are changed.
- [x] no historical artifacts are rewritten
  (`git diff 9b4a2eb..ab61ac7 -- qualification/performance/*.json` is
  empty).
- [x] no unjustified timeout increase is introduced (the existing
  25-minute `check` / `language-clients` / `python-native` bounds
  are unchanged; only the new `performance-provenance` job has its
  own tighter 12-minute bound).
- [x] job-duration evidence shows the integration remains
  operationally bounded (per-job deltas vs M047 baseline are within
  ±10 s on the existing jobs; the new job uses ~2m 15s of a
  12-minute budget).
- [x] registry/README/roadmap/AGENTS agree on closure (see WP11
  reconciliation below).

## Invariance proof

- `git diff 9b4a2eb..ab61ac7 -- crates/ fuzz/ benchmarks/src/ qualification/performance/*.json plans/reference/ plans/adrs/` is empty.
- M047 schema v1 in `qualification/performance/README.md` is unchanged.
- M046 historical provenance map in
  `qualification/performance/README.md` is unchanged.
- The M023/M024 datagram thresholds in `scripts/benchmark_datagram.sh`
  are unchanged (`0.45 / 2.5 / 0.7 / 1.6 / 0.7`).
- The M008 stream `empty_plan ≥ 70%` budget in
  `architecture/verification-qualification.md` is unchanged.
- M042 classifications and per-target thresholds are unchanged.
- Production runtime, public API, fault semantics, RNG, fixed-target
  behavior, byte-conservation invariants, half-close/shutdown,
  bounded-buffer/backpressure, CLI/SDK contract, OpenAPI drift, and
  Toxiproxy differential corpus (50/50) are unchanged.

## Portability / hermeticity

- The Tier A and structural-guard scripts are POSIX `sh` with
  `set -eu` and depend only on Git + stdlib Python, both of which are
  pre-installed on `ubuntu-latest` and `macos-latest` GitHub-hosted
  runners. Windows is intentionally skipped for both scripts (M048
  does not claim Windows POSIX-shell coverage).
- The Tier B script needs `python3` + the pinned Rust 1.89 toolchain
  + `cargo run --release`. It uses the existing shortened workload
  knobs (`EGGCHAOS_BENCH_BYTES=131072 EGGCHAOS_BENCH_ROUNDS=1` for
  stream, `EGGCHAOS_BENCH_BYTES=65536 EGGCHAOS_BENCH_ROUNDS=1
  EGGCHAOS_BENCH_CASE=eggchaos_live_empty_plan` for bypass,
  `EGGCHAOS_DATAGRAM_BENCH_DATAGRAMS=50
  EGGCHAOS_DATAGRAM_BENCH_ROUNDS=1` for datagram). All knobs match
  M047's frozen shortened workloads.
- No hosted hermeticity fix was required — the M047 fixtures already
  honored macOS symlink resolution (`/var -> /private/var`), Git
  default branch differences, locale, and detached-HEAD semantics;
  the GitHub Actions `actions/checkout@v4` + `dtolnay/rust-toolchain`
  + `Swatinem/rust-cache@v2` setup is identical to the existing CI
  and was enough to run both tiers cold on the candidate.

## Successor activation and planning reconciliation

M048 activates no automatic successor. M046 remains the authority
for historical M042–M045 evidence provenance; M047 remains the
provenance schema/tooling authority for newly generated artifacts;
M048 becomes the hosted qualification/CI-ownership authority for
that tooling (the scripts, the schema, and the policy are unchanged
— only the CI placement and the structural guard are new).

Further performance work must use the M047 canonical clean-evidence
path from its first baseline onward, and any new provenance-related
concern must add to this contract rather than bypass it
(`scripts/tests/test_ci_provenance_integration.sh` exists exactly
to prevent silent de-integration of that contract from CI).

After closure:

- `plans/registry.md` row M048 transitions `ready` → `closed` with
  exact candidate `ab61ac7809b9260e44a827567065e1479445f2f7` and
  hosted run `36331806587` (14/14 jobs green).
- `plans/roadmap.md` §11M and `plans/README.md` "current execution
  order" gain a closed M048 with hosted run ID.
- `AGENTS.md` "Planning state" advances M048 from `ready` to the
  closed M048 entry.
- `architecture/verification-qualification.md`, `architecture/tooling-distribution.md`,
  and `qualification/performance/README.md` carry the M048 hosted
  gate description (already on the candidate).

Nothing is blocked on M048. The post-M041 performance tranche
remains closed at M047 (M048 is its sole hosted qualification
successor). After M048 closure the registry reads:

`M042 (closed) -> M043 (closed) -> M044 (closed) -> M045 (closed)
-> M046 (closed historical provenance) -> M047 (closed
provenance schema/tooling) -> M048 (closed hosted qualification/CI
integration)`.

Future performance work begins from new measured findings, not from
another provenance corrective unless hosted regression evidence
identifies one.
