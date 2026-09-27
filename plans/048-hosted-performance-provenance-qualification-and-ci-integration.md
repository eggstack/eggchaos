# M048 — Hosted Performance-Provenance Qualification and CI Integration

Status: ready  
Depends on: M047 closed at `493fb032c9665494247dcfaad8f4c5dcd7e7b6d1` / closure commit `9b4a2eb548ae9dc40dbfb770236707f553f1d157`  
Role: narrow hosted qualification and CI-integration successor to M047  
Baseline: `9b4a2eb548ae9dc40dbfb770236707f553f1d157`

## Objective

Move M047's performance-artifact provenance contract from local-only
qualification into the repository's ordinary hosted regression surface without
materially increasing CI duration or reopening the completed performance
optimization tranche.

M047 implemented and locally qualified:

- the shared stdlib-only `scripts/bench_provenance.py` authority;
- one provenance schema across stream, stream-probe, and datagram evidence;
- clean authoritative vs dirty exploratory behavior;
- stable dirty-source fingerprints;
- canonical `EGGCHAOS_BENCH_REQUIRE_CLEAN=1` enforcement;
- disposable-repository Git-state regression coverage; and
- shortened artifact-level wrapper qualification.

The remaining gap is explicit in the M047 closure: neither
`scripts/tests/test_bench_provenance.sh` nor
`scripts/tests/test_bench_provenance_artifacts.sh` is exercised by
`.github/workflows/ci.yml` or `scripts/check.sh`.

M048 must add hosted protection for that contract while preserving the current
bounded CI shape and avoiding redundant benchmark execution across the full
OS/language matrix.

## Baseline

Registration baseline:

`9b4a2eb548ae9dc40dbfb770236707f553f1d157`

Current hosted status:

- CI run `36262361337` is green 13/13 on `9b4a2eb`;
- Linux/macOS/Windows Rust `check` jobs are green;
- Linux/macOS language-client jobs are green;
- Linux/macOS native-Python jobs are green;
- the provenance-specific shell regressions are not invoked by hosted CI.

This is a qualification-coverage gap, not a known implementation failure.

## CI-cost principle

M048 must not solve the gap by running the full shortened benchmark artifact
qualification on every OS or every existing matrix entry.

The intended cost model is:

1. **cheap Git-state/provenance contract test** on Linux and macOS;
2. **one shortened artifact-level stream/probe/datagram qualification** on a
   single Linux hosted job;
3. no additional Windows benchmark job unless implementation reveals a
   platform-specific reason;
4. no duplication across Python/Node/native-Python matrices;
5. retain existing 25-minute hard bounds and add a tighter bound for any new
   dedicated job.

If measurements show the proposed integration materially pushes an existing
job toward its timeout, isolate it rather than raising the timeout.

## Scope

### In scope

- hosted execution of `scripts/tests/test_bench_provenance.sh`;
- hosted execution of `scripts/tests/test_bench_provenance_artifacts.sh`;
- CI placement that minimizes redundant setup/build work;
- Linux/macOS portability coverage for the Git-state collector;
- one Linux canonical clean-tree wrapper qualification for stream, probes, and
  datagram evidence;
- validation that CI checkout state is recognized as clean and authoritative;
- validation that retained-evidence mode fails closed when the test
  deliberately dirties a disposable repository;
- workflow timeout/failure behavior;
- lightweight local aggregation in `scripts/check.sh` only if it does not
  accidentally turn the normal developer check into a release-benchmark run;
- documentation of which gate owns provenance qualification;
- exact-head hosted evidence and closure;
- planning/registry/roadmap/AGENTS reconciliation.

### Non-goals

- no production Rust changes;
- no benchmark workload or threshold changes;
- no M042 threshold retuning;
- no rewrite of M046/M047 historical evidence;
- no new provenance schema version;
- no redesign of `bench_provenance.py` unless hosted execution finds a real
  portability defect;
- no full performance baseline rerun;
- no Toxiproxy, SDK, native-Python, fuzz, or release requalification beyond
  the ordinary CI jobs already triggered by the exact candidate;
- no new Windows-specific shell layer solely for symmetry;
- no increase to the existing global CI timeout as the first response to a
  slow test;
- no release/tag/publication action.

## Affected surfaces

Expected:

- `.github/workflows/ci.yml`;
- optionally `scripts/check.sh` for a cheap provenance-only subgate;
- `scripts/tests/test_bench_provenance.sh`;
- `scripts/tests/test_bench_provenance_artifacts.sh` only if hosted
  qualification exposes a portability or hermeticity defect;
- `scripts/bench_provenance.py` / benchmark wrappers only if hosted evidence
  demonstrates a concrete bug;
- `architecture/verification-qualification.md`;
- `architecture/tooling-distribution.md`;
- `qualification/performance/README.md`;
- `plans/registry.md`;
- `plans/README.md`;
- `plans/roadmap.md`;
- `AGENTS.md`;
- new M048 closure evidence.

A clean implementation should primarily be workflow + documentation.

## Required CI architecture

### Tier A — cheap provenance contract on Linux and macOS

Run:

```sh
sh scripts/tests/test_bench_provenance.sh
```

on at least:

- `ubuntu-latest`;
- `macos-latest`.

Preferred placement is early in existing `check` jobs for those two OSes,
before expensive audit/deny steps, so failures are cheap and no new runner is
needed.

Windows may skip this POSIX-shell fixture unless the existing Windows runner
already supports it reliably and adding it is effectively free. M048 must not
claim Windows provenance-script coverage if it is not actually exercised.

This tier protects:

- clean HEAD detection;
- staged/unstaged/untracked source detection;
- ignored/generated exclusions;
- output-file exclusion;
- detached HEAD;
- subdirectory invocation;
- missing-Git failure;
- deterministic/sensitive source fingerprint;
- no Git mutation;
- no path leakage;
- single collector authority.

### Tier B — one hosted artifact-level qualification

Run:

```sh
sh scripts/tests/test_bench_provenance_artifacts.sh
```

once on `ubuntu-latest`.

Do not run it in every `check` matrix row.

Preferred integration options, in order:

1. a dedicated `performance-provenance` Linux job with Rust 1.89 +
   `Swatinem/rust-cache`, timeout 10–15 minutes;
2. the Ubuntu `check` job only if measured wall time stays comfortably below
   the existing 25-minute bound;
3. another existing Linux Rust job only if the dependency/tool setup already
   matches and the ownership remains obvious.

The dedicated job is preferred if adding release-mode benchmark compilation to
`check (ubuntu-latest)` would materially lengthen the critical path.

Tier B must continue to use shortened workloads already frozen by M047. It is
a schema/wrapper qualification, not a throughput benchmark.

### Tier C — ordinary exact-head repository matrix

The M048 implementation candidate must still trigger and pass the ordinary CI
matrix:

- Rust `check`: Ubuntu/macOS/Windows;
- language clients: current Ubuntu/macOS Python/Node matrix;
- native Python: current Ubuntu/macOS matrix.

M048 does not replace those jobs. Its new provenance job/steps are additive.

## Ordered work packages

### WP1 — Measure current CI/job placement constraints

Before editing the workflow:

- record current `ci.yml` job graph and timeout values;
- inspect recent successful run durations for:
  - `check (ubuntu-latest)`;
  - `check (macos-latest)`;
  - `check (windows-latest)`;
- determine whether adding Tier B to Ubuntu `check` would leave reasonable
  timeout headroom;
- record the chosen placement rationale.

Do not increase timeouts merely to avoid creating a dedicated job.

### WP2 — Add Tier A hosted contract coverage

Wire `test_bench_provenance.sh` into Linux and macOS hosted execution.

Requirements:

- run after checkout and Python/Git availability;
- fail the job on nonzero exit;
- produce a visible named step;
- do not conditionally suppress failures;
- avoid duplicate execution in unrelated matrix jobs.

If using the existing `check` matrix, guard the step so Windows is explicitly
skipped rather than relying on shell-resolution accidents.

### WP3 — Add Tier B artifact qualification

Wire `test_bench_provenance_artifacts.sh` into exactly one Linux hosted path.

Requirements:

- Rust 1.89;
- release benchmark build allowed;
- existing shortened workload settings from the script;
- no throughput-value assertions;
- fail on provenance/schema/clean-guard regression;
- hard timeout;
- no artifact upload required unless it materially improves diagnosis;
- no mutation of repository state beyond disposable/temp outputs;
- if the job runs from a clean checkout, the wrapper branch exercised by the
  test must observe `authoritative=true`.

### WP4 — Make the hosted tests hermetic

Hosted CI may expose assumptions hidden on the local macOS developer host.

If failures occur, correct only proven portability/hermeticity issues such as:

- Git default branch/config differences;
- temp-directory symlink behavior;
- missing executable bits;
- Python invocation differences;
- Git safe-directory behavior;
- detached checkout semantics;
- locale/path quoting;
- timing/sample size instability in the shortened artifact test.

Do not weaken assertions to make CI green.

Do not classify a hosted limitation as harmless if it prevents the core
clean-vs-dirty contract from being tested.

### WP5 — Decide local `check.sh` ownership

Evaluate whether the **cheap** Tier A test belongs in `scripts/check.sh`.

Preferred rule:

- add `test_bench_provenance.sh` to `check.sh` if it remains fast,
  deterministic, and requires only Git + stdlib Python available in the normal
  developer environment;
- do **not** add `test_bench_provenance_artifacts.sh` to `check.sh` because
  that would make every local full check compile/run release benchmarks.

If Tier A is not added to `check.sh`, document that CI owns it and provide the
direct command in verification docs.

### WP6 — Add a guard against accidental CI de-integration

The provenance qualification must not silently disappear in a later workflow
cleanup.

Add one low-cost structural assertion, using the repository's existing
conventions, that verifies the workflow or canonical gate still references:

- `test_bench_provenance.sh`;
- `test_bench_provenance_artifacts.sh`.

Acceptable forms:

- a focused shell contract test over `.github/workflows/ci.yml`;
- a documented single qualification script invoked by CI, with the workflow
  checking only that one script;
- another simple deterministic mechanism.

Avoid adding a YAML parser dependency solely for this check.

### WP7 — Documentation and ownership reconciliation

Update:

- `architecture/verification-qualification.md`: hosted ownership and OS
  coverage;
- `architecture/tooling-distribution.md`: provenance test inventory;
- `qualification/performance/README.md`: M048 hosted gate and distinction
  between schema qualification vs performance measurement;
- `AGENTS.md`: exact commands and which gate is expected in CI.

The M046 historical map and M047 schema contract must remain unchanged.

### WP8 — Local pre-push qualification

On one committed M048 candidate, run at minimum:

```sh
sh scripts/tests/test_bench_provenance.sh
sh scripts/tests/test_bench_provenance_artifacts.sh
./scripts/check.sh
```

If Tier A was added to `check.sh`, direct execution is still useful and
should be recorded once for closure clarity.

Confirm:

- no production crate changes unless a hosted-found bug genuinely required
  one;
- no benchmark workload/threshold diff;
- CI YAML validates syntactically;
- the new job/steps have explicit timeout/failure semantics.

### WP9 — Hosted exact-head qualification

Push the exact M048 implementation candidate and wait for hosted CI.

Closure requires:

- Tier A green on Ubuntu;
- Tier A green on macOS;
- Tier B green on Ubuntu;
- ordinary Rust `check` green on Ubuntu/macOS/Windows;
- current language-client matrix green;
- current native-Python matrix green;
- every required job tied to the exact M048 candidate SHA;
- no borrowed green result from M047 or earlier.

Record run ID, exact SHA, job names, and conclusions.

### WP10 — Runtime-cost review

After the first green hosted run, compare job durations with the recent M047
baseline run `36262361337`.

Record:

- added wall-clock duration to affected existing jobs;
- duration of any new dedicated provenance job;
- whether overall critical-path CI duration materially increased.

Acceptance target:

- Tier A should add only a small amount of time to Linux/macOS existing jobs;
- Tier B should not push an existing job near the 25-minute limit;
- a dedicated job should stay within its tighter timeout;
- no matrix multiplication of release benchmark work.

If integration materially worsens CI duration, restructure placement rather
than weakening coverage.

### WP11 — M048 closure

Create:

`plans/closure/M048-hosted-performance-provenance-qualification-and-ci-integration-closure.md`

The closure must record:

- exact implementation/evidence candidate SHA;
- final CI placement;
- Tier A Linux/macOS results;
- Tier B Linux result;
- ordinary matrix result;
- hosted run ID;
- before/after job-duration evidence;
- any portability fixes;
- confirmation that production behavior, benchmark workloads, thresholds,
  schema v1, and historical artifacts are unchanged;
- successor activation.

## Required invariants

- M047 provenance schema v1 is unchanged unless a hosted-only correctness bug
  proves a schema defect;
- dirty runs never become authoritative;
- clean exact-candidate evidence still requires a clean source-relevant tree;
- historical M042–M047 artifacts are not rewritten;
- benchmark case/workload semantics are unchanged;
- M008/M023/M024/M042 thresholds are unchanged;
- no production runtime/API/fault behavior changes;
- CI failures in provenance tests are blocking, not advisory;
- hosted coverage is real and exact-head;
- CI duration remains bounded.

## Acceptance criteria

M048 closes only when:

- `test_bench_provenance.sh` runs and passes in hosted Linux CI;
- the same cheap contract runs and passes in hosted macOS CI;
- `test_bench_provenance_artifacts.sh` runs and passes on one hosted Linux
  job/path;
- hosted clean checkout exercises the authoritative-clean branch;
- dirty/refusal behavior remains covered by the disposable-repo/artifact tests;
- a structural guard protects against accidental future removal of the CI
  integration;
- the ordinary 13-job-equivalent repository matrix remains green, plus any new
  provenance job if added;
- exact candidate/run IDs are recorded;
- no benchmark thresholds/workloads are changed;
- no historical artifacts are rewritten;
- no unjustified timeout increase is introduced;
- job-duration evidence shows the integration remains operationally bounded;
- registry/README/roadmap/AGENTS agree on closure.

## Stop / escalation conditions

Do not close M048 if:

- provenance tests remain local-only;
- only Linux is tested for the cheap Git-state collector with no documented
  macOS coverage;
- artifact qualification is duplicated across the full OS matrix;
- the integration raises an existing timeout instead of addressing poor job
  placement;
- a provenance step is marked `continue-on-error`;
- CI skips the test based on an unrelated matrix variable;
- the workflow runs the test but closure evidence is borrowed from another SHA;
- hosted failures are "fixed" by weakening clean/dirty assertions;
- the artifact test becomes a throughput benchmark with unstable numeric
  expectations;
- CI integration changes production behavior or performance thresholds;
- added CI cost is substantial and no placement optimization is attempted.

If hosted execution exposes a substantive M047 implementation defect, M048 may
include the minimal corrective needed to make the qualified contract true, but
the closure must identify it explicitly. A broader provenance redesign requires
a separate plan.

## Follow-on activation

M048 activates no automatic successor.

After clean closure:

- M046 remains the authority for historical M042–M045 provenance;
- M047 remains the provenance schema/tooling authority;
- M048 becomes the hosted qualification/CI-ownership authority for that
  tooling.

Further performance work should begin from new measured findings, not from
another provenance corrective unless hosted regression evidence identifies one.
