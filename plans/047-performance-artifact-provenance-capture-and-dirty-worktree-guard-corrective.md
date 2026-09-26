# M047 — Performance Artifact Provenance Capture and Dirty-Worktree Guard Corrective

Status: ready  
Depends on: M046 closed at `f569a0a36f99c103ce62852a3aa89584005442d9` / closure commit `33b5c087723da5a1eafd9b9d2f17875c5e31442c`  
Role: root-cause corrective for future performance evidence provenance  
Baseline: `33b5c087723da5a1eafd9b9d2f17875c5e31442c`

## Objective

Prevent a recurrence of the evidence-lineage ambiguity corrected by M046.

M046 reconstructed the historical M042–M045 record correctly, but it was
deliberately documentation/evidence-only. The underlying artifact-generation
mechanism still has two weaknesses:

1. the datagram wrapper stamps only `git rev-parse HEAD`, so a dirty
   worktree produces an artifact whose `candidate_sha` names the base commit
   rather than the actual source tree that ran; and
2. the stream and stream-probe JSON emitted by
   `eggchaos-benchmarks` carry no Git revision/worktree provenance at all.

M047 must make new retained performance evidence self-describing and must
separate **authoritative clean-tree evidence** from **exploratory dirty-tree
measurement**. It must do this without changing benchmark workloads,
production behavior, existing performance budgets, or historical artifacts.

## Baseline and root cause

Registration baseline:

`33b5c087723da5a1eafd9b9d2f17875c5e31442c`

### Datagram provenance today

`scripts/benchmark_datagram.sh` runs the benchmark into JSON and then
post-processes it with Python:

```python
report["rustc"] = subprocess.check_output(["rustc", "--version"], text=True).strip()
report["candidate_sha"] = subprocess.check_output(
    ["git", "rev-parse", "HEAD"], text=True
).strip()
```

That value is correct only as a base HEAD. It says nothing about whether
tracked files were modified, whether untracked source files participated, or
whether the benchmark harness itself differed from that commit.

M042/M043/M044 demonstrate the consequence: useful measurements were taken
from dirty worktrees, and the embedded SHA later had to be interpreted as a
base revision rather than an exact candidate.

### Stream provenance today

`benchmarks/src/main.rs` emits the stream report with platform,
architecture, Rust version, case results, and benchmark-only probes, but no
Git identity or worktree state.

`scripts/benchmark.sh` is only:

```sh
cargo fmt --manifest-path benchmarks/Cargo.toml -- --check
cargo run --manifest-path benchmarks/Cargo.toml --release --quiet --bin eggchaos-benchmarks
```

Therefore a retained stream JSON currently cannot prove its execution base
HEAD from its own contents. M046 had to infer M042/M043/M045 stream lineage
from adjacent commits and co-generated datagram evidence.

### M046 historical record is now authoritative

M047 must not rewrite the provenance reconstruction frozen by M046.

The following are historical facts, not implementation inputs to mutate:

- M042 dirty-worktree/base-HEAD distinction;
- M043/M044 dirty-worktree/base-HEAD rows;
- the two preserved M045 datagram generations;
- M042 thresholds and classifications;
- M043/M044 no-op dispositions;
- M045 qualification verdict;
- M046 closure/provenance map.

M047 applies to **newly generated artifacts only**.

## Design requirements

### 1. One provenance model across all benchmark families

New stream, stream-probe, and datagram reports must use the same provenance
schema.

At minimum the JSON must carry an object equivalent to:

```json
{
  "provenance": {
    "schema": 1,
    "head_sha": "<40-hex Git HEAD>",
    "worktree": "clean",
    "authoritative": true,
    "source_fingerprint": "<stable digest or null>"
  }
}
```

For a dirty run:

```json
{
  "provenance": {
    "schema": 1,
    "head_sha": "<base HEAD>",
    "worktree": "dirty",
    "authoritative": false,
    "source_fingerprint": "<digest of the measured worktree source state>"
  }
}
```

Additional useful fields may include:

- `index_dirty`;
- `tracked_dirty`;
- `untracked_source`;
- `git_describe`;
- `repo_root_relative`;
- `collector_version`.

Do not expose absolute local paths, usernames, tokens, environment secrets, or
full `git status` text in retained JSON.

### 2. Preserve `candidate_sha` compatibility where it already exists

Datagram reports may retain top-level `candidate_sha` for compatibility with
existing scripts/artifacts.

For new reports:

- `candidate_sha` must equal `provenance.head_sha`;
- documentation must define it as **base HEAD**, not sufficient proof of a
  clean exact candidate by itself;
- `provenance.authoritative == true` is valid only when the measured
  worktree is clean under the policy below.

Do not add a misleading `candidate_sha` to stream JSON unless there is a
specific compatibility reason. Prefer the shared `provenance` object as the
new authority.

### 3. Dirty-worktree runs remain possible but cannot be authoritative

Developers need to benchmark an in-progress optimization before committing it.

M047 must support that workflow.

A dirty run may execute and produce JSON, but:

- it must be marked `authoritative: false`;
- it must carry a non-empty source fingerprint;
- the wrapper must emit a clear stderr warning;
- qualification/closure instructions must prohibit using it as an "exact
  candidate" artifact;
- any tool mode intended to write canonical retained qualification evidence
  must fail on dirty worktree unless an explicitly named exploratory override
  is used.

The override must not silently set `authoritative: true`.

### 4. Clean-tree authoritative evidence must be mechanically provable

For a clean run:

- `head_sha` must resolve before execution;
- tracked/index/untracked **source-relevant** state must satisfy the clean
  policy;
- `authoritative` must be true;
- any source fingerprint that is emitted must be reproducible on the same
  tree.

Generated build directories, benchmark output files, and known
qualification-result destinations must not make an otherwise clean source tree
appear dirty merely because the benchmark is writing its own output.

### 5. Source fingerprint must not be self-referential

If M047 fingerprints dirty source content, the digest must exclude:

- `.git/`;
- Cargo `target/` directories;
- benchmark output destination files;
- `qualification/performance/` generated JSON unless a specific source file
  under that path is ever introduced;
- other documented generated/cache paths.

The fingerprint should cover repository source/config files that can affect the
benchmark or production runtime, including untracked source files that are not
ignored.

Preferred implementation is a small stdlib-only helper that hashes a sorted
`path + content hash` manifest rather than depending on platform-specific
`sha256sum`/BSD `shasum` behavior.

Python 3 is already used by `benchmark_datagram.sh`; if a Python helper is
chosen, it must remain stdlib-only and portable across the repository's
supported developer hosts.

## Scope

### In scope

- a shared benchmark-provenance collector/helper;
- stream report provenance;
- stream-probe report provenance;
- datagram report provenance;
- dirty/clean authoritative policy in benchmark wrappers;
- optional explicit artifact-output modes for the stream wrapper if needed to
  preserve stdout/stderr usability while retaining annotated JSON;
- tests for clean, dirty-tracked, dirty-staged, and untracked-source states;
- documentation defining exact-candidate versus exploratory evidence;
- one clean-tree M047 proof artifact for each benchmark family if practical;
- planning/registry/roadmap/AGENTS reconciliation;
- M047 closure on an exact clean candidate.

### Non-goals

- no production runtime optimization;
- no fault-semantic change;
- no benchmark workload/case/threshold change;
- no retuning M008/M023/M024/M042 budgets;
- no attempt to retroactively inject provenance fields into M023–M046 JSON;
- no rewrite of M046 preserved historical copies;
- no requirement that every ad-hoc developer benchmark be run from a clean
  tree;
- no Git commit automation;
- no automatic `git add`/`git stash`/`git commit`;
- no collection of host-identifying secrets or absolute repository paths;
- no new third-party runtime dependency;
- no release/tag/publication action.

## Affected surfaces

Expected:

- `scripts/benchmark.sh`;
- `scripts/benchmark_datagram.sh`;
- new shared helper under `scripts/` if that produces the clearest single
  provenance authority;
- `benchmarks/src/main.rs`;
- `benchmarks/src/bin/datagram.rs` only if provenance is injected in Rust
  rather than wrapper post-processing;
- focused tests under the repository's script-test convention;
- `qualification/performance/README.md`;
- optionally `architecture/verification-qualification.md`;
- `plans/registry.md`;
- `plans/README.md`;
- `plans/roadmap.md`;
- `AGENTS.md`;
- new M047 closure record.

No production crate should need modification. If implementation discovers that
a production crate must change to support provenance, stop and revise the plan
rather than coupling benchmark metadata to runtime APIs.

## Ordered work packages

### WP1 — Freeze the provenance schema and clean-tree policy

Before implementation, define the exact JSON object and dirty-state rules in
`qualification/performance/README.md` or a test fixture.

At minimum freeze:

- schema version;
- `head_sha`;
- `worktree = clean|dirty`;
- `authoritative`;
- `source_fingerprint` semantics;
- which paths count as source-relevant;
- which generated/output paths are excluded;
- relationship of legacy datagram `candidate_sha` to
  `provenance.head_sha`.

The schema is an internal evidence contract, not a public application API, but
it must be stable enough for qualification scripts to validate.

### WP2 — Add a single reusable provenance collector

Implement one authority used by both stream and datagram wrappers.

Required behavior:

1. locate the repository root via Git rather than current-working-directory
   assumptions;
2. obtain exact `HEAD`;
3. distinguish staged, tracked-unstaged, and untracked source changes;
4. ignore documented generated/build/evidence-output paths;
5. emit a stable source fingerprint for dirty source state;
6. expose machine-readable metadata to the benchmark wrappers;
7. exit nonzero on malformed/unavailable Git metadata when an authoritative
   evidence mode is requested.

Do not copy subtly different Git-state logic into both benchmark scripts.

### WP3 — Annotate stream JSON and stream-probe JSON

Because the stream harness currently writes the case report to stdout and
probe JSON to stderr, choose one explicit, testable propagation mechanism.

Preferred options, in order:

1. wrapper collects provenance and supplies it through environment variables
   consumed by `benchmarks/src/main.rs`, causing both JSON documents to embed
   the same provenance object; or
2. wrapper captures/annotates both documents after execution without
   intermixing human diagnostics.

Whichever approach is chosen:

- the case report and probe report from one invocation must have identical
  provenance;
- probe stderr must remain machine-separable from warnings/diagnostics;
- ordinary `cargo run --manifest-path benchmarks/Cargo.toml --release`
  may emit `authoritative: false` / unavailable provenance if it bypasses the
  wrapper, but canonical qualification docs must use the wrapper.

If a new `EGGCHAOS_STREAM_BENCH_OUTPUT` /
`EGGCHAOS_STREAM_PROBE_OUTPUT` convention is introduced, preserve the
existing console-oriented default for developers and document the new
artifact mode.

### WP4 — Annotate datagram JSON with the same schema

Replace the current one-field Git stamping with the shared collector.

Retain top-level `candidate_sha` if downstream tooling expects it, but assert:

`candidate_sha == provenance.head_sha`

The budget summary logic and all existing sample/scheduler fields must remain
unchanged.

### WP5 — Add authoritative retained-evidence guard

Define a clear wrapper mode for retained qualification evidence.

Examples of acceptable designs:

- `EGGCHAOS_BENCH_REQUIRE_CLEAN=1`;
- a `--require-clean` helper flag invoked by qualification scripts;
- an explicit canonical-output mode that implies clean-tree enforcement.

Requirements:

- clean tree -> run proceeds, `authoritative=true`;
- dirty tracked/index/untracked source -> authoritative mode exits nonzero
  before benchmark execution or before artifact acceptance;
- exploratory dirty mode -> run proceeds with `authoritative=false`,
  fingerprint + warning;
- no override can label a dirty result authoritative.

Do not globally refuse dirty developer benchmarks.

### WP6 — Regression-test Git-state handling in disposable repositories

Tests must not dirty the real checkout.

Create temporary Git repositories/fixtures and cover:

1. clean committed tree;
2. tracked unstaged edit;
3. staged edit;
4. untracked source file;
5. ignored/generated file only;
6. output artifact written to an excluded qualification path;
7. detached HEAD if supported;
8. invocation from a subdirectory;
9. missing Git repository / unresolved HEAD failure in authoritative mode.

Assert deterministic fingerprint output for identical dirty source content and
changed fingerprint when relevant content changes.

If executable-bit or line-ending behavior differs across platforms, define the
fingerprint at byte-content level and test it accordingly.

### WP7 — Freeze artifact-level schema regressions

Add focused checks that execute minimal/short benchmark runs where practical
and assert:

- stream case JSON has provenance;
- stream probe JSON has the same provenance;
- datagram JSON has provenance;
- datagram `candidate_sha == provenance.head_sha`;
- clean canonical mode says `authoritative=true`;
- no absolute local path or environment secret is emitted;
- legacy sample/case fields remain present;
- budget parser still accepts the annotated datagram report.

Tests should not assert benchmark throughput values.

### WP8 — Documentation and operator/agent guidance

Update `qualification/performance/README.md` to distinguish:

- exploratory result;
- retained authoritative result;
- clean exact candidate;
- dirty base HEAD;
- source fingerprint;
- evidence commit.

Add a short rule to `AGENTS.md`: do not call a performance artifact
"exact-candidate evidence" unless `provenance.authoritative == true` and its
`head_sha` is the candidate under discussion.

M046's historical provenance map remains intact and explicitly predates the
M047 schema.

### WP9 — Exact-candidate qualification

Create one committed M047 implementation candidate before authoritative proof
runs.

On that **clean** candidate:

- run focused provenance tests;
- run `./scripts/check.sh`;
- run a shortened or ordinary stream benchmark through the canonical wrapper;
- run a shortened datagram benchmark through the canonical wrapper;
- verify both retained outputs report:
  - the exact M047 candidate SHA;
  - `worktree=clean`;
  - `authoritative=true`;
- verify no performance budget or benchmark case definition changed.

If running a benchmark itself writes to a path inside the repository, that
output path must be excluded from dirty detection so provenance remains clean
for the measured source tree.

### WP10 — M047 closure

Create:

`plans/closure/M047-performance-artifact-provenance-capture-and-dirty-worktree-guard-corrective-closure.md`

Record:

- exact implementation/evidence candidate SHA;
- provenance schema;
- clean/dirty policy;
- focused test results;
- representative clean stream/probe/datagram metadata;
- proof benchmark workloads/thresholds are unchanged;
- proof production crates are untouched;
- any portability limitation;
- successor activation.

## Required invariants

- benchmark workloads and case semantics are unchanged;
- M008/M023/M024/M042 numeric thresholds are unchanged;
- historical JSON/artifacts are not rewritten;
- M046 provenance map remains the authority for pre-M047 artifacts;
- dirty exploratory runs stay possible;
- dirty runs can never claim `authoritative=true`;
- exact-candidate evidence requires a clean source-relevant worktree;
- provenance collection never mutates Git state;
- provenance JSON contains no secret/environment dump or absolute personal
  path;
- production crate/API/wire/fault behavior is unchanged.

## Verification

Minimum implementation-candidate checks:

```sh
./scripts/check.sh

# Focused provenance/script tests added by M047.
# Record their exact commands in closure.

# Canonical stream benchmark evidence (shortened workload is acceptable when
# the wrapper supports it without changing case semantics).
./scripts/benchmark.sh

# Canonical datagram evidence; existing environment knobs may reduce sample
# count for provenance verification.
EGGCHAOS_DATAGRAM_BENCH_DATAGRAMS=100 \
EGGCHAOS_DATAGRAM_BENCH_ROUNDS=1 \
./scripts/benchmark_datagram.sh
```

Also run a temporary-repository dirty-state matrix.

If the implementation touches only benchmark/scripts/docs and the benchmark
crate, full Toxiproxy, SDK, native-Python, fuzz, and release-artifact
qualification are not required. Hosted ordinary CI should still be green on
the final implementation candidate if the changed scripts are exercised there;
otherwise closure must state the coverage limitation.

## Acceptance criteria

M047 closes only when:

- every newly generated stream case report contains the shared provenance
  object;
- every newly generated stream probe report contains the same provenance;
- every newly generated datagram report contains the shared provenance;
- legacy datagram `candidate_sha` equals `provenance.head_sha`;
- clean-tree canonical runs are mechanically marked authoritative;
- tracked/staged/untracked source dirt makes canonical authoritative mode fail;
- exploratory dirty runs remain possible and are marked non-authoritative with
  a stable source fingerprint;
- generated outputs/build artifacts do not falsely dirty the measured source
  tree;
- temporary-repository regressions cover the clean/dirty matrix;
- no historical artifact is rewritten;
- no benchmark workload or threshold changes;
- no production crate/API/runtime change;
- exact clean M047 evidence identifies its own real commit SHA without
  post-hoc inference;
- planning/current-state docs agree on closure.

## Stop / escalation conditions

Do not close M047 if:

- a dirty run can be emitted as authoritative;
- provenance only records `git rev-parse HEAD` with no worktree-state proof;
- stream/probe and datagram use divergent provenance definitions;
- the source fingerprint includes the output artifact itself and therefore
  changes as the report is written;
- provenance collection runs `git add`, `stash`, `commit`, or otherwise
  mutates developer state;
- absolute local paths or arbitrary environment variables enter the artifact;
- canonical clean enforcement prevents ordinary exploratory dirty benchmarks;
- implementation alters benchmark workloads or production behavior to make
  provenance easier;
- the final retained M047 proof artifact again requires manual Git-history
  inference to identify what source tree ran.

A failure of the clean-candidate mechanism itself is an M047 blocker, not
something to document away in another provenance table.

## Follow-on activation

M047 activates no automatic successor.

After clean closure, M046 remains the authority for reconstructing historical
M042–M045 evidence, while M047 becomes the authority for provenance carried by
new performance artifacts. Further performance work should use the M047
canonical clean-evidence path from its first baseline onward.
