# M014 — Release State and Planning Reconciliation

Status: closed
Depends on: M008, M013
Successor: M015

## Historical closure note

M014 closed with the reconciliation commit family described in `plans/closure/M014-release-state-and-planning-reconciliation-closure.md`. Status retained as completed history; M015 is the final pre-tag exact-HEAD authority.

## Objective

Reconcile planning, closure, roadmap, dependency, and release-candidate records after the corrective campaign and the post-M008 dependency/CI fixes.

This is documentation/evidence cleanup. It does not change fault behavior, runtime semantics, compatibility behavior, or public APIs unless the census uncovers a substantive defect that requires a new numbered corrective plan.

## User-visible outcome

After M014:

- every canonical planning document agrees that M001–M013 and M008 are closed;
- the M009–M013 corrective chain is described as completed history rather than active work;
- the only remaining pre-tag sequence is M014 -> M015;
- M008's historical qualified candidate is clearly distinguished from later commits;
- stale dependency/publication blockers are removed or made explicitly historical;
- M015 is the sole final exact-HEAD pre-tag qualification gate.

## Baseline findings

The 2026-09-22/23 audit found no new architecture or implementation defect, but found evidence drift:

1. plans/registry.md marks M008 closed while plans/008-qualification-release-and-distribution.md still says Status: active.
2. plans/README.md still says release qualification is paused and describes M009/M010 as ready.
3. plans/roadmap.md still says corrective implementation is active and M009/M010 are ready.
4. The corrective graph is still described as active instead of completed.
5. M008 closure identifies 645a761723f6fe10fda0975c74e33343e2700764 as its qualified candidate, while main moved beyond it through closure bookkeeping, the EggServe crates.io dependency switch, bounded CI/H2 waits, and the deterministic H2 server-close fix.
6. Current main at audit time, a8933f37a13d7a7c2e7408ed16d61a511c826373, has green ordinary CI on Linux, macOS, and Windows, but no dedicated Release qualification workflow run after those later changes.
7. No v0.1.0 tag, crates.io publication, or GitHub release exists; those remain owner decisions.

Preserve historical closure records. Do not edit old candidate SHAs to imply evidence was produced on later commits.

## Scope

Primary surfaces:

- plans/registry.md
- plans/README.md
- plans/roadmap.md
- plans/008-qualification-release-and-distribution.md
- plans/closure/M008-qualification-release-and-distribution-closure.md
- README.md
- docs/
- qualification/
- Cargo.toml and Cargo.lock as census inputs
- .github/workflows/ as census inputs

## Non-goals

Do not change production semantics, add features, expand supported targets, create a tag, publish crates, create a GitHub release, manufacture evidence, or rewrite historical closure candidate SHAs.

If a correctness or release-workflow defect is discovered, stop and register a new corrective plan rather than hiding it in cleanup.

## Canonical state to establish

The repository should clearly state:

    M001–M013: closed
    M008: closed as the first release-qualification milestone
    M014: release-state/planning reconciliation
    M015: final exact-HEAD release requalification
    tag/publish/GitHub release: owner decision after M015

M015 does not reopen M008. It exists because legitimate changes landed after M008's recorded candidate.

## Candidate-lineage record

Add a concise lineage section to the M008 closure record or a companion release-state note covering:

- M008 candidate SHA 645a761723f6fe10fda0975c74e33343e2700764;
- every post-M008 commit through the M014 candidate;
- purpose of each commit;
- whether each changes production code, dependencies, tests/workflows, or planning only;
- latest ordinary CI verdict;
- statement that M015, not the older M008 candidate, is the final pre-tag exact-commit authority.

Do not call a later commit release-qualified merely because ordinary CI passed.

## Planning and release census

Search all canonical planning/top-level docs for stale current-state wording, including:

- M008 active or ready;
- release qualification paused;
- M009 and M010 ready;
- corrective implementation active;
- M013 -> M008 described as future work;
- git-pinned EggServe references;
- publication blocked by unpublished EggServe;
- old current-candidate language.

Classify each remaining match as historical context or correct it.

Also verify current claims for Rust 1.89, version 0.1.0, EggServe 0.2.0 registry dependencies, Eggfetch 0.2.0, Eggress relay 1.0.7, supported CI platforms, release targets, Toxiproxy limitations, hard-reset qualification, and the performance budget.

Downgrade unsupported claims rather than inventing evidence.

## Ordered work packages

1. WP1 — State census: enumerate stale current-state statements across plans, README, docs, workflow comments, and release notes.
2. WP2 — Planning reconciliation: align registry, planning README, roadmap, and M008 plan state.
3. WP3 — Candidate lineage: record post-M008 commit lineage and establish M015 as final exact-HEAD authority.
4. WP4 — Dependency/package census: verify manifests/lock are registry-based and stale publication blockers are removed.
5. WP5 — Support/release wording: reconcile README/docs/support claims against evidence.
6. WP6 — Search-based stale-state gate: review every remaining stale phrase as historical or corrected.
7. WP7 — CI confirmation: confirm ordinary Linux/macOS/Windows CI is green on the M014 candidate; run full local gates if manifests/code changed.
8. WP8 — Closure: write plans/closure/M014-release-state-and-planning-reconciliation-closure.md and activate M015.

## Verification

At minimum run:

    git grep -n -E 'M008 (is )?(active|ready)|release qualification.*paused|M009 and M010.*ready|corrective implementation active'
    git grep -n -E 'git.*eggserve|eggserve.*git|unpublished.*eggserve|eggserve.*unpublished'
    cargo metadata --format-version 1 --locked
    cargo tree --locked

If code/manifests change, also run the full workspace gate:

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features
    cargo doc --workspace --all-features --no-deps
    cargo audit --deny warnings
    cargo deny check advisories licenses bans sources

## Acceptance criteria

M014 closes only when canonical planning sources agree, M008's plan is marked closed, completed corrective work is not described as pending, historical evidence remains tied to its real candidate, post-M008 lineage is explicit, dependency/publication wording is current, stale current-state phrases are resolved, no implementation defect is concealed, exact-commit closure evidence exists, and M015 is the sole ready pre-tag milestone.

## Stop/rejection conditions

Stop and create a new corrective plan if the census finds a code correctness regression, a nontrivial release-workflow defect, a claimed production capability that is missing, a non-publishable dependency graph, or a reproducible platform failure caused by product code.

Do not close M014 by deleting contradictory evidence.

## Follow-on activation

On closure: M014 -> closed and M015 -> ready. No tag, crate publication, or GitHub release occurs in M014.
