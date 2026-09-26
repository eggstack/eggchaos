# M046 — Performance Evidence Provenance and Closure-Lineage Corrective Closure

Status: closed

Exact M046 corrective candidate:
`f569a0a36f99c103ce62852a3aa89584005442d9`
(docs + preserved evidence only; this closure record itself is committed
on top as docs-only — see `git log --oneline` at closure).

Closure date: 2026-09-26

Depends on: M045 closed (evidence candidate `a27a67a`, closure commit
`37acc6b`); M046 plan baselined at `37acc6b`, registered in `8214826`.

## Outcome

M046 repairs the three provenance defects named in its plan without
reopening the completed M042–M045 optimization tranche, changing any
production or benchmark source, rerunning optimization work, retuning
any threshold, rewriting any historical measurement, or taking any
release/tag action:

1. the non-resolving M042 exact-candidate SHA is no longer presented
   as a Git candidate — corrected additively as a transcription error
   with the evidence-backed base HEAD / evidence commit distinction;
2. the M042 datagram artifact's `candidate_sha` is documented as the
   pre-commit execution base HEAD, not a clean harness tree — the JSON
   itself is untouched;
3. the two M045 datagram evidence generations are distinguished in
   closure prose and both preserved as byte-identical additive copies —
   the canonical file and its history are not rewritten.

M046 activates no successor. The M042–M045 tranche remains
substantively closed; M046 is the latest evidence-provenance authority.

## Before/after provenance table

| # | Before | After (M046) | Evidence |
| --- | --- | --- | --- |
| 1 | M042 closure: `Exact implementation candidate: 2ce0c2388e3c2645e9b85969b8a96c39d2c89c45` | Original statement retained as superseded; header now points at the M046 correction: base HEAD `2ce0c232c20898ddb7728d45130791ff1db53a27`, evidence/implementation commit `fb2c8fc740717cc17ba2cdf45aff8e8c1c8544a1`, dirty-worktree execution, exact dirty content not recoverable, token recorded as unresolvable transcription error | `git rev-parse --verify 2ce0c238…` fails; `2ce0c232`, `fb2c8fc` resolve; `git diff 2ce0c232..fb2c8fc --stat` shows the harness + artifacts first appear in `fb2c8fc` |
| 2 | M042 datagram artifact meaning ambiguous (records `2ce0c232` while committed in `fb2c8fc`) | Documented: `candidate_sha` is the execution base HEAD of a dirty `2ce0c232`-based worktree (stamped by `scripts/benchmark_datagram.sh` via `git rev-parse HEAD`); JSON byte content unchanged | Blob first appears in `fb2c8fc`, embedded SHA `2ce0c232`, unchanged since (`git diff fb2c8fc..HEAD` empty for the file) |
| 3 | M045 closure: all local evidence "gathered on" `e8ff753`, while canonical `m045-datagram.json` records `a27a67a` | Additive clarification: generation 1 (`e8ff753`, stored in `a27a67a`) vs generation 2 refresh (`a27a67a`, stored in `37acc6b`); canonical file = generation 2; stream/probe files = single generation (`e8ff753` base, `a27a67a` evidence, untouched by `37acc6b`) | `git show a27a67a:<path>` → `e8ff753…` (sha256 `171e9676…`); HEAD blob → `a27a67a…` (sha256 `2591e131…`); `git diff --name-only a27a67a..37acc6b` shows only the datagram JSON refreshed among artifacts |
| 4 | Original M045 datagram run reachable only via history | Both generations auditable from HEAD: `2026-09-26-macos-arm64-m045-datagram-e8ff753.json` (byte copy of `a27a67a` blob) and `…-a27a67a.json` (byte copy of `37acc6b`/HEAD blob) | sha256 copies match source blobs exactly (see WP8); embedded metadata unedited |
| 5 | No M042–M045 lineage map for future agents | `qualification/performance/README.md` freezes the six-concept provenance map (production / harness / base HEAD / evidence / refresh / hosted) plus the additive-refresh naming rule | This candidate |
| 6 | Registry/README/roadmap/AGENTS: M046 `ready` | M046 `closed`, tranche order extended, AGENTS points at M046 as latest performance-evidence authority | This candidate |

Full per-artifact map (including the M043 `fb2c8fc`-embedded and M044
`df81ea4`-embedded datagram rows, both consistent with the same
dirty-worktree/base-HEAD pattern on production-identical datagram
paths) lives in `qualification/performance/README.md` and is not
duplicated here beyond the summary above.

## Every corrected invalid/misleading SHA statement

- `2ce0c2388e3c2645e9b85969b8a96c39d2c89c45` (M042 closure): does not
  resolve; superseded as above. No replacement SHA was invented.
- M042 `candidate_sha = 2ce0c232…`: retained verbatim in JSON;
  relabeled as base HEAD, not evidence commit.
- M045 "gathered on `e8ff753`" prose: retained, now scoped to
  generation 1 + stream/probe files; generation-2 refresh explicitly
  attributed to `a27a67a` / `37acc6b`.
- All other M042–M045 exact-commit references audited (WP6) resolve:
  `fb2c8fc`, `67ce4ab`, `7a6dbb8`, `df81ea4` (M043 closure commit,
  embedded in the M044 artifact as its base HEAD), `e8ff753`,
  `a27a67a`, `37acc6b`, `724b967`. Stream/probe JSON files carry no
  embedded SHA by harness design (only the datagram script stamps
  one); their rows are marked inferred where JSON alone proves nothing.

## Preserved historical artifacts (blob sources)

- `qualification/performance/2026-09-26-macos-arm64-m045-datagram-e8ff753.json`:
  source blob `a27a67a0577e3a92a2189d5188a63bf8a0a94dc0:qualification/performance/2026-09-26-macos-arm64-m045-datagram.json`,
  sha256 `171e9676606e169b78839ba05cce526c75ffdf0a237eacc6e8b1f7fab2850e4e`
  (identical for source and copy), embedded `candidate_sha = e8ff753…`.
- `qualification/performance/2026-09-26-macos-arm64-m045-datagram-a27a67a.json`:
  source blob `HEAD:qualification/performance/2026-09-26-macos-arm64-m045-datagram.json`
  at the corrective candidate (== `37acc6b` blob),
  sha256 `2591e131b0937d821845ad27e95fa0b1f03548e488853a061914d6ea9312129d`
  (identical for source and copy), embedded `candidate_sha = a27a67a…`.

## Proof of no production or benchmark-source change

- `git diff 37acc6b..f569a0a --name-only -- crates benchmarks scripts api bindings src Cargo.toml Cargo.lock rust-toolchain.toml`
  is empty (corrective candidate vs M046 plan baseline).
- `git diff HEAD` for the pre-existing `m042/m045` JSON blobs is empty;
  the only `qualification/performance/` changes are `README.md` plus
  the two new additive copies.
- `e8ff753..a27a67a --name-only` remains docs + JSON only (re-verified);
  `a27a67a..37acc6b` production delta remains empty (re-verified) —
  the stop conditions did not trigger, so no requalification successor
  was registered.

## Hosted run verification

- `gh api repos/eggstack/eggchaos/actions/runs/36255454409`:
  `head_sha = a27a67a0577e3a92a2189d5188a63bf8a0a94dc0`,
  `conclusion = success`, 13/13 jobs green (ubuntu/macOS/windows
  `check`, `language-clients` matrix, `python-native`).

## Verification run on the exact candidate

- `./scripts/check.sh`: exit 0 on the corrective-candidate tree
  (fmt + clippy `-D warnings` + workspace `--all-targets --all-features`
  tests + doc; 28 `test result: ok`, zero failures). The closure commit
  on top adds only this file; no code, harness, script, contract, or
  artifact change.
- Required plan checks re-run: all five SHAs resolve; the invalid
  `2ce0c238…` token confirmed non-resolving; preserved-copy checksums
  match source blobs; registry/README/roadmap/AGENTS agree on
  M046 `closed` with no `ready`/`active` remainder.

## Substantive conclusions explicitly unchanged

- M042 target classifications and frozen per-target thresholds:
  unchanged.
- M043/M044 no-op dispositions (WP7 vectored prefix; WP3 registry
  conversion) and skipped-WP rationale: unchanged.
- M008/M023/M024 budgets: unchanged and unweakened.
- M045 substantive qualification verdict (combined-head green,
  hosted 13/13 on `a27a67a`): unchanged.
- No exception requiring a successor was found.

## Successor activation / follow-on rules

M046 activates no automatic successor. Further performance work
requires a new measured optimization finding with a numbered plan;
further qualification work requires a concrete new regression or
provenance failure. Future artifact refreshes must follow the additive
naming rule in `qualification/performance/README.md`, never rewriting
embedded `candidate_sha` history.

## Test commands retained for re-qualification

```sh
git rev-parse --verify 2ce0c232c20898ddb7728d45130791ff1db53a27^{commit}
git rev-parse --verify fb2c8fc740717cc17ba2cdf45aff8e8c1c8544a1^{commit}
git rev-parse --verify e8ff753d08acfc2eb6dd1a452bb1a887d4fa41d4^{commit}
git rev-parse --verify a27a67a0577e3a92a2189d5188a63bf8a0a94dc0^{commit}
git rev-parse --verify 37acc6b2cd8c632a3f73aa354297e83c08522e90^{commit}
git rev-parse --verify f569a0a36f99c103ce62852a3aa89584005442d9^{commit}

git diff --stat e8ff753d08acfc2eb6dd1a452bb1a887d4fa41d4..a27a67a0577e3a92a2189d5188a63bf8a0a94dc0
git diff --stat a27a67a0577e3a92a2189d5188a63bf8a0a94dc0..37acc6b2cd8c632a3f73aa354297e83c08522e90
./scripts/check.sh
```
