# M015 — Final Exact-HEAD Release Requalification

Status: ready
Depends on: M014
Role: final pre-tag gate

## Objective

Run release qualification against one exact post-cleanup commit and make that commit, rather than the older M008 candidate, the final pre-tag release-candidate authority.

Ordinary three-platform CI is necessary but not sufficient. M015 requires the dedicated release workflow, artifact matrix, Toxiproxy differential, Eggfetch qualification, fuzz/security/package gates, and exact-commit evidence.

## User-visible outcome

After M015 the repository has one explicit commit qualified for a v0.1.0 tag, with full release-workflow success, Linux/macOS/Windows artifacts, checksums, pinned Toxiproxy evidence, Eggfetch regression evidence, fuzz/security/package evidence, final dependency graph, and reconciled planning/docs.

Tagging, crates.io publication, and GitHub release creation remain owner actions after M015.

## Preconditions

M014 is closed and included in the candidate tree. There is no uncommitted or unregistered implementation work.

If a fix lands after candidate selection, select a new candidate and rerun every affected gate. Planning-only closure-note commits may follow if explicitly distinguished from the qualified code candidate.

## Candidate freeze

Record one exact SHA before qualification. Evidence from multiple moving commits must not be combined into one clean verdict without explicit justification.

## Dedicated release workflow

Trigger .github/workflows/release.yml via workflow_dispatch against the exact candidate ref.

The workflow must finish green, including its qualification and artifact jobs.

The expected qualification lane includes the current equivalents of:

- release smoke;
- fuzz qualification;
- pinned Toxiproxy v2.12 qualification;
- Eggfetch qualification;
- release artifact smoke.

The artifact matrix must cover:

- Linux x86_64 GNU;
- Linux aarch64 GNU;
- macOS x86_64;
- macOS aarch64;
- Windows x86_64 MSVC.

Record workflow run ID/URL and exact workflow configuration SHA.

## Ordinary CI gate

The exact candidate must also have green ordinary CI on Ubuntu, macOS, and Windows for fmt, clippy with warnings denied, full workspace tests, docs, audit, and deny.

A release workflow success does not excuse ordinary CI regression.

## External qualification

### Toxiproxy

Run the pinned v2.12.0 oracle corpus and record oracle version/checksum, platform, pass/fail count, normalization count, Go client smoke version/result, and independent client smoke version/result.

The declared M012/M013 compatibility surface must not regress.

### Eggfetch

Run the full current integration/regression suite including H1, keep-alive/live mutation, HTTPS trust/rejection, H2 concurrency, blackhole/timeout interaction, mid-response termination, shaping, reconnect/redial, and error/redaction paths.

The deterministic H2 test transport fix that landed after M008 must be covered on the exact candidate.

## Fuzz, security, and package gates

At minimum:

    EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh
    cargo audit --deny warnings
    cargo deny check advisories licenses bans sources

Record actual fuzz execution/time where available.

For every publishable crate, run cargo package --locked or the canonical package dry-run and prove:

- required metadata/readme/license are present;
- no local path or git dependency blocks crates.io publication;
- the final dependency graph resolves from registry packages in a documented publish order.

Expected order should begin with eggchaos-core, then eggchaos-eggfetch, then server/toxiproxy/cli as Cargo metadata requires. Do not publish.

## Artifact evidence

Record from the successful release workflow:

- run ID/URL;
- candidate SHA;
- artifact names;
- target triples;
- SHA-256 files;
- sizes;
- runtime smoke result where execution is possible;
- build-only qualification where cross/foreign execution is not possible.

Do not claim runtime smoke for a target that was only built.

## Performance sanity

The M008 budget remains authoritative unless deliberately revised with evidence: empty-plan mean throughput at least 70% of same-session bare relay.

At minimum rerun the canonical performance qualification or a comparable no-fault sanity measurement. If the candidate includes hot-path changes, rerun the full M008 performance matrix.

## Documentation/version census

Verify workspace version remains 0.1.0, docs list exact support/limitations, registry identifies M015 correctly, no current text calls 645a761 the final release candidate, and owner-action wording clearly separates qualification from tag/publish/release actions.

## Ordered work packages

1. WP1 — Candidate freeze: select exact HEAD after M014 and record workflow/config SHAs.
2. WP2 — Ordinary CI: obtain green Ubuntu/macOS/Windows CI on the candidate.
3. WP3 — Release workflow: dispatch and obtain a fully green release-qualification run including all artifact jobs.
4. WP4 — External/integration gates: rerun pinned Toxiproxy and Eggfetch qualification.
5. WP5 — Fuzz/security/package gates: rerun fuzz, audit/deny, and package dry-runs with registry-only dependency verification.
6. WP6 — Artifact inspection: record target artifacts/checksums/smoke/build evidence.
7. WP7 — Performance/docs census: confirm budget and final support/version/release wording.
8. WP8 — Closure: write plans/closure/M015-final-exact-head-release-requalification-closure.md with a clean/not-clean verdict.

## Acceptance criteria

M015 closes only when one exact candidate is named; ordinary Linux/macOS/Windows CI and the dedicated release workflow are green on it; every required artifact job succeeds; Toxiproxy and Eggfetch qualification pass; fuzz/security/package gates pass; package dry-runs prove an order-publishable registry graph; artifacts/checksums/target evidence are recorded; performance is within budget or explained; docs identify the M015 candidate as final pre-tag authority; and no unresolved correctness or workflow finding remains.

## Stop/rejection conditions

Do not close if the release workflow was never dispatched, a required artifact target failed or was silently skipped, evidence comes from a different code SHA without explanation, package dry-runs rely on unpublished/local/git sources, Toxiproxy/Eggfetch qualification fails, ordinary CI and release CI disagree without explanation, or a release-critical fix lands without rerunning affected gates.

Transient infrastructure failures may be rerun and documented. Reproducible product/workflow failures require a new corrective plan.

## Follow-on activation

On clean M015 closure there is no further planned pre-tag implementation work. By explicit owner decision the repository may then create the v0.1.0 tag, publish crates in verified order, and create the GitHub release with qualified artifacts/checksums.
