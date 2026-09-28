# M055 — Post-v0.1.0 Release-State Reconciliation and v0.2.0 Development Baseline

Status: ready
Depends on: M054 closed
Role: release-state/version-coherence corrective and next-release development baseline
Registration baseline: ea12798e44d468dac1bd9e88ccde52f9dbaeac7e

## Objective

Reconcile eggchaos's repository/package/release state after the published
`v0.1.0` release and the substantial post-v0.1.0 implementation that is now
present on `main`.

The repository currently has two simultaneously true facts that must be made
explicit rather than conflated:

1. `v0.1.0` is a real published historical release, tagged from commit
   `81994dbc427365f1dfdaabfa39bdf077850e69ec` on 2026-09-24, with GitHub
   release assets and the original Rust package publication lineage.
2. current `main` contains the post-v0.1.0 feature/corrective tranches through
   M054 but still reports package version `0.1.0` across the Rust workspace,
   first-party language packages, internal dependency requirements, and some
   documentation, while `architecture/overview.md` still calls the repository
   "Pre-release 0.1.0".

M055 establishes `0.2.0` as the **unreleased development baseline** for the
post-v0.1.0 tree, makes version metadata coherent across first-party packages,
and adds mechanical version/tag coherence checks so a future release cannot
silently publish artifacts whose package metadata disagrees with the tag.

M055 does **not** create a `v0.2.0` tag, publish crates, publish Python/npm
packages, upload release assets, or create a GitHub release. Those remain
explicit owner-controlled release actions after this milestone closes.

## Historical release baseline

The implementation must preserve the following historical facts:

- annotated tag `v0.1.0` resolves to commit
  `81994dbc427365f1dfdaabfa39bdf077850e69ec`;
- GitHub release `eggchaos v0.1.0` was published on 2026-09-24;
- the v0.1.0 release carries five target binaries plus SHA-256 sidecars;
- M019 remains the final pre-tag qualification authority for the v0.1.0
  release line;
- later milestones M020-M054 are post-v0.1.0 development and must not be
  retroactively described as part of the v0.1.0 release candidate.

Do not move, rewrite, or recreate the `v0.1.0` tag or release.

## Version decision

M055 freezes the next development package version as **0.2.0**.

Rationale:

- `0.1.0` is already published and immutable as a historical release;
- post-release development added material additive capability, including
  datagram chaos, Scenario V2, consumer-neutral experiment/integration
  surfaces, remote SDKs/native Python embedding, post-v2.12 stream-loss
  support, and subsequent corrective/performance work;
- this is materially larger than a patch-only maintenance delta;
- package versioning is independent from the stable native HTTP path
  `/v1`, config schema version 1, RNG version 1, provenance schema version 1,
  and Toxiproxy profile names. M055 must not rename any of those contracts.

The implementation must use plain `0.2.0` package metadata rather than a
Cargo prerelease string such as `0.2.0-dev`; repository prose must make clear
that it is an unreleased development baseline until publication occurs.

## Scope

### In scope

- workspace package version `0.1.0 -> 0.2.0`;
- every intra-workspace Rust dependency requirement
  `version = "0.1.0" -> version = "0.2.0"`;
- `Cargo.lock` regeneration/verification;
- first-party language package metadata:
  - `bindings/python-client/pyproject.toml`;
  - `bindings/typescript-client/package.json` and lockfile if present;
  - `bindings/python-native/pyproject.toml`;
  - `bindings/python-native/Cargo.toml`, including local eggchaos dependency
    requirements;
- any generated/package metadata that intentionally mirrors the repository
  release version;
- `eggchaos version` / native version-reporting tests insofar as they derive
  from `CARGO_PKG_VERSION`;
- README/release documentation that currently conflates the published
  v0.1.0 release with current `main`;
- a stdlib-only or shell-only version-coherence check covering Rust and
  first-party language manifests;
- tag-to-package-version validation in the release workflow;
- release-smoke/order-proof updates so the publish graph is checked against
  the new workspace version rather than stale hard-coded 0.1.0 assumptions;
- planning/architecture/tooling-distribution reconciliation;
- exact-candidate qualification and closure evidence.

### Non-goals

- no `v0.2.0` tag creation;
- no crates.io/PyPI/npm publication;
- no GitHub release creation or asset upload;
- no API, route, CLI, config-schema, RNG, deterministic, or fault-semantic
  change;
- no native `/v2` control API;
- no Toxiproxy compatibility change;
- no dependency upgrade merely because a release version changes;
- no branch-protection/ruleset change;
- no scenario-enumeration or egress-outbound feature work;
- no rewrite of v0.1.0 closure evidence or release artifacts.

## Affected surfaces

Expected primary files:

- `Cargo.toml`;
- `Cargo.lock`;
- `crates/*/Cargo.toml` for intra-workspace dependency requirements;
- `bindings/python-client/pyproject.toml`;
- `bindings/typescript-client/package.json` and lockfile if present;
- `bindings/python-native/pyproject.toml`;
- `bindings/python-native/Cargo.toml`;
- `README.md`;
- `.github/workflows/release.yml`;
- `scripts/release-smoke.sh`;
- a new `scripts/check_version_coherence.py` (preferred) plus focused test
  fixture, or an equivalently small stdlib/shell implementation;
- `architecture/overview.md`;
- `architecture/tooling-distribution.md`;
- `plans/registry.md`, `plans/README.md`, `plans/roadmap.md`,
  `AGENTS.md`;
- closure evidence under `plans/closure/`.

Do not mechanically replace every textual occurrence of `0.1.0`: historical
release references must remain `v0.1.0`.

## Ordered work packages

### WP1 — Freeze the v0.1.0 historical lineage and classify version-bearing text

Before changing metadata:

1. resolve and record the `v0.1.0` annotated tag target;
2. record the published GitHub release identity/date/assets;
3. inventory every `0.1.0` occurrence and classify it as:
   - historical v0.1.0 fact;
   - current package/dependency metadata;
   - stable-install documentation;
   - stale current-main/release-state prose;
   - test fixture whose value is intentionally version-specific;
4. preserve historical facts unchanged.

This classification prevents an unsafe global search/replace.

### WP2 — Establish the 0.2.0 Rust workspace baseline

Set `[workspace.package].version = "0.2.0"` and update every local
eggchaos dependency requirement to `0.2.0`.

Requirements:

- all eight workspace crates resolve to `0.2.0`;
- path dependencies still include a registry version requirement for ordered
  publication;
- publish order remains
  `core -> experiment/eggfetch -> protocol -> server/toxiproxy/cli -> embed`;
- `Cargo.lock` records the local package versions coherently;
- no external dependency version changes are bundled into this work.

Add a regression proving there is no remaining current intra-workspace
`version = "0.1.0"` requirement.

### WP3 — Align first-party language package metadata

Set the repository-aligned development version to `0.2.0` for:

- Python remote client;
- TypeScript remote client;
- Python-native package;
- standalone Python-native Rust crate and its local eggchaos dependency
  requirements.

If any package intentionally has an independent release cadence, stop and
document that decision rather than silently forcing alignment. At registration
time there is no recorded independent-version policy, so aligned `0.2.0`
metadata is the expected implementation.

Do not change package names, Python ABI target, Node API shape, generated
operation inventory, or native-control behavior.

### WP4 — Make stable-release vs development-main documentation explicit

Reconcile documentation so users cannot mistake current `main` for the
published v0.1.0 artifact.

Required presentation:

- v0.1.0 remains described as the current published historical release until
  an owner actually publishes a successor;
- current `main` is described as the unreleased 0.2.0 development baseline;
- `architecture/overview.md` must no longer say "Pre-release 0.1.0";
- M019 must be described specifically as the final **v0.1.0 pre-tag**
  authority, not as a timeless final pre-tag authority;
- README installation examples must not tell registry users to install an
  unpublished `0.2.0`.

Preferred README shape while 0.2.0 is unpublished:

- "Latest published release: 0.1.0" with the existing registry install
  commands;
- a separate "Development/main" source-build path for the 0.2.0 tree.

After a future owner publication, changing the stable-install commands to
0.2.0 is a separate release-closeout action.

### WP5 — Add one version-coherence authority

Add a cheap, dependency-free check that derives the intended repository
development version from the Rust workspace and verifies at least:

- every workspace crate inherits/resolves the workspace version;
- every intra-workspace path dependency requirement equals the workspace
  version;
- Python client version equals the workspace version;
- TypeScript client version equals the workspace version;
- Python-native Python and Rust package versions equal the workspace version;
- Python-native local eggchaos dependency requirements equal the workspace
  version.

Prefer Python stdlib because TOML parsing via `tomllib` is available on the
repository's supported Python CI versions; JSON requires only stdlib.

The checker must have:

- ordinary repository check mode;
- focused fixture/negative tests proving mismatches fail;
- no network access;
- no mutation in check mode.

Wire the cheap coherence check into `scripts/check.sh` and one existing CI
path without adding a dedicated job.

### WP6 — Enforce tag/version agreement in release qualification

The release workflow currently builds artifacts named from
`GITHUB_REF_NAME`. Add a release-only guard so a tag such as `v0.2.0`
cannot build/package a workspace whose package version is different.

Requirements:

- on tag-triggered release runs, strip the leading `v` and compare the tag
  version to the canonical workspace version;
- mismatch fails before expensive qualification/artifact builds;
- `workflow_dispatch` remains usable for qualification without requiring a
  tag, but must still run ordinary manifest coherence;
- artifact naming continues to derive from the release tag on tag runs;
- no automated tag creation or publication is introduced.

Extend the existing release workflow structural tests if present; otherwise add
a focused check so removal of the guard is detectable.

### WP7 — Reconcile release-smoke and publish-order proof

Update comments/assertions that still describe the intra-workspace publish
requirements as specifically `0.1.0`.

The order proof must remain version-generic by deriving the workspace version
from Cargo metadata and asserting every local eggchaos requirement is
`^<workspace-version>`.

Run package-list/package-smoke checks for all publishable crates. Do not publish.

### WP8 — Exact-candidate qualification and planning closure

Run the full release-relevant gate on the exact implementation candidate and
record closure evidence.

At minimum:

    python3 scripts/check_version_coherence.py
    ./scripts/check.sh
    ./scripts/check_openapi.sh
    ./scripts/check_python_client.sh
    ./scripts/check_typescript_client.sh
    ./scripts/qualify_language_clients.sh
    ./scripts/qualify_python_native.sh
    ./scripts/qualify_eggfetch.sh
    ./scripts/release-smoke.sh
    ./scripts/release-artifact-smoke.sh

Also run both mandatory Toxiproxy oracle qualifications because version
reconciliation must not be allowed to mask a release-candidate compatibility
regression:

    TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 ./scripts/qualify_toxiproxy_v2_12.sh
    TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh)" EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1 ./scripts/qualify_toxiproxy_post_v2_12.sh

Require hosted CI green on the exact candidate.

## Version and compatibility invariants

- published `v0.1.0` history is immutable;
- current development package version becomes `0.2.0`;
- native control remains `/v1`;
- config schema remains version 1;
- RNG/determinism versions remain unchanged;
- Toxiproxy strict v2.12 remains the default profile;
- post-v2.12 packet-loss profile remains opt-in;
- all 36 native operations remain;
- no public Rust symbol is removed because of the version bump;
- Scenario V1/V2 semantics/capacity remain unchanged;
- no package is actually published by this milestone.

## Failure semantics

Version disagreement is a build/qualification failure, not a warning.

A future tag whose semantic version does not equal the canonical package
version must fail before release artifacts are treated as authoritative.

Historical documentation containing `v0.1.0` is not version drift and must not
be rejected merely for containing the old version.

## Acceptance criteria

M055 may close only when:

1. `v0.1.0` historical lineage is explicitly documented and unchanged;
2. all current Rust workspace packages resolve to `0.2.0`;
3. all intra-workspace Rust version requirements resolve to `0.2.0`;
4. Python client, TypeScript client, and Python-native package metadata are
   coherently `0.2.0`, absent an explicitly documented independent-version
   exception;
5. current-main documentation no longer calls the repo pre-release 0.1.0;
6. README clearly distinguishes the published 0.1.0 install path from the
   unreleased 0.2.0 development tree;
7. the version-coherence checker catches deliberately mismatched fixtures;
8. tag-triggered release qualification mechanically rejects tag/package
   version mismatch;
9. release-smoke/order proof is version-generic and green;
10. OpenAPI/SDK/native-Python/Eggfetch/Toxiproxy behavior remains green;
11. hosted CI is green on the exact candidate;
12. no tag/release/publication action occurred;
13. closure evidence records the exact candidate and the future owner release
    steps without claiming that 0.2.0 has been published.

## Rejection / stop conditions

Stop and re-plan if implementation discovers that:

- a first-party language package intentionally follows an independent version
  contract that would make a forced 0.2.0 alignment breaking;
- current public package metadata cannot be bumped without an API/ABI change;
- `/v1` or config/RNG schema versions would need to change solely because the
  package version changes;
- the release workflow cannot validate tag/package coherence without
  broadening GitHub permissions or introducing network-dependent tooling.

Do not solve a versioning problem by rewriting historical v0.1.0 evidence.

## Closure evidence

Create:

`plans/closure/M055-post-v0-1-0-release-state-and-v0-2-0-development-baseline-closure.md`

Record:

- exact implementation candidate;
- resolved v0.1.0 tag target and published release identity;
- version-bearing manifest census before/after;
- version-coherence positive and negative tests;
- full release-smoke/package results;
- mandatory oracle results;
- hosted CI run;
- explicit statement that v0.2.0 remains unpublished;
- owner-facing release handoff (tag/publish order/assets) without executing it.

## Follow-on rule

M055 activates no automatic feature successor.

After M055 closure the repository is a coherent unreleased 0.2.0 development
baseline. An owner may separately authorize a v0.2.0 publication/release
handoff, or the project may continue feature development before that release.
Scenario enumeration, egress-outbound chaining, new fault models, additional
native bindings, and downstream EggReplay/EggProbe product adapters remain
separate feature decisions.
