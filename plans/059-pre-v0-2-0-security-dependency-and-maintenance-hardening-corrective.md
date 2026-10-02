# M059 — Pre-v0.2.0 Security, Dependency, and Maintenance Hardening Corrective

Status: closed
Depends on: M057 closed; M058 publication not started
Role: release-blocking, compatibility-preserving security/dependency/maintenance corrective
Activation baseline: `0f8a8ebc8b8f734951624be32362ee512b9e3d5e`
Closure: closed on exact candidate `1409d0fa11dddfeff3f680ef453bf830e1339606`
(evidence in `plans/closure/M059-pre-v0-2-0-security-dependency-and-maintenance-hardening-corrective-closure.md`;
hosted CI `36922662654` 15/15 + release dispatch `36922672946` 7/7; M058
reactivated to `ready` on the exact M059 candidate).

## Objective

Close the remaining pre-`v0.2.0` security, dependency, and maintenance gaps identified by the 2026-10-01 repository audit without removing or redefining any existing API, protocol, CLI command, deterministic behavior, compatibility profile, supported transport, language binding, or release target.

M059 is a corrective interlock in front of M058. M058 remains the owner-controlled publication milestone, but it must not tag, publish, or create a GitHub Release until M059 closes on an exact candidate and the resulting release workflow is requalified.

The audit originally found two native-admin authentication issues: an empty configured bearer token could satisfy the non-loopback startup guard, and the token comparison was length-dependent. Both are already corrected on the activation baseline: public admin rejects an empty token, missing/non-Bearer authorization fails explicitly, and token comparison hashes both inputs to fixed-width SHA-256 digests before comparison. M059 must preserve those fixes; it does not reopen the native auth design.

## Research baseline

### Repository observations at activation

At `0f8a8ebc8b8f734951624be32362ee512b9e3d5e`:

- the M057 qualified candidate `818e5674f2efaf96ec8effda81cef1dfa7a48614` is eight commits behind the activation baseline, with post-M057 production changes across core/server/protocol/CLI/embed/Toxiproxy/Eggfetch in addition to documentation and release planning; those changes have not yet received M057-equivalent exact-head release qualification;
- at least one post-M057 commit changes public Rust helper signatures in `eggchaos-toxiproxy` (for example compatibility JSON helpers now return `Result`), so the pre-release API check must detect and classify drift from M057 rather than using the already-changed activation HEAD as its sole baseline;
- all eight normal workspace crates still omit `[lints] workspace = true` even though the root defines `[workspace.lints.rust]` and `[workspace.lints.clippy]`;
- therefore the intended workspace lint policy is not inherited by package manifests. Cargo documents that workspace lints are opt-in at the package level and exposes `missing_lints_inheritance` because assuming implicit inheritance is a common error;
- the root lock resolves first-party runtime dependencies older than sibling-repository baselines:
  - `eggress-relay 1.0.7` versus current Eggress workspace `1.0.11`;
  - `eggfetch-core 0.2.0` versus current Eggfetch `0.2.1`;
  - `eggserve-primitives 0.2.0` versus current EggServe primitives `0.2.2`;
  - `eggserve-server 0.2.0`, while standalone Eggchaos workspaces already resolve `0.2.1`; current EggServe main has moved to `eggserve-server 0.4.0`, which is outside Eggchaos's existing `0.2.x` compatibility line and is **not** an automatic M059 upgrade;
- the four committed Cargo lockfiles disagree on first-party runtime versions:
  - root: relay 1.0.7 / EggServe server+primitives 0.2.0 / Eggfetch core 0.2.0;
  - Python-native: relay 1.0.10 / EggServe server+primitives 0.2.1;
  - benchmarks: relay 1.0.7 / EggServe server 0.2.1 / primitives 0.2.0 / Eggfetch core 0.2.0;
  - fuzz: relay 1.0.8 / EggServe server 0.2.1 / primitives 0.2.0;
- root `[workspace.dependencies]` still declares six entries not inherited by any current workspace member: `http-body-util`, `hyper`, `hyper-util`, `prometheus-client`, `rand`, and `url`;
- the direct `socket2 = "0.5"` dependency coexists with transitive `socket2 0.6`;
- CI and release workflows use mutable action references such as `actions/checkout@v4`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`, `actions/setup-python@v5`, `actions/setup-node@v4`, and `actions/upload-artifact@v4`;
- neither workflow declares explicit `permissions:`, so least-privilege intent is not encoded in repository policy;
- there is no `.github/dependabot.yml`, dedicated scheduled dependency-security workflow, dependency-review workflow, or `SECURITY.md`;
- TypeScript CI falls back to `npm install` despite a committed `package-lock.json`;
- Python CI installs several build/test tools without exact version constraints;
- `bindings/python-native/src/lib.rs` still carries crate-wide `#![allow(unsafe_code)]` because PyO3 macro expansion requires an exception, while ordinary workspace crates forbid handwritten unsafe;
- native/OpenAPI/SDK drift is mechanically checked, but public Rust API removal is not independently checked.

The August 20, 2026 Rust supply-chain incident involved malicious crates and a compromised publication path. None of the affected malicious/deleted versions identified by the Rust Security Response Team is present in any of Eggchaos's four current Cargo lockfiles. This is evidence that the repository is not currently carrying that incident, not a reason to omit continuous dependency monitoring.

### External security guidance used by this plan

Implementation should preserve the intent of the following current upstream guidance rather than copying version tags blindly:

- Cargo workspace lint inheritance: https://doc.rust-lang.org/cargo/reference/lints.html and https://doc.rust-lang.org/cargo/reference/workspaces.html
- GitHub Actions secure use: https://docs.github.com/en/actions/reference/security/secure-use
- GitHub dependency review: https://docs.github.com/en/code-security/how-tos/secure-your-supply-chain/manage-your-dependency-security/configure-dependency-review-action
- GitHub artifact attestations: https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations
- Rust Security Response Team, 2026-08-20 supply-chain incident: https://blog.rust-lang.org/2026/08/20/supply-chain-attack-on-arrayref/
- cargo-semver-checks baseline-revision support: https://github.com/obi1kenobi/cargo-semver-checks

GitHub currently recommends full-length commit-SHA action pinning as the immutable action reference. Current first-party action release lines are newer than Eggchaos's workflow references, but M059 must qualify each selected action revision rather than treating “latest” as a security property.

## Scope

### In scope

1. activate the workspace lint policy for all eight normal workspace members;
2. correct newly exposed lint failures with semantics-preserving edits only;
3. remove proven-unused root workspace dependency declarations;
4. update and requalify first-party Eggstack dependencies within already-supported compatibility lines;
5. reconcile committed lockfiles where the same first-party runtime dependency is intended to be shared;
6. evaluate and, if behavior/API compatible, remove the direct `socket2 0.5` duplication by adopting `0.6`;
7. add a mechanical first-party dependency/lock coherence check;
8. pin every third-party GitHub Action used by CI/release/security workflows to a full 40-character commit SHA with a human-readable release comment;
9. encode least-privilege GitHub Actions permissions and disable checkout credential persistence where no authenticated Git operation follows;
10. add scheduled Cargo dependency-security coverage for every committed Cargo lockfile;
11. add Dependabot coverage for each independent Cargo workspace, GitHub Actions, TypeScript/npm, and pinned CI Python tooling;
12. add PR dependency review with an explicit failure policy;
13. make JavaScript and Python CI tool installation deterministic enough that the lock/constraint file, rather than a floating registry query, owns the tested graph;
14. narrow the PyO3 unsafe exception so safe implementation code is compiler-enforced as unsafe-free;
15. add an automated Rust public-API regression gate relative to the M059 activation baseline;
16. add a repository security-reporting policy;
17. add release-binary provenance attestations if the repository/account capability is available and verification can be made blocking on the qualified release workflow;
18. re-run the full exact-head compatibility/security/release qualification required to unblock M058.

### Non-goals

- no native `/v1` operation addition/removal/rename;
- no config schema change;
- no CLI command removal or argument/output contract change;
- no fault semantic, RNG, seed-derivation, queue, timing, scenario, evidence, or replay change;
- no Toxiproxy strict-v2.12 or post-v2.12 behavior change;
- no Eggfetch integration contract change;
- no change to MSRV 1.89;
- no `eggserve-server 0.4.x` migration in this milestone;
- no new transport, listener, forward-proxy, TLS-admin, or authentication feature;
- no generic C ABI, Node native addon, JNI, P/Invoke, cgo, UniFFI, or WASM work;
- no broad refactor of `eggchaos-core::engine` / `stream`, `runtime/control.rs`, Toxiproxy, or CLI solely to reduce line count;
- no performance threshold retuning;
- no tag, crates.io/PyPI/npm publication, or GitHub Release creation;
- no rewrite of M055–M057 closure evidence.

M052 already audited private module boundaries. Large-file size alone is not sufficient justification for another pre-release decomposition pass after the recent correctness work.

## Affected surfaces

Expected implementation surfaces include:

- root `Cargo.toml` and `Cargo.lock`;
- all eight workspace member `Cargo.toml` files;
- `bindings/python-native/Cargo.toml`, `Cargo.lock`, and source organization;
- `benchmarks/Cargo.lock` and `fuzz/Cargo.lock`;
- `.github/workflows/ci.yml`;
- `.github/workflows/release.yml`;
- a new scheduled security/dependency workflow and PR dependency-review workflow, or a deliberately consolidated equivalent;
- `.github/dependabot.yml`;
- pinned Python CI requirements/constraints files if introduced;
- TypeScript check/qualification scripts;
- dependency/lint/action/API structural regression scripts under `scripts/tests/` or focused companion scripts;
- `SECURITY.md`;
- release/tooling and verification architecture docs;
- M058 release-plan wording where M059 changes the qualified release contract;
- planning registry/current-state projections;
- `plans/closure/M059-pre-v0-2-0-security-dependency-and-maintenance-hardening-corrective-closure.md` at closure.

## Ordered work packages

### WP1 — Freeze and prove the current security baseline

Before dependency/workflow edits:

1. record activation HEAD `0f8a8ebc8b8f734951624be32362ee512b9e3d5e`;
2. run the ordinary local gate and native admin auth regressions;
3. explicitly prove:
   - non-loopback + `public_admin=true` + missing token is rejected;
   - an empty token is rejected;
   - missing/malformed/wrong Bearer authorization is rejected;
   - a correct non-empty token succeeds;
   - token material remains redacted from Debug/error output;
4. record the current `constant_time_equal` fixed-width digest implementation as the baseline;
5. verify the four lockfiles do not contain the malicious/deleted crate versions listed by the Rust Security Response Team's 2026-08-20 incident;
6. inventory every `uses:` reference and workflow permission scope;
7. capture direct and duplicate dependency trees before changes.

Do not “improve” native auth semantics during this work package. The purpose is to make sure later maintenance changes cannot regress the already-landed correctness fixes.

### WP2 — Activate workspace lints without changing behavior

Add:

```toml
[lints]
workspace = true
```

to each of the eight normal workspace member manifests.

Then run:

```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --all-features --no-deps
```

Repair newly exposed warnings by preference order:

1. documentation or local expression cleanup;
2. narrowly scoped lint allowance with a comment when the existing API/compatibility contract intentionally conflicts with a lint;
3. no crate-wide new allow unless the plan records why it is unavoidable.

Do not rename public items, change wire spelling, remove compatibility aliases, or alter runtime control flow merely to satisfy pedantic style.

Add a cheap structural regression that enumerates workspace members and fails if a future ordinary member omits workspace lint inheritance.

### WP3 — Reconcile dependency declarations and committed graphs

#### Root declaration cleanup

Remove the six proven-unused root workspace dependency declarations:

- `http-body-util`;
- `hyper`;
- `hyper-util`;
- `prometheus-client`;
- `rand`;
- `url`.

Removal here means only the unused root declaration. Do not remove a dependency actually referenced by a package or standalone workspace.

#### First-party dependency floor updates

Qualify the current already-compatible first-party releases, with the audit baseline targets:

- `eggress-relay 1.0.11`;
- `eggfetch-core 0.2.1`;
- `eggserve-primitives 0.2.2`;
- latest published `eggserve-server 0.2.x` compatible with the current API (the committed standalone locks already demonstrate `0.2.1`).

Update the root minimum requirements only after source/API review and focused tests show compatibility.

**Do not migrate to `eggserve-server 0.4.x` in M059.** That is a separate compatibility-line migration with a larger server-runtime audit surface.

Re-resolve all four committed Cargo lockfiles so first-party runtime packages converge where their manifest constraints permit.

#### Duplicate direct dependency review

Test `socket2 0.6` for the Eggchaos direct server usage. Adopt it if:

- the server compiles on all supported targets;
- reset/socket option semantics are unchanged;
- focused transport tests pass;
- it removes the unnecessary `0.5` direct branch.

If any required behavior differs, retain `0.5` and record the exact incompatibility in closure evidence instead of forcing deduplication.

#### Coherence guard

Add a stdlib-only check that fails when the committed root/native/benchmark/fuzz lockfiles resolve different versions of the first-party Eggstack runtime crates that M059 intentionally synchronizes. Keep the checked set narrow; do not require whole-lockfile identity across workspaces with different purposes.

Wire the guard into `scripts/check.sh` and one independent CI location so removing it from the local aggregate is detectable.

### WP4 — Harden GitHub Actions and dependency monitoring

#### Immutable actions

For every external `uses:` in `.github/workflows/*.yml`:

- resolve a reviewed upstream release to its **full 40-character commit SHA**;
- use the full SHA in YAML;
- retain the human-readable release/tag as a trailing comment;
- never use a branch, moving major tag, short SHA, or forked replacement.

Upgrade old action lines only when needed to reach a currently supported/security-maintained release. At registration time, upstream first-party actions have moved to newer release lines (for example checkout/setup Python/setup Node are on v7-era releases). Review each major's release notes before selecting it; preserve Eggchaos workflow semantics.

`dtolnay/rust-toolchain` must likewise be pinned by commit while the requested project toolchain remains exactly `1.89.0`.

Add a structural test that rejects non-local `uses:` entries not matching a full SHA.

#### Permissions and credentials

Set a least-privilege default such as:

```yaml
permissions:
  contents: read
```

for ordinary CI/release qualification.

Where checkout is followed only by reads/builds, set `persist-credentials: false`.

If release provenance attestation is enabled, grant only the artifact job the additional scopes it needs (normally `id-token: write` and `attestations: write`), not the whole workflow.

Do not introduce `pull_request_target` or another privileged untrusted-code trigger.

#### Scheduled security workflow

Add a bounded Linux-only daily scheduled job (plus manual dispatch) that at minimum:

- audits root `Cargo.lock`;
- audits `bindings/python-native/Cargo.lock`;
- audits `benchmarks/Cargo.lock`;
- audits `fuzz/Cargo.lock`;
- runs the repository's cargo-deny policy against each production/release-relevant manifest where the tool supports it;
- runs the first-party lock-coherence guard;
- fails, rather than merely annotates, on an advisory covered by existing policy.

Reuse pinned `cargo-audit` / `cargo-deny` tool versions. The scheduled job is an additional detection path; it does not replace push/PR security gates.

#### Dependency review and Dependabot

Add PR dependency review with `contents: read` and an explicit vulnerability failure threshold. Keep license authority in `cargo-deny` unless duplication has a demonstrated benefit.

Configure Dependabot for:

- Cargo root workspace;
- `/bindings/python-native`;
- `/benchmarks`;
- `/fuzz`;
- GitHub Actions;
- `/bindings/typescript-client` npm dependencies;
- the directory containing any pinned Python CI requirements introduced by M059.

Prefer grouped weekly updates with bounded open-PR counts to avoid one-PR-per-transitive-noise patterns.

### WP5 — Make language-tool CI reproducible and narrow the Python FFI unsafe exception

#### TypeScript

Replace conditional `npm install` setup with a lockfile-enforcing path such as:

```sh
npm ci --ignore-scripts --no-audit --no-fund
```

before explicit typecheck/build/test commands.

The committed `package-lock.json` remains the install authority. Lifecycle scripts must not run implicitly during dependency installation.

#### Python CI tooling

Move floating workflow installs such as `build`, `pytest`, and `pyyaml` into exact-version CI requirement/constraint files. Keep `maturin` exact. Use those files in the workflows and let Dependabot update them.

Do not add runtime dependencies to the stdlib-only Python remote client.

Where supported, make standalone Cargo/maturin qualification honor committed lockfiles explicitly.

#### PyO3 unsafe boundary

Replace the current “whole binding crate may contain macro-generated unsafe” stance with the narrowest compiler-enforceable boundary that PyO3 supports:

1. first try item/module-scoped `allow(unsafe_code)` only on the PyO3 macro-facing layer;
2. move pure conversion, validation, and lifecycle helper code into modules carrying `#![deny(unsafe_code)]`;
3. if PyO3 expansion still requires a crate-root allowance, retain it only as a framework compatibility necessity while ensuring every ordinary implementation module denies unsafe;
4. preserve the existing handwritten-unsafe source audit as a secondary guard;
5. add a structural regression proving safe modules keep `deny(unsafe_code)`.

No Python-visible class, method, exception hierarchy, abi3 floor, or lifecycle behavior may change.

### WP6 — Add a Rust public-API regression gate

Use `cargo-semver-checks` (current researched tool release at registration: 0.50.0) from a dedicated Linux qualification job running on a current stable toolchain, not the project's 1.89 MSRV compiler.

Use two complementary baselines:

1. **Qualified compatibility baseline:** `818e5674f2efaf96ec8effda81cef1dfa7a48614` (M057). The final M059 candidate must be compared to this revision so public-surface drift introduced by the eight post-M057 commits is not grandfathered merely because it predates M059 registration.
2. **Activation snapshot:** `0f8a8ebc8b8f734951624be32362ee512b9e3d5e`. Preserve all public API/capability present here unless restoring an M057-compatible signature requires an additive compatibility wrapper or alias.

Run the check with a no-breaking-change policy for every public library crate:

- `eggchaos-core`;
- `eggchaos-experiment`;
- `eggchaos-protocol`;
- `eggchaos-server`;
- `eggchaos-toxiproxy`;
- `eggchaos-eggfetch`;
- `eggchaos-embed`.

Use all supported features unless the tool demonstrates a false result caused solely by an intentionally non-public feature; any exclusion must be documented narrowly.

`eggchaos-cli` is binary-only and is covered by CLI contract/command tests rather than a library API diff.

The API gate is additive evidence. It does not authorize changing package versions or declaring a breaking release.

First run the M057-to-activation comparison as a diagnostic census. For every reported breaking change, classify it as:

- an unintended API regression that must be restored compatibly before M059 can close;
- a false positive caused by tooling/feature topology, with a narrow reproducible explanation; or
- a change that cannot be restored without contradicting a correctness/security invariant, which triggers the stop condition and requires a separate explicit compatibility decision.

Do not suppress a real break globally. Prefer additive compatibility wrappers/re-exports while routing internal behavior through corrected implementations.

After `v0.2.0` is published, a future plan may move the qualified compatibility baseline to the immutable `v0.2.0` tag.

### WP7 — Add security reporting and release artifact provenance

Add `SECURITY.md` describing:

- currently supported release line(s);
- private vulnerability reporting path;
- what information is useful in a report;
- explicit instruction not to publish credentials, exploit payloads, or sensitive details in a public issue.

Do not invent an email address or organization team that does not exist. Prefer GitHub private vulnerability reporting/security advisories when repository settings support it.

For release binaries, add GitHub artifact provenance attestations if the repository supports them:

- attest the exact binary produced by each release artifact matrix leg;
- keep the existing SHA-256 sidecar contract;
- pin the attestation action by full SHA;
- grant attestation/OIDC permissions only to the artifact job;
- add hosted verification of at least one native-host binary using GitHub's attestation verification path;
- do not substitute attestation for checksum validation.

If repository/account policy makes attestations unavailable, closure must record the capability limitation and M058 must retain checksum-only release semantics rather than fabricating provenance evidence.

### WP8 — Exact-head qualification and M058 reactivation

Because M059 changes dependency graphs and CI/release workflow security, M057 remains historical evidence for the M056 DAG but is no longer sufficient as the final `v0.2.0` candidate qualification.

On the exact M059 candidate run at minimum:

```sh
python3 scripts/check_planning_state.py --check
python3 scripts/check_version_coherence.py --check
sh scripts/tests/test_release_tag_version.sh
./scripts/check.sh
./scripts/check_openapi.sh
./scripts/check_python_client.sh
./scripts/check_typescript_client.sh
./scripts/check_python_native.sh
./scripts/qualify_language_clients.sh
./scripts/qualify_python_native.sh
./scripts/qualify_eggfetch.sh
./scripts/release-smoke.sh
./scripts/release-artifact-smoke.sh
EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh
```

Mandatory compatibility oracles:

```sh
TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" \
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 \
  ./scripts/qualify_toxiproxy_v2_12.sh

TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh)" \
  EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1 \
  ./scripts/qualify_toxiproxy_post_v2_12.sh
```

Additionally require:

- workspace-lint inheritance structural test green;
- first-party lock-coherence test green;
- action-SHA/permissions structural test green;
- scheduled-security workflow parses and its commands pass manually on the exact candidate;
- dependency-review workflow parses and is runnable for a PR;
- Rust API regression gate green against the activation SHA;
- normal hosted CI green on the exact candidate;
- release `workflow_dispatch` green on the exact candidate after action/dependency/provenance changes;
- if attestations are enabled, hosted attestation generation and verification green;
- no publication/tag/GitHub Release action occurred.

Only after the M059 closure record contains that evidence may `plans/registry.md` move M058 back to `ready`.

## Invariants

M059 must preserve:

- Rust MSRV 1.89;
- workspace version `0.2.0`;
- all seven public Rust library crate APIs present at the activation baseline;
- the `eggchaos` binary name and CLI command/capability inventory;
- native `/v1` route/DTO/OpenAPI contract;
- config schema v1;
- all native operation inventory entries;
- stream/datagram deterministic RNG and evidence semantics;
- Scenario V1 and V2 semantics and isolation;
- strict Toxiproxy v2.12 default and its pinned differential corpus;
- opt-in post-v2.12 `packet_loss` profile;
- Eggfetch Dialer ownership/composition contract;
- Python/TypeScript remote client behavior;
- Python-native public surface and abi3 policy;
- five release binary target triples;
- release tag/version gate behavior from M056/M057;
- M047 provenance schema and M048 provenance CI ownership;
- bounded queues, bodies, histories, connection/association registries, and metric cardinality.

## Failure semantics

Security/maintenance automation must fail closed in the following ways:

- an unpinned external GitHub Action fails the structural check;
- an action unexpectedly requiring broader permissions blocks the change until the scope is justified;
- a dependency advisory covered by policy fails scheduled/PR qualification;
- lockfile first-party drift fails the coherence guard;
- inherited lint warnings fail under the existing `-D warnings` gate;
- a public Rust API removal/change detected against the activation baseline fails qualification;
- a dependency update that changes runtime/API semantics is reverted or split into a separately planned migration;
- a PyO3 safety-boundary change that requires handwritten unsafe stops and requires a separate ADR;
- missing artifact-attestation capability is recorded as unavailable rather than downgraded to a false pass.

A transient external advisory/registry outage should be recorded distinctly from a clean security result. Do not weaken severity/policy merely to make a red external gate green.

## Acceptance criteria

M059 may close only when:

1. all eight ordinary workspace members inherit root lint policy;
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings` is green with no broad new suppression hiding real findings;
3. the six unused workspace dependency declarations are removed;
4. first-party dependency updates are qualified within existing compatibility lines;
5. all four committed Cargo lockfiles are reconciled for the selected first-party runtime versions;
6. `eggserve-server 0.4.x` remains deferred unless M059 is explicitly replaced by a migration plan;
7. direct `socket2 0.5` is either removed in favor of 0.6 with green transport qualification or retained with a concrete recorded incompatibility;
8. a first-party lock-coherence regression runs locally and independently in CI;
9. every external GitHub Action in owned workflows is pinned to a full commit SHA;
10. ordinary workflow token permissions are explicitly least-privilege;
11. checkout credential persistence is disabled where no authenticated Git operation follows;
12. scheduled dependency security covers all committed Cargo lockfiles;
13. Dependabot covers all four Cargo workspaces, Actions, npm, and pinned Python CI tooling;
14. PR dependency review is active or a repository-capability blocker is explicitly recorded;
15. TypeScript CI uses the committed npm lockfile deterministically;
16. Python CI tool versions are exact and centrally maintained;
17. ordinary Python-native implementation code is compiler-enforced unsafe-free outside the minimum PyO3 macro boundary;
18. the handwritten-unsafe audit remains green;
19. Rust public-API comparison is green against M057 candidate `818e5674f2efaf96ec8effda81cef1dfa7a48614`, every M057-to-activation finding is classified, and the final candidate does not regress either the qualified M057 surface or additive API/capability present at activation `0f8a8eb`;
20. `SECURITY.md` exists and names a real private reporting mechanism without invented contact data;
21. release artifact attestations are generated/verified when supported, or the unsupported capability is explicitly recorded without weakening checksum requirements;
22. OpenAPI/SDK/native binding drift checks are green;
23. both mandatory Toxiproxy oracle qualifications are complete and green;
24. Eggfetch, fuzz, release/package/artifact, language-client, and Python-native qualification are green;
25. normal hosted CI is green on the exact M059 candidate;
26. release workflow dispatch is green on the same candidate after workflow hardening;
27. no tag or registry/GitHub release publication occurred;
28. M055–M057 closure evidence is untouched;
29. closure evidence records exact dependency versions, action SHAs, workflow run IDs, API baseline/result, security-scan results, and any explicitly retained exception;
30. registry/current-state projections mark M059 closed and M058 ready only after all preceding criteria are evidenced.

## Stop conditions

Stop and split/re-plan if:

- adopting a first-party dependency requires changing a public Eggchaos API or deterministic/runtime behavior;
- EggServe 0.4 becomes necessary to resolve a security issue, because that is a separate compatibility-line migration;
- inherited lints reveal a fix that would require API removal/rename rather than a narrow documented lint exception;
- public API checking requires nightly-only behavior that cannot be isolated from the MSRV build;
- action pinning/upgrades require privileged workflow triggers or broad write permissions;
- PyO3 cannot be isolated without handwritten unsafe or a user-visible binding redesign;
- dependency-review/attestation features are unavailable in repository settings and making them available requires an organization-policy change outside repository scope;
- any mandatory pinned Toxiproxy oracle fails;
- exact-head release qualification exposes a production correctness defect.

A production correctness defect discovered here blocks M058 and should receive a focused corrective successor rather than being hidden inside dependency/security cleanup.

## Closure evidence

Create:

`plans/closure/M059-pre-v0-2-0-security-dependency-and-maintenance-hardening-corrective-closure.md`

Record at minimum:

- activation SHA and exact closure candidate;
- before/after direct first-party dependency table;
- all four before/after lockfile first-party versions;
- unused dependency declarations removed;
- `socket2` disposition and transport evidence;
- workspace lint inheritance census and any intentionally scoped lint allowances;
- external action name -> reviewed release -> full pinned SHA mapping;
- workflow permissions matrix;
- scheduled-security and dependency-review behavior;
- Dependabot ecosystem/directory matrix;
- Python/npm deterministic-install evidence;
- PyO3 unsafe-boundary before/after map plus handwritten-unsafe audit;
- cargo-semver-checks version, M057 qualified baseline SHA, activation snapshot SHA, per-package M057-to-activation finding census/disposition, and final-candidate results;
- malicious-crate incident lockfile census result;
- `SECURITY.md` reporting path;
- artifact-attestation support/disposition and verification output;
- local full-gate results;
- strict/post-v2.12 mandatory oracle results;
- hosted ordinary CI run ID/job census;
- release workflow-dispatch run ID/job census;
- confirmation that no release/tag/publication occurred;
- confirmation that M055–M057 historical closure records were not rewritten;
- final statement reactivating M058 only if every release-blocking criterion passed.

## Follow-on rule

A clean M059 closure reactivates M058 and nothing else.

M058 then owns the irreversible `v0.2.0` tag, crates.io publication graph, GitHub Release, qualified binary/checksum distribution, optional language-registry go/no-go decisions, and fresh public-install verification.

If M059 cannot close cleanly, M058 remains blocked. Do not tag or publish `v0.2.0` from M057 or any earlier candidate.
