# M046 — Performance Evidence Provenance and Closure-Lineage Corrective

Status: ready  
Depends on: M045 closed at `a27a67a0577e3a92a2189d5188a63bf8a0a94dc0` / closure commit `37acc6b2cd8c632a3f73aa354297e83c08522e90`  
Role: narrow post-M045 evidence-provenance and closure-hygiene corrective  
Baseline: `37acc6b2cd8c632a3f73aa354297e83c08522e90`

## Objective

Repair provenance ambiguity in the M042/M045 performance evidence and closure
records without reopening the completed M042–M045 optimization tranche,
changing production code, rerunning optimization work, retuning thresholds, or
rewriting historical measurements to make their lineage look cleaner than it
was.

M046 exists because the production implementation and qualification are
healthy, but two exact-candidate statements are not currently self-consistent
with the Git history:

1. the M042 closure names an exact implementation candidate SHA that does not
   resolve;
2. the M042 datagram artifact records the pre-M042 registration HEAD even
   though the benchmark harness/artifact changes first appear in the M042
   implementation commit; and
3. the retained M045 datagram artifact was refreshed after the M045 evidence
   commit, while the closure prose still describes the earlier local evidence
   chronology.

The corrective must reconstruct and document what actually happened. It must
not invent a commit that never existed and must not silently edit old JSON
metadata so that it appears to have been produced on a different tree.

## Baseline and observed defects

Registration baseline:

`37acc6b2cd8c632a3f73aa354297e83c08522e90`

### 1. M042 closure references a non-existent SHA

`plans/closure/M042-performance-measurement-authority-and-baseline-correction-closure.md`
currently says:

`Exact implementation candidate: 2ce0c2388e3c2645e9b85969b8a96c39d2c89c45`

GitHub does not resolve that SHA in `eggstack/eggchaos`.

The registered pre-M042 planning commit is:

`2ce0c232c20898ddb7728d45130791ff1db53a27`

The M042 implementation/closure commit is:

`fb2c8fc740717cc17ba2cdf45aff8e8c1c8544a1`

M046 must determine whether the invalid SHA was a transcription error, an
uncommitted-worktree identifier copied incorrectly, or some other local
provenance label. If Git history cannot prove more than a transcription error,
the closure must say so explicitly rather than substituting a guessed SHA.

### 2. M042 raw datagram artifact records the pre-implementation HEAD

The retained artifact:

`qualification/performance/2026-09-26-macos-arm64-m042-datagram.json`

contains:

`candidate_sha = 2ce0c232c20898ddb7728d45130791ff1db53a27`

Yet the M042 benchmark code changes and the raw artifact are first committed
together in `fb2c8fc`.

This strongly indicates that the benchmark was executed from a working tree
whose Git HEAD was still `2ce0c232` while benchmark-harness changes were
uncommitted. In that case `candidate_sha` is a **base HEAD**, not a complete
content-addressed representation of the harness that produced the result.

M046 must preserve this distinction. Do not rewrite the JSON field to
`fb2c8fc`; doing so would make the artifact claim something it did not print
at execution time.

### 3. M045 datagram evidence has two historical revisions

At commit `a27a67a0577e3a92a2189d5188a63bf8a0a94dc0`, the retained
`m045-datagram.json` records:

`candidate_sha = e8ff753d08acfc2eb6dd1a452bb1a887d4fa41d4`

That is consistent with the M045 closure statement that the local performance
evidence was gathered on the combined production head `e8ff753`, with
`a27a67a` containing artifacts/docs only.

The later M045 closure commit:

`37acc6b2cd8c632a3f73aa354297e83c08522e90`

modifies the same canonical datagram artifact. The current file records:

`candidate_sha = a27a67a0577e3a92a2189d5188a63bf8a0a94dc0`

and contains refreshed measurements.

Therefore the current repository has two legitimate M045 datagram evidence
generations in Git history:

- original local evidence run against production head `e8ff753`, committed
  in `a27a67a`;
- a later controlled refresh run against the production-identical evidence
  tree `a27a67a`, committed as part of `37acc6b`.

The closure wording must distinguish them. It must not call both runs the same
"exact candidate" and must not imply that the current canonical JSON is the
same byte-for-byte artifact described by the earlier prose.

### 4. Current conclusions remain valid unless provenance audit disproves them

M045's hosted CI run `36255454409` is green 13/13 on
`a27a67a0577e3a92a2189d5188a63bf8a0a94dc0`.

The M045 closure also records that `e8ff753..a27a67a` contains no production
code changes.

M046 is not authorized to change the substantive M042 target classifications,
M042 frozen thresholds, M043/M044 no-op dispositions, M023/M024 budgets, or
M045 qualification verdict unless provenance reconstruction produces concrete
evidence that a stated result was associated with the wrong production tree.

## Scope

### In scope

- audit M042–M045 performance artifacts and closure records for:
  - resolvable commit SHAs;
  - artifact-embedded `candidate_sha`;
  - commit where each artifact first appeared;
  - later commits that changed the artifact;
  - production-code diff between the measured revision and evidence commit;
  - hosted run SHA where applicable;
- correct the invalid M042 closure SHA with an explicit, evidence-backed
  provenance description;
- add an additive provenance note/table to the M042 closure explaining that
  the datagram artifact's `candidate_sha` is the pre-commit base HEAD if that
  is what Git history supports;
- reconcile M045 closure wording to distinguish the original
  `e8ff753` local run from the later `a27a67a` controlled refresh;
- preserve both M045 datagram evidence generations in an accessible,
  immutable form if the current canonical filename obscures the earlier one;
- update `qualification/performance/README.md` with a compact provenance
  table for M042–M045;
- add explicit correction references from M042/M045 closure records to M046;
- reconcile registry/README/roadmap/AGENTS current state;
- create M046 closure evidence on an exact documentation/evidence candidate.

### Non-goals

- no production Rust code change;
- no benchmark-harness code change;
- no runtime, API, wire, config, CLI, SDK, binding, or Toxiproxy change;
- no new benchmark case;
- no performance rerun unless needed solely to verify artifact tooling and
  clearly labeled as new M046 evidence;
- no retuning M042 thresholds;
- no reinterpretation of M043/M044 no-op dispositions;
- no weakening of M008/M023/M024 budgets;
- no replacement of historical raw numbers;
- no deletion of older artifact revisions from Git history;
- no historical closure rewrite that erases the original statement instead of
  recording an additive correction;
- no release/tag/publication action.

## Affected surfaces

Expected:

- `plans/closure/M042-performance-measurement-authority-and-baseline-correction-closure.md`;
- `plans/closure/M045-performance-optimization-exact-head-qualification-and-reconciliation-closure.md`;
- `qualification/performance/README.md`;
- optional additive preserved M045 artifact snapshot(s) under
  `qualification/performance/` if needed to make both run generations
  directly accessible from HEAD;
- `plans/registry.md`;
- `plans/README.md`;
- `plans/roadmap.md`;
- `AGENTS.md`;
- new M046 closure record.

Production crates, benchmark sources, scripts, API contracts, and existing raw
artifact contents are outside scope unless the audit proves that preserving an
older artifact requires adding a new copy under a distinct filename.

## Provenance model to freeze

For each M042–M045 artifact family, record separate fields/concepts:

1. **production revision** — commit whose production Rust/runtime code was
   under measurement;
2. **benchmark-harness revision** — commit containing the benchmark source
   when a clean committed tree exists;
3. **execution base HEAD** — what `git rev-parse HEAD` reported during the
   run, if different because the worktree contained uncommitted harness
   changes;
4. **artifact evidence commit** — commit that first added that exact artifact
   byte content to Git;
5. **artifact refresh commit** — later commit that replaced/refreshed the
   canonical artifact, if any;
6. **hosted qualification revision** — exact Git SHA used by CI, where
   applicable.

Do not collapse these concepts into one generic "candidate" when they differ.

If a field cannot be proven from Git history/artifact metadata, write
`unknown / not reconstructable from retained evidence` rather than infer it.

## Ordered work packages

### WP1 — Reconstruct M042 provenance from Git history

Audit:

- `2ce0c232` registration tree;
- `fb2c8fc` M042 implementation commit;
- M042 stream, stream-probe, and datagram artifact blobs;
- first-appearance commit and embedded metadata;
- closure text that contains the invalid `2ce0c238...` SHA.

Record a provenance table before editing prose.

Determine whether any clean Git commit exactly represents the code/harness
used for the M042 measurements. If not, say that the run was produced from a
dirty working tree based on `2ce0c232`, to the extent Git history supports
that conclusion.

### WP2 — Correct M042 closure additively

Replace the invalid exact-candidate claim with precise terminology.

Preferred shape when supported by WP1:

- production/execution base HEAD: `2ce0c232...`;
- evidence/implementation commit containing the harness + raw artifacts:
  `fb2c8fc...`;
- note that the benchmark artifact was generated before the harness/artifact
  commit and therefore `candidate_sha` identifies the base HEAD rather than
  a clean content-addressed harness tree;
- explicitly state that the exact dirty worktree content cannot be recovered
  as a Git commit unless another retained source proves it.

Add a short "M046 provenance correction" note rather than pretending the
original invalid SHA never existed.

### WP3 — Reconstruct M045 artifact chronology

Use Git history to compare:

- `e8ff753`;
- `a27a67a`;
- `37acc6b`;
- the `m045-datagram.json` blob at `a27a67a`;
- the current `m045-datagram.json` blob.

Record which data set corresponds to each execution SHA and which commit first
stored each artifact.

Verify again that `e8ff753..a27a67a` has no production-code delta. If that
statement is false, M046 must stop and escalate to a substantive requalification
plan rather than treating this as documentation hygiene.

### WP4 — Preserve both M045 datagram generations without falsifying history

If the canonical filename replacement makes the original run inconvenient to
audit, add immutable additive copies with unambiguous names, for example:

- `2026-09-26-macos-arm64-m045-datagram-e8ff753.json`;
- `2026-09-26-macos-arm64-m045-datagram-a27a67a.json`.

The first must be byte-for-byte copied from the historical blob at the commit
where `candidate_sha=e8ff753`; the second must be byte-for-byte copied from
the current/refreshed blob with `candidate_sha=a27a67a`.

Do not edit either copy's embedded metadata.

The existing canonical filename may remain as the latest retained run, but
`qualification/performance/README.md` must say which generation it points
to.

### WP5 — Reconcile M045 closure wording

Clarify that:

- the initial local combined-head evidence was measured against
  `e8ff753`;
- `a27a67a` was production-identical and hosted CI ran green 13/13 there;
- the datagram artifact was later refreshed against `a27a67a` and committed
  in `37acc6b`;
- the current canonical datagram JSON therefore represents the later refresh,
  not the earlier `e8ff753` run;
- conclusions that rely on same-host A/B/no-op dispositions remain as
  originally stated unless the audit finds otherwise.

Add an M046 correction reference.

### WP6 — Audit the rest of M042–M045 for the same class of issue

Check every retained M042–M045 performance JSON/probe artifact and closure
candidate reference.

For each SHA-like token advertised as an exact commit:

- resolve it;
- verify it is reachable from repository history where expected;
- verify the surrounding prose labels it correctly.

Do not expand into M000–M041 unless the same audit reveals a directly related
contradiction in a current-state document.

### WP7 — Reconcile current-state planning and qualification docs

Update:

- `plans/registry.md`: M046 ready → closed only after corrective evidence;
- `plans/README.md`: explain M046 as a provenance-only successor;
- `plans/roadmap.md`: append the provenance corrective after the closed
  M042–M045 tranche without reopening it;
- `AGENTS.md`: point agents at M046 while ready and later identify it as the
  latest performance-evidence closure authority;
- `qualification/performance/README.md`: add the M042–M045 provenance table
  and preserved-artifact naming rules.

Historical M042–M045 plans remain historical. Their thresholds/status do not
change.

### WP8 — Exact-candidate corrective verification

Because M046 is documentary/evidence-only, verification is provenance-focused.

Required:

- all cited Git commit SHAs resolve;
- `git diff` proves no production/benchmark source changes in the M046
  implementation candidate;
- preserved historical artifacts, if added, are byte-identical to their source
  Git blobs;
- the current M045 artifact's embedded SHA matches the run it is documented as
  representing;
- hosted run `36255454409` still resolves to `a27a67a` and is 13/13 green;
- registry/README/roadmap/AGENTS agree on M046 state.

Run `./scripts/check.sh` on the exact corrective candidate unless the change
is strictly Markdown/JSON preservation and the repository's hosted workflow
already proves the same tree; any skip must be justified in closure.

No performance rerun is required merely to correct provenance.

### WP9 — M046 closure

Create:

`plans/closure/M046-performance-evidence-provenance-and-closure-lineage-corrective-closure.md`

The closure must include:

- exact M046 candidate SHA;
- before/after provenance table;
- every corrected invalid/misleading SHA statement;
- blob/source commit for any preserved historical artifact copy;
- proof that no production or benchmark-source code changed;
- hosted run verification for `36255454409`;
- explicit statement that M042 thresholds and M043/M044/M045 substantive
  conclusions were unchanged, or a clearly identified exception requiring a
  successor.

## Required tests / checks

At minimum:

```sh
git rev-parse --verify 2ce0c232c20898ddb7728d45130791ff1db53a27^{commit}
git rev-parse --verify fb2c8fc740717cc17ba2cdf45aff8e8c1c8544a1^{commit}
git rev-parse --verify e8ff753d08acfc2eb6dd1a452bb1a887d4fa41d4^{commit}
git rev-parse --verify a27a67a0577e3a92a2189d5188a63bf8a0a94dc0^{commit}
git rev-parse --verify 37acc6b2cd8c632a3f73aa354297e83c08522e90^{commit}

git diff --stat e8ff753d08acfc2eb6dd1a452bb1a887d4fa41d4..a27a67a0577e3a92a2189d5188a63bf8a0a94dc0
git diff --stat a27a67a0577e3a92a2189d5188a63bf8a0a94dc0..37acc6b2cd8c632a3f73aa354297e83c08522e90
```

Also verify historical artifact blobs with `git show <sha>:<path>` or
equivalent and compare checksums for any preserved copy.

If local repository tooling is available, run:

```sh
./scripts/check.sh
```

M046 does not require re-running M008/M023/M024 benchmarks, fuzz, Toxiproxy
oracles, SDK/native-Python qualification, or release artifact qualification
unless the corrective unexpectedly touches production/benchmark/script code.

## Acceptance criteria

M046 closes only when:

- the non-resolving `2ce0c238...` M042 closure SHA is no longer presented as
  an exact Git candidate;
- the M042 closure accurately distinguishes the base HEAD from the commit that
  first stored the changed harness/artifacts;
- no historical JSON is edited merely to replace its embedded candidate SHA;
- the M045 closure clearly distinguishes the `e8ff753` original local run,
  `a27a67a` evidence/hosted tree, and later `a27a67a` datagram refresh;
- both M045 datagram generations remain auditable from HEAD or from an
  explicitly documented immutable Git reference;
- every M042–M045 exact-commit reference checked by WP6 resolves;
- `qualification/performance/README.md` contains a provenance map sufficient
  for future agents to interpret the artifact lineage correctly;
- hosted run `36255454409` is still verified as green 13/13 on `a27a67a`;
- M042 target classifications and thresholds are unchanged;
- M043/M044 no-op dispositions are unchanged;
- M045's substantive qualification verdict is unchanged unless concrete
  evidence requires escalation;
- no production or benchmark-source code changes are included;
- registry/README/roadmap/AGENTS agree on final state;
- an exact M046 closure candidate is recorded.

## Stop / escalation conditions

Do not close M046 as a hygiene pass if:

- Git history shows `e8ff753..a27a67a` contains production code changes;
- a retained artifact is proven to have been labeled with a production SHA
  different from the tree actually measured, beyond the already-understood
  dirty-worktree/base-HEAD issue;
- M042 thresholds depend on a measurement that cannot be associated with any
  reconstructable production revision;
- the M045 13/13 hosted run is not actually on `a27a67a`;
- preserving the original M045 artifact requires altering its numeric content;
- the corrective changes benchmark or runtime source;
- provenance reconstruction changes a substantive performance/qualification
  conclusion.

Any of those findings requires a separately registered requalification
successor rather than silently broadening M046.

## Follow-on activation

M046 activates no automatic successor.

After clean closure, the M042–M045 performance tranche remains substantively
closed, with M046 serving only as the latest evidence-provenance authority.
Further performance work requires a new measured optimization finding; further
qualification work requires a concrete new regression or provenance failure.
