# M064 — Repository Security Governance and Release-Integrity Guardrails

Status: blocked
Depends on: M062 closed; 2026-11-17 read-only security audit at `62682d9`
Role: repository-governance and supply-chain corrective; no production-code
change
Registration baseline: `60c28a48c2375d20c0cf793372423751967f2b9a` (`main`)
Audit evidence: `62682d955c802f8caedc43ec66f7100ca1af9dbb`

## Current execution status (2026-11-18)

M064 is blocked, not closed. GitHub readback confirms active rulesets
`main-protection` (ID `27505299`) and `release-tag-protection` (ID
`27505300`), enabled secret scanning and push protection, enabled vulnerability
alerts (`GET /vulnerability-alerts` returned `204`), enabled Dependabot
security updates, and zero open Dependabot alerts. The previous three
push-metadata vulnerability findings were not reproduced in the committed
Cargo lockfiles or the audited lockfile set; no advisory was dismissed.

The organization API reports the `eggstack` plan as `Free`. Repository-settings
update requests for non-provider pattern scanning and secret-scanning validity
checks returned successfully, but subsequent readbacks still report both as
disabled. GitHub’s documented plan requirements make these controls unavailable
under the current plan; WP3 therefore remains incomplete under this plan’s
acceptance criteria and stop condition. The pinned CodeQL workflow is present
only in the local candidate: there is no hosted run or analysis result for the
candidate, and the code-scanning-alert endpoint has no analysis to return.
Exact-head hosted CI and ruleset behavior on a test PR are also unverified.

Do not mark M064 closed unless the required plan features become available and
the remaining exact-head hosted evidence is obtained. The repository-security
readback and current limitations are recorded in `SECURITY.md`.

## Objective

Close the repository-governance findings from the 2026-11-17 security audit
so the project's published crates and attested binaries are protected by the
same discipline the code already applies:

1. protect `main` with a ruleset that requires pull requests and the
   qualified status checks, and blocks force-push and deletion;
2. protect `v*.*.*` release tags against creation, update, and deletion by
   anyone other than the release owner;
3. enable the GitHub security features that are currently disabled:
   secret scanning, push protection, non-provider pattern scanning,
   validity checks, Dependabot alerts, and Dependabot security updates;
4. add a pinned, least-privilege CodeQL workflow for Rust and the workflow
   surfaces, or record a supported fallback if CodeQL cannot analyse the
   pinned toolchain;
5. reconcile the three default-branch vulnerabilities reported in earlier
   push metadata, using the enabled alerting path rather than inference;
6. decide and document the dependency-review failure threshold and any
   additional Python/npm advisory coverage;
7. record reproducible API readbacks so the enabled state is evidence, not a
   claim.

This milestone changes repository settings and CI configuration. It does not
change crate source, the release workflow DAG, package versions, or the
published-release record.

## Research baseline

At the registration baseline, the audit established:

- `GET /repos/eggstack/eggchaos/branches/main/protection` returns `404`
  "Branch not protected";
- `GET /repos/eggstack/eggchaos/rulesets` returns `[]`, so neither branch nor
  tag rulesets are active;
- `security_and_analysis` reports `secret_scanning`,
  `secret_scanning_push_protection`,
  `secret_scanning_non_provider_patterns`,
  `secret_scanning_validity_checks`, and
  `dependabot_security_updates` all `disabled`;
- `GET /repos/eggstack/eggchaos/vulnerability-alerts` returns `204`, the
  documented response when vulnerability alerts are enabled; `404` means they
  are not enabled;
- `GET /repos/eggstack/eggchaos/code-scanning/alerts` returns `404`, so code
  scanning is not configured;
- `.github/dependabot.yml` already covers Cargo (all four workspaces),
  GitHub Actions, npm, and the pinned Python CI requirements, but version
  updates are not the same control as security alerts or security updates;
- every existing workflow already uses full-length action SHAs,
  `permissions: contents: read`, and `persist-credentials: false`; the
  release artifact job alone adds `id-token: write` and
  `attestations: write`;
- `.github/workflows/dependency-review.yml` fails on `high` severity only;
- `.github/workflows/security.yml` audits all four Cargo lockfiles daily and
  runs the `cargo-deny` policy, which is the current Rust advisory path;
- `cargo audit --deny warnings` reports no advisories on the four lockfiles
  at the audit revision, and also none on `main`'s lockfiles;
- earlier push metadata reported three default-branch vulnerabilities that
  could not be reproduced from any committed Cargo lockfile; because
  Dependabot alerts are disabled, the GitHub-side count could not be
  triaged through the API;
- `release.yml` builds and attests binaries from `v*.*.*` tags, so tag
  integrity is a release-integrity control, not a convenience setting;
- `SECURITY.md` already documents private vulnerability reporting and the
  supported release line.

The defect class is missing repository guardrails, not a code vulnerability.
The repository is public, so secret scanning and push protection are
available without a paid plan; rulesets and CodeQL are available to public
repositories as well. Enabling these settings is an owner-authorized
operation; this plan cannot execute it by itself.

## Scope

### In scope

1. Create a `main` ruleset with:
   - pull request required before merge;
   - the qualified status checks required and up to date (the exact contexts
     are enumerated from a green run, not guessed: at minimum the Rust
     `check`, `language-clients`, `python-native`, `api-gate`, and
     `performance-provenance` families, plus the dependency-review check and
     the new CodeQL check);
   - non-fast-forward (force-push) blocked;
   - deletion blocked;
   - linear history and conversation resolution required;
   - a documented, minimal bypass for the repository owner so an emergency
     release is not permanently locked out. If the account is a solo
     maintainer, state the review limitation explicitly rather than
     pretending independent review exists.
2. Create a `v*.*.*` tag ruleset that restricts creation, update, and
   deletion to the release owner, with the same documented bypass.
3. Enable secret scanning, push protection, non-provider patterns, and
   validity checks through the repository security settings.
4. Enable vulnerability alerts and Dependabot security updates; keep the
   existing version-update configuration and grouping.
5. Add `.github/workflows/codeql.yml`:
   - triggers on `push` to `main`, `pull_request`, and a bounded schedule;
   - `permissions: contents: read` plus `security-events: write` only;
   - SHA-pinned `actions/checkout`, the pinned Rust toolchain action, and a
     resolved, pinned `github/codeql-action` release SHA with a version
     comment;
   - a Rust analysis that builds the workspace under the pinned 1.89
     toolchain, or a documented supported build mode if CodeQL requires a
     different one;
   - the same `persist-credentials: false` discipline as the other
     workflows.
6. If CodeQL Rust cannot analyse the pinned toolchain, record the attempted
   configuration and use a supported fallback (for example a pinned
   `cargo-clippy` SARIF upload or an equivalent static-analysis job) without
   claiming CodeQL coverage that does not exist.
7. Enable the Dependabot alert path and triage every open alert:
   - reproduce each advisory against the committed lockfile;
   - fix it by a compatible dependency update where a fixed version exists;
   - otherwise dismiss with an explicit, recorded reason and scope;
   - reconcile the earlier three-vulnerability report, or record that it
     cannot be reproduced and why.
8. Decide the dependency-review threshold with evidence:
   - keep `high` and document that `cargo audit --deny warnings` covers all
     Rust severities; or
   - lower it to `moderate` if the resulting signal is actionable and does
     not create noise the project will ignore.
9. Evaluate Python/npm advisory coverage. The Python clients are stdlib-only
   and the TypeScript client has no runtime dependencies; the remaining
   surface is pinned build/test tooling. Add a pinned `pip-audit`-style check
   only if it does not introduce an unpinned runtime dependency, and record
   the decision either way.
10. Add a reproducible repository-settings readback (a small stdlib script
    or a documented command set) that asserts rulesets, security features,
    alerts, and CodeQL state from the GitHub API. The readback must not be
    wired into pull-request CI with a privileged token; run it manually or in
    an owner-controlled job.
11. Update `SECURITY.md`, `architecture/tooling-distribution.md`, and the
    README security section with the enabled controls, the owner bypass, the
    release-tag policy, and the alert-triage expectations.
12. Obtain exact-head hosted CI plus a green CodeQL run and record the
    evidence.

### Non-goals

- no crate source, Cargo manifest, lockfile, MSRV, package-version, API,
  wire, CLI, SDK, fault, scenario, Toxiproxy, or Eggfetch change;
- no change to the release workflow's jobs, permissions, artifact set,
  attestation scope, or publication sequence;
- no tag, crates.io/PyPI/npm publication, or GitHub Release;
- no organization-wide policy change outside `eggstack/eggchaos`;
- no claim that branch protection provides independent review when the
  repository has a single maintainer;
- no weakening of an existing workflow permission to make a new feature
  easier;
- no secret, token, or credential written into the repository or a workflow
  file;
- no rewrite of M059's security-hardening closure record; this plan extends
  its repository-governance layer.

## Affected surfaces

Expected implementation surfaces:

- `.github/workflows/codeql.yml` (new);
- `.github/workflows/dependency-review.yml` if the threshold changes;
- `.github/dependabot.yml` only if grouping or coverage needs correction;
- `SECURITY.md`;
- `architecture/tooling-distribution.md`;
- `README.md` security paragraph if the operator-facing controls change;
- `scripts/` for an optional settings-readback helper and its test;
- `plans/registry.md`, this plan, and the generated planning-state
  projections;
- a closure record under `plans/closure/` at closure.

GitHub repository settings are not files in this repository. Their evidence
belongs in the closure record with API readbacks, not in a fabricated local
configuration file.

## Ordered work packages

### WP1 — Snapshot the current state

1. Record the registration baseline and the exact API responses for branch
   protection, rulesets, `security_and_analysis`, vulnerability alerts, and
   code scanning.
2. Enumerate the actual required check-run contexts from the most recent
   green run on `main` or on the qualification candidate. Do not guess check
   names in the ruleset.
3. Confirm the authenticated GitHub identity can administer the repository;
   record the permission used. If it cannot, stop at this package and record
   the owner action required.

### WP2 — Branch and tag rulesets

1. Create the `main` ruleset in evaluate mode first if available, then
   active, with the enumerated checks.
2. Verify a test pull request cannot merge while a required check is failing
   or missing, and that direct force-push/deletion is rejected.
3. Create the release-tag ruleset for `refs/tags/v*.*.*`; verify a
   non-owner token cannot create or move a matching tag.
4. Record the ruleset IDs and the exact readback JSON in the closure
   evidence. Do not leave an evaluate-mode ruleset as the final state.

### WP3 — Secret scanning and Dependabot

1. Enable secret scanning, push protection, non-provider patterns, and
   validity checks.
2. Enable vulnerability alerts and Dependabot security updates.
3. Verify with readbacks, including the `204`/`200` distinction where the
   API uses it.
4. Open the Dependabot alert list and triage every open alert under WP6.

### WP4 — CodeQL

1. Resolve the current `github/codeql-action` release SHA and record its
   version comment; do not use a moving tag.
2. Add the workflow with least-privilege permissions and the same
   `persist-credentials: false` discipline.
3. Verify the Rust analysis actually builds and analyses the workspace on
   the pinned 1.89 toolchain; if it cannot, implement and document the
   fallback rather than leaving a permanently red workflow.
4. Confirm the workflow is accepted by `scripts/check_action_pins.py` and
   that a pull request produces a code-scanning result.
5. Record the first green CodeQL run ID and the analysed commit.

### WP5 — Alert and advisory reconciliation

1. Query the enabled Dependabot alert path and list every open alert.
2. Reproduce each advisory against the committed lockfile with the pinned
   audit tooling; record the path, package, advisory ID, and fixed version.
3. Fix what is fixable within the existing compatibility line; do not use
   this milestone for a major migration.
4. Dismiss only with a recorded reason and scope; never dismiss an
   unreproduced alert merely to make the count zero.
5. Reconcile the earlier three-vulnerability push report: either identify
   and address the same advisories, or record that the current lockfiles
   audit clean and that the GitHub-side count cannot be reproduced. Do not
   claim a fix that was not verified.
6. Decide and document the dependency-review threshold and Python/npm
   advisory coverage.

### WP6 — Documentation and readback

1. Update `SECURITY.md` with the enabled reporting/alerting controls and the
   supported release line.
2. Update `architecture/tooling-distribution.md` with the ruleset, tag,
   secret-scanning, Dependabot, CodeQL, and readback controls.
3. Add a README security note that points operators at the policy and the
   supported line without implying a bug bounty or guaranteed SLA.
4. Add the optional reusable readback helper; keep it stdlib-only or use the
   already-authenticated `gh` CLI, and do not add a dependency to the
   production crates.
5. Run the planning, version, release-state, and action-pin guards.

### WP7 — Qualification and closure

1. Obtain exact-head hosted CI on the final candidate and a green CodeQL
   analysis for that SHA.
2. Verify the ruleset/check behavior on a real pull request.
3. Write the closure record with API readbacks, ruleset IDs, alert triage,
   run IDs, and any owner-only settings that could not be verified from the
   repository.

## Invariants and failure semantics

- `main` cannot be force-pushed or deleted once the ruleset is active; the
  only bypass is the documented owner path.
- A `v*.*.*` tag cannot be created, moved, or deleted by a non-owner token.
- Existing workflows keep `contents: read` unless they already require a
  narrower write scope; the CodeQL job adds only `security-events: write`.
- No new secret is introduced; the readback uses the operator's existing
  authenticated `gh` session and records no token material.
- If a setting is unavailable on the repository plan or requires an
  organization owner, the plan records the blocker and does not mark the
  work package complete.
- If an alert cannot be reproduced, it is recorded as unreproduced, not
  silently dismissed.
- No production behavior changes, so the runtime and data-plane test matrix
  remains a regression check rather than a new-feature gate.
- The release workflow's permissions, jobs, and artifact contract remain
  byte-identical except for the additive CodeQL workflow.

## Test commands

Repository guards:

```sh
python3 scripts/check_action_pins.py --check
python3 scripts/check_planning_state.py --check
sh scripts/tests/test_planning_state.sh
python3 scripts/check_release_state_docs.py --check
python3 scripts/check_version_coherence.py --check
python3 scripts/check_lock_coherence.py --check
```

Workflow syntax and action-pin validation:

```sh
# If the repository's pinned Python environment is available:
uv run --with-requirements .github/python-ci/requirements.txt -- \
  python3 -c 'import yaml, pathlib; [yaml.safe_load(p.read_text()) for p in pathlib.Path(".github/workflows").glob("*.yml")]'
python3 scripts/check_action_pins.py --check
```

Settings readback (owner-authenticated; exact helper name may change):

```sh
gh api repos/eggstack/eggchaos/rulesets
gh api repos/eggstack/eggchaos/branches/main/protection
gh api repos/eggstack/eggchaos --jq '.security_and_analysis'
gh api repos/eggstack/eggchaos/vulnerability-alerts
gh api 'repos/eggstack/eggchaos/dependabot/alerts?state=open'
gh api repos/eggstack/eggchaos/code-scanning/alerts
```

Live checks:

```sh
cargo audit --deny warnings
for dir in . bindings/python-native benchmarks fuzz; do
  (cd "$dir" && cargo audit --deny warnings)
done
cargo deny check advisories licenses bans sources
```

## Acceptance criteria

1. `main` has an active ruleset requiring a pull request, the enumerated
   status checks, linear history, and conversation resolution, with
   force-push and deletion blocked; a failing-check PR cannot merge.
2. A `v*.*.*` tag cannot be created, updated, or deleted by a non-owner
   token.
3. Secret scanning, push protection, non-provider patterns, validity
   checks, vulnerability alerts, and Dependabot security updates all read
   back as enabled.
4. CodeQL is either green on the exact candidate for Rust and the workflow
   surfaces, or a documented supported fallback is green and the CodeQL gap
   is recorded. A permanently failing workflow is not acceptable.
5. Every open Dependabot alert is fixed or dismissed with a recorded reason;
   the earlier three-vulnerability report is reconciled or explicitly marked
   unreproduced with the evidence.
6. The dependency-review threshold and Python/npm advisory coverage are
   documented decisions, not accidental omissions.
7. `SECURITY.md`, `architecture/tooling-distribution.md`, and the README
   describe the enabled controls and the owner bypass.
8. All repository guards and exact-head hosted CI are green; the closure
   record contains the API readback evidence and run IDs.

## Stop conditions

- Stop if the authenticated account lacks repository-admin permission; record
  the exact owner action instead of fabricating enabled state.
- Stop if the repository plan does not offer a required feature (for example
  private-repository-only secret scanning); record the plan limitation.
- Stop if the `main` ruleset would lock the sole maintainer out of an
  emergency release without a documented bypass.
- Stop if CodeQL cannot build the pinned 1.89 workspace and no supported
  fallback exists; record the gap rather than claiming coverage.
- Stop if enabling Dependabot alerts requires a broader token scope than the
  operator is willing to grant; record the blocker.
- Do not close on "settings changed" alone; closure needs readback and live
  behavior evidence.

## Closure evidence

The closure record under `plans/closure/` must include:

- the exact candidate SHA and exact hosted run IDs;
- ruleset IDs, target refs, required contexts, bypass actors, and readback
  JSON;
- the `security_and_analysis` readback and vulnerability-alert state;
- the CodeQL run ID and the analysed SHA, or the documented fallback;
- the Dependabot alert list with per-alert disposition and evidence;
- the dependency-review threshold and Python/npm coverage decision;
- the documentation diff and guard outputs;
- any remaining owner-only action or unverifiable setting.

## Follow-on rules

M064 activates no successor. M063 and M065 are independently ready. A future
organization-wide policy, signed-commit requirement, or release-environment
approval gate needs a separate plan; it must not be inferred from this
milestone's ruleset.
