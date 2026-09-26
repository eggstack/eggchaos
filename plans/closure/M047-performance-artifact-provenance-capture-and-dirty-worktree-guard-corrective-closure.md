# M047 — Performance Artifact Provenance Capture and Dirty-Worktree Guard Corrective Closure

Status: closed

Exact M047 implementation/evidence candidate:
`493fb032c9665494247dcfaad8f4c5dcd7e7b6d1`
(this closure record, registry/roadmap/README/AGENTS reconciliation, and
the retained M047 proof artifacts are committed on top as docs + evidence
only — no source change after the candidate; see `git log --oneline`).

Closure date: 2026-09-26

Depends on: M046 closed (closure commit `33b5c087723da5a1eafd9b9d2f17875c5e31442c`);
M047 plan baselined at `33b5c087723da5a1eafd9b9d2f17875c5e31442c`, registered
in `08ca8a6`.

## Outcome

M047 eliminates the mechanism defect M046 could only document: new
stream, stream-probe, and datagram artifacts are now self-describing
under one shared provenance schema, clean authoritative evidence is
mechanically distinguishable from dirty exploratory measurement, and a
dirty tree can no longer silently stamp only its base HEAD as if it
were an exact candidate.

What was built on the candidate (benchmark/scripts/docs only; zero
files under `crates/`):

1. `scripts/bench_provenance.py` — the single Git-state authority
   (stdlib-only): repo root via `git rev-parse --show-toplevel`, base
   HEAD, staged/tracked-unstaged/untracked classification minus
   documented generated/output exclusions, stable SHA-256 dirty-source
   fingerprint, `{"provenance": {...}}` envelope, `--require-clean`
   (exit 2 on dirty), nonzero failure when Git metadata is
   unavailable. Never mutates Git state; emits no absolute paths,
   usernames, tokens, environment dumps, or full `git status` text.
2. `benchmarks/src/main.rs` — reads `EGGCHAOS_BENCH_PROVENANCE_JSON`
   (bare object or collector envelope) and embeds the identical object
   in the stdout case report and the stderr probe report; a direct
   `cargo run` bypass emits an explicit non-authoritative
   `collector: "unavailable"` marker.
3. `scripts/benchmark.sh` — collects provenance before execution,
   exports it, warns on dirty runs, refuses dirty trees before
   execution under `EGGCHAOS_BENCH_REQUIRE_CLEAN=1` / `--require-clean`,
   and supports `EGGCHAOS_STREAM_BENCH_OUTPUT` /
   `EGGCHAOS_STREAM_PROBE_OUTPUT` artifact mode.
4. `scripts/benchmark_datagram.sh` — same collector and guard;
   annotates with `provenance` plus legacy `candidate_sha` asserted
   equal to `provenance.head_sha` (documented as base HEAD, not
   clean-tree proof). Budget summary logic and thresholds untouched.
5. `scripts/tests/test_bench_provenance.sh` (WP6) — disposable-repo
   matrix: clean, tracked-unstaged, staged, untracked source,
   ignored-only, generated-only (`target/`, performance JSON),
   `--exclude` output artifact, detached HEAD, subdirectory,
   missing-Git failure, fingerprint determinism + content
   sensitivity, no Git mutation, no absolute paths, single-authority
   wiring.
6. `scripts/tests/test_bench_provenance_artifacts.sh` (WP7) —
   shortened wrapper runs asserting shared provenance in all three
   families, authoritative-iff-clean consistency, no path/secret
   leaks, legacy fields intact, datagram budget acceptance,
   unavailable bypass provenance, and guard behavior on both clean
   and dirty trees.
7. `qualification/performance/README.md` — frozen schema v1,
   clean-tree policy, evidence vocabulary
   (exploratory / retained authoritative / clean exact candidate /
   dirty base HEAD / evidence commit), and operator knobs. The M046
   historical map is untouched above the new section.
8. `AGENTS.md` exact-candidate rule; script inventory updates in
   `architecture/tooling-distribution.md` and
   `architecture/verification-qualification.md`.

## Provenance schema (v1, frozen)

```json
{
  "provenance": {
    "schema": 1,
    "head_sha": "<40-hex base HEAD, or null when unavailable>",
    "worktree": "clean | dirty | unknown",
    "authoritative": true,
    "source_fingerprint": "<64-hex sha256 of the dirty source delta, or null when clean>",
    "index_dirty": false,
    "tracked_dirty": false,
    "untracked_source": false,
    "git_describe": "<git describe output or null>",
    "collector": "bench_provenance.py v1"
  }
}
```

Clean/dirty policy: `authoritative` is true only when
`worktree == "clean"` and `head_sha` is 40-hex. Dirty runs execute
(exploratory) but carry `authoritative: false`, a non-empty
fingerprint, and a wrapper stderr warning. Canonical retained-evidence
mode (`EGGCHAOS_BENCH_REQUIRE_CLEAN=1` / `--require-clean`) exits 2
before benchmark execution on any source-relevant dirt, and no
override can label a dirty result authoritative. Exclusions:
any `target/` component, `__pycache__` / `*.pyc`, any `*.json` under
`qualification/performance/`, plus caller `--exclude` paths
(symlink-resolved, so macOS `/var -> /private/var` TMPDIR entries
match). The fingerprint hashes byte content only (sorted
`status + relpath + content-sha` manifest), so it is deterministic
for identical trees and sensitive to any byte change; output
artifacts can never self-contaminate it.

## Evidence (all on exact candidate `493fb03`, clean tree)

Host: Apple M4 Pro, macOS, `rustc 1.89.0 (29483883e 2025-08-04)`,
arm64 benchmark binary (harness `arch` field `aarch64`; retained
filenames follow the established `macos-arm64` series).

- `sh scripts/tests/test_bench_provenance.sh` → `{"bench_provenance":"pass"}`
  (full disposable-repo matrix: wiring, clean, tracked-unstaged,
  fingerprint determinism/sensitivity, staged, untracked, ignored,
  generated, output-exclusion, detached HEAD, subdirectory,
  missing-Git, no-mutation; also passes pre-commit on the dirty tree,
  proving the dirty fixtures do not depend on checkout state).
- `sh scripts/tests/test_bench_provenance_artifacts.sh` →
  `{"bench_provenance_artifacts":"pass"}` on the dirty pre-commit
  tree (`authoritative=false` branch + guard refusal) and again on
  the clean candidate (`authoritative=true` branch + guard pass).
- `./scripts/check.sh` → exit 0 on the candidate (fmt + clippy
  `-D warnings` + workspace tests + doc; 28 `test result: ok`, zero
  failures).
- Canonical stream proof (shortened workload, unchanged case
  semantics):
  `EGGCHAOS_BENCH_REQUIRE_CLEAN=1 EGGCHAOS_BENCH_BYTES=1048576 EGGCHAOS_BENCH_ROUNDS=1 EGGCHAOS_STREAM_BENCH_OUTPUT=…-m047-stream.json EGGCHAOS_STREAM_PROBE_OUTPUT=…-m047-stream-probes.json ./scripts/benchmark.sh`
  → exit 0; both files report `head_sha == 493fb03…`,
  `worktree=clean`, `authoritative=true`, byte-identical provenance
  objects, 17 cases + 8 probes, no `/Users` path leakage.
- Canonical datagram proof:
  `EGGCHAOS_BENCH_REQUIRE_CLEAN=1 EGGCHAOS_DATAGRAM_BENCH_DATAGRAMS=200 EGGCHAOS_DATAGRAM_BENCH_ROUNDS=1 EGGCHAOS_DATAGRAM_BENCH_OUTPUT=…-m047-datagram.json ./scripts/benchmark_datagram.sh`
  → exit 0; `candidate_sha == provenance.head_sha == 493fb03…`,
  `authoritative=true`, 24 samples + 4 scheduler probes,
  `datagram_budget: pass` + `matched_budget: pass` on the retained
  M023/M024 thresholds.
- Retained M047 proof artifacts (execution HEAD = candidate):
  `qualification/performance/2026-09-26-macos-arm64-m047-stream.json`,
  `…-m047-stream-probes.json`, `…-m047-datagram.json`.

## Invariance proof (no retune, no rewrite, no runtime change)

- `git diff 08ca8a6..493fb03 --stat -- crates/` is empty: no
  production crate, API, wire, fault, or runtime change.
- `benchmarks/src/main.rs` diff is provenance plumbing only
  (`bench_provenance()` + `emit_probes(&provenance)` + two format
  strings); `cases()`, all plan constructors, write profiles,
  evidence assertions, and probe definitions are byte-identical.
- Datagram thresholds in `scripts/benchmark_datagram.sh` remain
  `0.45 / 2.5 / 0.7 / 1.6 / 0.7`; the M008/M023/M024 budgets and the
  M042 classifications are not referenced, let alone retuned.
- No pre-M047 JSON was modified (`git diff` on
  `qualification/performance/` shows only the README addition plus
  the three new `*-m047-*.json` files); the M046 map and both M045
  generations stand.
- A dirty run can never be authoritative: the collector computes
  `authoritative` solely from measured state (no flag can set it),
  the wrappers only enforce `--require-clean` refusal, and both
  branches are covered by the committed tests.

## Portability limitations and coverage statement

- The collector needs `git` and stdlib-only `python3`; outside a Git
  checkout it fails closed (nonzero; `--require-clean` exits 2).
  Detached HEAD and subdirectory invocation are covered by tests.
- The fingerprint is byte-content addressed: permission-only
  (executable-bit) transitions without content change do not alter
  it, by design; any content byte change does.
- Per the plan, full Toxiproxy/SDK/native-Python/fuzz/release
  qualification was not rerun (benchmark/scripts/docs + benchmark
  crate only). Hosted ordinary CI is not awaited for this
  script-surface change: the benchmark wrappers and the two new
  script tests are not exercised in hosted CI, so closure rests on
  the local gates above (green on the exact candidate). The push of
  this closure triggers CI as usual; any hosted-only surprise would
  be handled as a new finding, not a silent M047 reopen.

## Successor activation and planning reconciliation

M047 activates no automatic successor. M046 remains the authority
for reconstructing historical M042–M045 evidence; M047 is the
authority for provenance carried by newly generated performance
artifacts. Further performance work must use the M047 canonical
clean-evidence path from its first baseline onward.

M047 was the sole `ready` plan and nothing was blocked on it, so no
future plan changes status: after closure, active/ready/blocked are
all empty and the performance order reads
`M042 -> M043 -> M044 -> M045 -> M046 -> M047 (all closed)`.
Registry, roadmap, `plans/README.md`, and `AGENTS.md` are reconciled
to `closed` in the same change as this record.
