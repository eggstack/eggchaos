# M062 — v0.2.1 Patch Release

Status: closed (published as `v0.2.1`)

Closure evidence: `plans/closure/M062-v0-2-1-patch-release-closure.md`.

## Objective

Prepare, qualify, and publish Eggchaos `v0.2.1` from one exact `main`
candidate. Align first-party package metadata to `0.2.1`, update the direct
EggServe primitives requirement to the current published release, refresh
current documentation, then publish the eight Rust workspace crates and the
qualified CLI binaries through the repository's established release process.

## Baseline and dependencies

- Baseline: `96c7edae78f7dd14ef61b12f72f2a1b5805cdb9d`, after M061 and the
  consolidated Dependabot PR #72.
- Published baseline: M058's immutable `v0.2.0` tag and publication.
- Release hardening and workflow authorities: M059 and M056/M057; no release
  workflow redesign is in scope.
- As checked on 2026-11-13 with Cargo's registry search, current used
  Eggstack crates are `eggress-relay 1.0.12`, `eggfetch-core 0.3.0`,
  `eggserve-server 0.5.0`, and `eggserve-primitives 0.2.3`. Eggchaos uses the
  relay crate only from Eggress; it does not depend on the broader Eggress
  proxy/runtime crates. The workspace already resolves those four versions;
  the direct `eggserve-primitives` manifest requirement is the one stale
  requirement and must move from `0.2.2` to `0.2.3`.
- CI, release qualification, and crates.io credentials are available through
  the repository's configured GitHub and Cargo authentication.

## Scope and non-goals

In scope:

- Bump Rust workspace crates, the standalone native crate metadata, internal
  benchmark metadata, and aligned Python/TypeScript source metadata to
  `0.2.1`; update all first-party local dependency requirements and lockfiles.
- Keep the four used Eggstack dependencies at their current versions and make
  the EggServe primitives floor explicit at `0.2.3`.
- Update the release artifact workflow from GitHub's retired `macos-14`
  label to `macos-15`, keeping the same Apple target triples and artifact
  contracts.
- Add `v0.2.1` release notes and update current documentation after publication
  with the actual candidate, CI, tag, registry, binary, and checksum evidence.
- Push the candidate, require ordinary hosted CI green on its exact SHA, push
  an annotated `v0.2.1` tag, require the tag-triggered qualification and all
  five artifact targets green, publish all eight Rust crates in dependency
  order, and create the GitHub Release from the qualified uploaded artifacts.
- Verify fresh installation of the published CLI and close this plan with
  exact evidence.

Non-goals:

- Runtime, wire/API, configuration schema, RNG, deterministic semantics, or
  OpenAPI changes. The release workflow's DAG, publication permissions,
  target triples, artifact names, checksum format, and attestation contract
  remain unchanged.
- PyPI, npm, or `eggchaos-native` publication; those remain deferred under the
  M058 package-specific release decision.
- Moving or recreating any existing release tag, or rebuilding release assets
  locally for attachment.

## Affected crates and surfaces

- Eight workspace crates: `eggchaos-core`, `eggchaos-experiment`,
  `eggchaos-protocol`, `eggchaos-server`, `eggchaos-cli`,
  `eggchaos-toxiproxy`, `eggchaos-eggfetch`, and `eggchaos-embed`.
- `benchmarks/`, `fuzz/`, and standalone `bindings/python-native/` manifests
  and lockfiles; aligned Python and TypeScript client package metadata.
- `Cargo.toml`, `Cargo.lock`, release notes, `SECURITY.md`, release workflow,
  architecture and planning current-state documents, the release-state docs
  drift guard and regression test, and the release closure record.

## Ordered work packages

### WP1 — Prepare metadata and release documentation

Set every repository-aligned package version and local Eggchaos registry
requirement to `0.2.1`; set `eggserve-primitives` to `0.2.3`; refresh the four
Cargo lockfiles without gratuitous third-party updates. Add user-facing patch
release notes. Keep historical `v0.2.0` evidence unchanged.

### WP2 — Qualify the exact candidate

Run version, planning, lockfile, release-state, OpenAPI, and package-content
guards locally. Push the candidate and require the ordinary CI workflow to
finish green with the run's `head_sha` exactly equal to the candidate. Do not
tag on a red, incomplete, or mismatched run.

### WP3 — Dispatch pre-tag qualification, tag, and qualify release artifacts

Dispatch the release workflow on the green `main` candidate and require
`release-contract`, `qualify`, and all five binary artifact targets to pass
with the exact candidate SHA. These pre-tag artifacts are qualification
evidence and will not be attached to the release. Then confirm `v0.2.1` is
absent locally and remotely, create and push an annotated tag directly on the
green candidate, and require the tag-triggered workflow's `release-contract`,
`qualify`, and all five artifact legs to pass. Download only the tag-run
artifacts and verify each binary/checksum pair and filename. Do not create a
GitHub Release or publish crates while either release run is red or incomplete.

### WP4 — Publish the Rust crate graph

Publish with `cargo publish --locked` in this order, waiting for each exact
version to resolve and verifying it before its dependents:

1. `eggchaos-core`
2. `eggchaos-experiment`
3. `eggchaos-eggfetch`
4. `eggchaos-protocol`
5. `eggchaos-server`
6. `eggchaos-toxiproxy`
7. `eggchaos-cli`
8. `eggchaos-embed`

If publication fails part-way, stop and resume only after diagnosing the
immutable registry state. Never attempt to overwrite or casually yank a
healthy published version.

### WP5 — Create GitHub Release and verify fresh installs

Create `eggchaos v0.2.1` from the pushed tag and attach only the five binary
and five checksum artifacts produced by the successful tag workflow. Confirm
all eight crate versions are visible on crates.io and run a clean-cache
`cargo install eggchaos-cli --version 0.2.1 --locked` smoke. Record the
published tag object/target, hosted run IDs, artifact checksums, publish order,
fresh-install result, and deferred language packages.

### WP6 — Reconcile current documentation and close

Update live release-state documents to name `v0.2.1` as current, preserve
`v0.2.0` as history, update `SECURITY.md`, dependency-version prose, and
planning projections, then add the closure record and close M062. Run the
planning and release-state drift guards and require hosted CI green on the
documentation/closure candidate.

## Invariants and failure semantics

- All first-party Rust and aligned language-package source metadata must agree
  at `0.2.1`; local Rust package requirements must resolve to `^0.2.1`.
- The public contracts and runtime behavior remain unchanged from the
  qualified source. Tests may change only when required to resolve a
  dependency-compatible compile/regression issue, with the change explained.
- CI and release-workflow evidence must match the exact SHAs being claimed.
- The release tag is annotated, immutable, and points directly to the green
  candidate. Stop before publication if its target is wrong.
- Publish in the documented dependency order; dependents wait for registry
  visibility of their predecessors.
- GitHub Release assets come from the tag-triggered workflow, with checksums
  validated before attachment.
- If any quality, compatibility, registry, or publication gate fails, stop
  the dependent release steps, fix or diagnose the failure, and requalify the
  resulting exact candidate.

## Verification commands

```sh
python3 scripts/check_version_coherence.py --check
python3 scripts/check_lock_coherence.py --check
python3 scripts/check_planning_state.py --check
python3 scripts/check_release_state_docs.py --check
./scripts/check_openapi.sh
cargo package --list -p eggchaos-core
cargo package --list -p eggchaos-experiment
cargo package --list -p eggchaos-protocol
cargo package --list -p eggchaos-server
cargo package --list -p eggchaos-cli
cargo package --list -p eggchaos-toxiproxy
cargo package --list -p eggchaos-eggfetch
cargo package --list -p eggchaos-embed
./scripts/check.sh
./scripts/release-smoke.sh
./scripts/benchmark_datagram.sh
EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh
TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" \
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 \
  ./scripts/qualify_toxiproxy_v2_12.sh
./scripts/qualify_eggfetch.sh
./scripts/release-artifact-smoke.sh
```

Hosted ordinary CI must be green on the exact candidate. The tag-triggered
release workflow must pass `release-contract`, `qualify`, and all five
artifact legs before publication. Hosted oracles that cannot run are
incomplete evidence and do not count as passes.

## Acceptance criteria

1. All first-party package metadata and local requirements are coherent at
   `0.2.1`, with all four lockfiles coherent and no unnecessary external
   dependency drift.
2. The used Eggstack dependencies are at the current selected releases listed
   in the research baseline; no unused Eggress runtime crate is introduced.
3. Release notes and current docs accurately distinguish the published
   `v0.2.1` release, its immutable `v0.2.0` predecessor, and deferred language
   registries.
4. Exact-candidate hosted CI, pre-tag dispatch, and tag-triggered release
   workflow are green; five tag-run binary/checksum pairs verify and are
   attached to the GitHub Release.
5. All eight Rust crates are visible at `0.2.1` on crates.io in dependency
   order, and a clean-cache CLI install/version smoke succeeds.
6. M062 has a closure record with exact candidate/tag SHAs, hosted evidence,
   package/artifact evidence, checksums, command results, limitations, and no
   unresolved release blockers. Registry/header/generated projections agree.

## Stop conditions

- Ordinary CI is not green on the exact candidate.
- Pre-tag release dispatch is not green on the exact candidate.
- The tag already exists, has a mismatched target, or tag qualification is
  red/incomplete.
- Any checksum, attestation, package graph, API, dependency, or fresh-install
  check fails.
- A crate publication becomes partially visible and the remaining graph
  cannot be safely resumed from the registry state.
- Evidence is unavailable for a mandatory release gate.

## Closure evidence

Record in `plans/closure/M062-v0-2-1-patch-release-closure.md` after
publication: candidate and closure SHAs; ordinary CI and tag-workflow run IDs
and exact-head status; tag object and peeled target; all artifact names and
SHA-256 values; crates.io publication order and visibility; fresh-install
result; documentation/planning guards; deferred surfaces; final verdict.

## Follow-on

M062 activates no automatic successor. Any runtime, compatibility, security,
dependency, or new release work requires a separate scoped plan. A defect in
an already-published `0.2.1` artifact must be corrected in a new patch release;
the tag or immutable registry package must not be rewritten.
