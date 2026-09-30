# M058 — v0.2.0 Release Publication and Distribution

Status: ready
Depends on: M057 closed
Role: owner-controlled release publication / distribution milestone
Registration baseline: eb46b5fd416d2d76f5732301f739904fc8eb7d5e

## Objective

Publish Eggchaos `v0.2.0` from one frozen, fully qualified commit while
preserving the release-state, workflow-gating, and exact-head evidence
discipline established by M055–M057.

M058 is the first milestone in this line that is authorized to perform
irreversible release actions. It owns:

- freezing the exact `0.2.0` release candidate;
- final release notes/install-document reconciliation;
- creating the annotated `v0.2.0` tag on that exact candidate;
- running and verifying the tag-triggered release workflow;
- publishing the Rust crate graph to crates.io in dependency order;
- creating the GitHub `eggchaos v0.2.0` release with the five qualified
  binaries and SHA-256 sidecars;
- verifying fresh consumer/install paths from the published surfaces;
- deciding, with explicit per-registry evidence, whether the aligned
  first-party language packages are included in this release or deferred.

The milestone must not turn package-version alignment into implicit
publication. Registry uploads are permanent release operations and each
publication surface must have an explicit go/no-go decision before upload.

## Baseline

At registration baseline
`eb46b5fd416d2d76f5732301f739904fc8eb7d5e`:

- workspace and first-party package metadata are coherently `0.2.0`;
- `v0.1.0` is the only published Git tag/GitHub release;
- M055 owns the unreleased `0.2.0` package/version baseline;
- M056 owns the release-workflow `release-contract` implementation;
- M057 is the final exact-head qualification authority for that DAG:
  candidate `818e5674f2efaf96ec8effda81cef1dfa7a48614`, ordinary hosted CI
  run `36490497114` green 14/14, and release `workflow_dispatch` run
  `36630812771` green;
- current `main` is one documentation/closure commit beyond that M057
  candidate and has ordinary CI green;
- `.github/workflows/release.yml` does not publish packages or create a
  GitHub release; it qualifies and uploads workflow artifacts only;
- the release workflow builds five binary targets:
  - `x86_64-unknown-linux-gnu`;
  - `aarch64-unknown-linux-gnu`;
  - `x86_64-apple-darwin`;
  - `aarch64-apple-darwin`;
  - `x86_64-pc-windows-msvc`;
- the Rust publish graph is version-generic and currently resolves as:
  ```text
  core
   ├──> experiment ──> protocol ──> server ──> cli
   │                         └────> toxiproxy
   └──> eggfetch
  protocol/server ────────────────> embed
  ```
  with canonical publication order:
  `core -> experiment/eggfetch -> protocol -> server/toxiproxy/cli -> embed`;
- the Python remote client, TypeScript remote client, and Python-native
  package all carry `0.2.0` metadata, but M055 explicitly did not publish
  them. Version alignment alone is not release authorization.

The published Rust `0.1.0` lineage exists on crates.io/docs.rs; M058 is a
successor publication, not a first-name allocation for the Rust crate family.

## Release scope

### Required v0.2.0 release surface

M058 cannot close until all of these are published and verified:

1. **Rust crates.io graph** — all eight workspace crates at `0.2.0`:
   - `eggchaos-core`;
   - `eggchaos-experiment`;
   - `eggchaos-eggfetch`;
   - `eggchaos-protocol`;
   - `eggchaos-server`;
   - `eggchaos-toxiproxy`;
   - `eggchaos-cli`;
   - `eggchaos-embed`.
2. **Git tag** — annotated `v0.2.0` pointing to the exact frozen release
   candidate.
3. **GitHub release** — `eggchaos v0.2.0` attached to `v0.2.0`.
4. **Five binary artifacts plus five SHA-256 sidecars** from the successful
   tag-triggered release workflow.
5. **User-facing stable-install documentation** updated from published
   `0.1.0` to published `0.2.0` only after publication succeeds.

### Language-package release matrix

Before any PyPI/npm upload, record a per-package decision:

| Package | Registry | Default M058 disposition |
| --- | --- | --- |
| `eggchaos-client` | PyPI | gated candidate; publish only after namespace ownership/credentials, artifact contents, and fresh-install verification are proven |
| `@eggstack/eggchaos-client` | npm | gated candidate; publish only after scope/package ownership and npm provenance/credential path are proven |
| `eggchaos-native` | PyPI | **defer by default**; this remains an alpha/native embedding pilot and does not yet have a release-grade cross-platform wheel publication matrix |

A gated language package may be included in `0.2.0` only if the owner
explicitly selects it during M058 execution and all package-specific gates
below pass. Deferral does not block the required Rust/GitHub release and does
not change its aligned `0.2.0` source metadata.

Do not allocate or publish a new registry namespace merely to make the release
look more complete.

## Non-goals

- no runtime/API/fault/config/RNG/determinism change;
- no native `/v2` API;
- no package-version bump beyond `0.2.0`;
- no dependency upgrades unrelated to release correctness;
- no new binary target;
- no new installer/updater integration;
- no branch-protection/ruleset change;
- no generic C ABI or additional binding work;
- no EggReplay/EggProbe product integration;
- no rewrite of M055–M057 closure history;
- no retagging or movement of `v0.1.0`.

If qualification discovers a code defect requiring production changes, stop
publication, write a corrective successor, and produce a new exact candidate.

## Affected surfaces

Expected before publication:

- `README.md`;
- release notes/changelog material introduced by M058;
- `architecture/tooling-distribution.md` if release-state prose is stale;
- planning registry/current-state projections.

Expected release-system actions:

- Git annotated tag `v0.2.0`;
- crates.io publication of the eight Rust crates;
- GitHub Actions release workflow on the tag;
- GitHub Release creation and asset attachment;
- optional, separately gated PyPI/npm publication.

Expected after publication:

- README stable install commands switch to `0.2.0`;
- architecture/planning release-state prose says `v0.2.0` is published;
- closure evidence records immutable release identifiers, checksums, package
  versions, registry verification, and any deferred language package.

## Ordered work packages

### WP1 — Freeze the release surface and release notes

Before any irreversible action:

1. inventory user-visible changes since `v0.1.0`;
2. write concise `v0.2.0` release notes grouped around supported outcomes,
   not internal milestone numbers alone;
3. include at minimum:
   - UDP/datagram chaos;
   - deterministic Scenario V2 schedules;
   - consumer-neutral experiment/integration substrate;
   - Python/TypeScript remote control SDK source surfaces;
   - Python native embedding pilot status;
   - deterministic stream-loss / opt-in post-v2.12 `packet_loss`
     compatibility;
   - performance/provenance/tooling hardening;
   - relevant compatibility/security limitations;
4. freeze the required Rust/GitHub release surface above;
5. record go/no-go for each language-package registry surface.

Release notes must distinguish:

- strict/default Toxiproxy v2.12 compatibility;
- opt-in pinned post-v2.12 profile;
- native userspace `stream-loss` from IP/TCP packet loss;
- fixed-target proxy behavior from forward-proxy behavior.

### WP2 — Produce one exact release candidate

Create the release candidate commit containing all intended pre-tag
documentation/release-note changes.

After that candidate is frozen:

- no production, workflow, manifest, generated-contract, or release-note
  mutation may occur without invalidating the candidate;
- do not tag a moving `main`;
- record the full 40-character SHA that `v0.2.0` must target.

Because `main` is currently unprotected, release correctness comes from
freezing and checking the exact SHA, not from assuming branch-tip immutability.

### WP3 — Run the full exact-candidate pre-tag gate

On the exact release candidate run at minimum:

```sh
python3 scripts/check_planning_state.py --check
python3 scripts/check_version_coherence.py --check
sh scripts/tests/test_release_tag_version.sh
./scripts/check.sh
./scripts/check_openapi.sh
./scripts/check_python_client.sh
./scripts/check_typescript_client.sh
./scripts/qualify_language_clients.sh
./scripts/check_python_native.sh
./scripts/qualify_python_native.sh
./scripts/qualify_eggfetch.sh
./scripts/release-smoke.sh
./scripts/release-artifact-smoke.sh
EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh
```

Mandatory compatibility gates:

```sh
TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" \
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 \
  ./scripts/qualify_toxiproxy_v2_12.sh

TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh)" \
  EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1 \
  ./scripts/qualify_toxiproxy_post_v2_12.sh
```

Require:

- ordinary hosted CI green on the exact candidate (normal 14-job matrix);
- a pre-tag `workflow_dispatch` of the release workflow green on the same
  exact SHA/ref;
- retained performance budgets green where the release workflow enforces them;
- no dirty-tree authoritative performance evidence.

Any code or workflow change after these gates produces a new candidate and
restarts WP3.

### WP4 — Dry-run and externally validate the Rust package graph

Before tagging or publishing:

1. run `cargo package --list` for every crate;
2. run every dependency-independent `cargo publish --dry-run` that can
   succeed before registry predecessors exist;
3. inspect packaged manifests to ensure path dependencies carry the exact
   registry requirement `^0.2.0`;
4. verify README/license/repository/docs metadata in each package;
5. create disposable external consumer fixtures against the packaged source
   where possible;
6. record the publication order:
   ```text
   1. eggchaos-core
   2. eggchaos-experiment
   3. eggchaos-eggfetch
   4. eggchaos-protocol
   5. eggchaos-server
   6. eggchaos-toxiproxy
   7. eggchaos-cli
   8. eggchaos-embed
   ```
   Steps at the same dependency level may be adjacent but publish
   verification must complete before dependents are attempted.

Do not weaken dependency requirements to work around registry propagation.

### WP5 — Validate optional language-package artifacts

For each language package selected for publication:

#### Python remote client

- build sdist and wheel from a clean exact candidate;
- inspect archive contents;
- install each artifact into a fresh virtual environment;
- run import + contract smoke;
- verify PyPI namespace ownership and upload credentials without exposing
  tokens in logs;
- use the registry's preflight/test mechanism if available without changing
  the final package identity.

#### TypeScript remote client

- `npm pack` from a clean exact candidate;
- inspect tarball contents and generated `dist`;
- install tarball into a fresh consumer project;
- execute import/type smoke;
- verify `@eggstack` scope/package publish permission;
- use provenance/2FA policy required by the organization.

#### Python native pilot

Default: deferred.

Publishing it requires a separate explicit go decision plus a release-grade
wheel matrix and fresh-install/import smoke for every claimed platform. The
existing host-native build evidence is not enough to imply broad PyPI wheel
support. An sdist-only publication must not be used to hide missing platform
qualification.

### WP6 — Create the immutable v0.2.0 tag

Only after WP1–WP5 gates are satisfied for the selected release surface:

1. confirm `v0.2.0` does not already exist locally or remotely;
2. confirm the frozen candidate still equals the intended release source;
3. confirm workspace version is exactly `0.2.0`;
4. create an **annotated** `v0.2.0` tag pointing directly to the frozen
   candidate;
5. push only that tag;
6. resolve the remote tag object and peeled commit and record both SHAs.

Never move or recreate `v0.2.0` after publication. If the tag points to the
wrong commit, stop before package publication/GitHub Release creation and
resolve explicitly rather than silently force-moving a public release tag.

### WP7 — Verify the tag-triggered release workflow and artifacts

The pushed tag must trigger `.github/workflows/release.yml`.

Require:

- `release-contract` succeeds and proves tag `0.2.0` equals workspace
  package version `0.2.0`;
- `qualify` succeeds;
- all five artifact matrix legs succeed;
- tag-triggered run head SHA equals the frozen candidate;
- downloaded artifacts contain exactly the expected binary/checksum pairs;
- each SHA-256 sidecar validates its binary;
- binary filenames encode `v0.2.0` and the correct target triple;
- `eggchaos --json version` reports `0.2.0` on every executable target
  that can be run on available verification hosts;
- build-only cross targets are labeled build-verified rather than
  runtime-smoked if no matching host exists.

Do not create the GitHub Release while this workflow is red or incomplete.

### WP8 — Publish the Rust crates in dependency order

Use `cargo publish --locked` (or the repository-standard equivalent) from
the exact tagged candidate.

For every crate:

1. publish one `0.2.0` package;
2. wait until the registry/index can resolve that exact version;
3. verify the published package metadata/source;
4. run the next dependent package's full `cargo package` /
   `cargo publish --dry-run` against the now-published predecessor;
5. only then publish the dependent.

Required order:

`core -> experiment -> eggfetch -> protocol -> server -> toxiproxy -> cli -> embed`.

Where two crates are dependency-independent, the implementation may reorder
within that level only if the closure records the actual order.

If publication fails after one or more predecessors have succeeded:

- stop the remaining graph;
- do not attempt to overwrite an already-published `0.2.0`;
- do not yank a healthy predecessor merely because a later crate failed;
- diagnose whether the same `0.2.0` graph can safely resume;
- if a published package itself is defective, stop and prepare a patch
  release plan rather than trying to replace immutable registry content.

### WP9 — Publish selected language packages

Only packages with an explicit WP1 go decision and green WP5 evidence may be
uploaded.

For each selected package:

- publish exactly `0.2.0`;
- verify registry page/version/artifacts after upload;
- fresh-install from the public registry, not the local build directory;
- run import/contract smoke;
- record immutable registry identifiers/digests where available.

A deferred package stays source-versioned `0.2.0` in the repository but must
be called out clearly in release notes so users are not told to install an
unpublished package.

### WP10 — Create the GitHub v0.2.0 release

Create GitHub Release `eggchaos v0.2.0` from the existing annotated
`v0.2.0` tag.

Attach exactly the qualified artifacts from the tag-triggered workflow:

- 5 binaries;
- 5 SHA-256 sidecars.

Release notes must include:

- major additions since v0.1.0;
- supported binary targets;
- Rust MSRV 1.89+;
- strict Toxiproxy v2.12 default + opt-in post-v2.12 profile distinction;
- fixed-target/non-MITM/non-forward-proxy limitations;
- crates.io install examples;
- language-package publication/deferment status;
- links to checksums.

Do not rebuild release assets locally after the tag workflow merely for
attachment convenience; use the qualified workflow outputs so artifact
identity remains traceable.

### WP11 — Fresh-publication verification

After all selected publications are visible:

#### Rust

From a clean external directory/cache state:

```sh
cargo install eggchaos-cli --version 0.2.0 --locked
```

Then verify:

- `eggchaos --json version`;
- `eggchaos --help`;
- minimal TCP proxy startup/health;
- minimal UDP/datagram proxy startup/health;
- one native control operation.

Create small external Cargo consumer fixtures for at least:

- `eggchaos-core = "0.2.0"`;
- `eggchaos-server = "0.2.0"`;
- `eggchaos-eggfetch = "0.2.0"`;
- `eggchaos-toxiproxy = "0.2.0"`.

#### GitHub artifacts

Download at least one native-host artifact from the public GitHub Release,
verify its sidecar checksum, and repeat the version/health smoke.

#### Language packages

For every package actually published in WP9, install from the public registry
in a fresh environment and repeat the WP5 consumer smoke.

### WP12 — Post-publication documentation and closure

Only after public verification succeeds:

- change README "Latest published release" to `0.2.0`;
- change stable `cargo install` / `cargo add` examples to `0.2.0`;
- remove the "do not install 0.2.0 until published" wording;
- update architecture/tooling release-state prose;
- keep `v0.1.0` historical references where they describe historical
  qualification;
- record any deferred language package explicitly;
- create M058 closure evidence;
- set M058 `closed` and regenerate planning-state projections.

The documentation closeout commit is expected to be after the tagged candidate
because it records facts that become true only after publication. It must not
be described as part of the immutable `v0.2.0` tag unless those facts were
already true before tagging.

## Release invariants

- `v0.2.0` tag points to one frozen exact candidate and never moves;
- package version is exactly `0.2.0`;
- native HTTP remains `/v1`;
- config schema remains version 1;
- deterministic RNG/version contracts remain unchanged;
- provenance schema v1 remains unchanged;
- strict Toxiproxy v2.12 stays default/frozen;
- post-v2.12 `packet_loss` stays opt-in;
- Scenario V1/V2 remain semantically distinct;
- all 36 native operations remain;
- five binary target triples remain unchanged;
- no publication credential appears in repository content, logs, closure
  evidence, or release notes.

## Failure semantics

Before the first irreversible registry upload, any failed mandatory gate
blocks the release.

After an irreversible publication succeeds, failures are handled as partial
release incidents, not by pretending the release did not happen:

- preserve an exact ledger of what is already public;
- do not force-move public tags;
- do not overwrite registry versions;
- do not delete/rebuild assets under the same checksum claim;
- resume only when dependency and artifact identity are unchanged and safe;
- use yank only for an actually defective crate and document why;
- use a patch release for corrected immutable content.

## Acceptance criteria

M058 may close only when:

1. one frozen `0.2.0` candidate SHA is identified;
2. all mandatory local gates pass on that exact candidate;
3. ordinary hosted CI is green on the exact candidate;
4. pre-tag release `workflow_dispatch` is green on the exact candidate;
5. Rust package graph/package contents and publish order are verified;
6. every optional language package has an explicit publish/defer decision;
7. annotated `v0.2.0` exists and peels to the exact frozen candidate;
8. tag-triggered release qualification succeeds from that exact tag/SHA;
9. all five binary artifacts and five checksums verify;
10. all eight Rust crates are publicly available at `0.2.0`;
11. fresh crates.io install and external Rust-consumer smokes pass;
12. every selected language package is publicly installable and smoke-tested,
    while every deferred package is explicitly documented as deferred;
13. GitHub Release `eggchaos v0.2.0` exists on the immutable tag with the
    qualified artifacts;
14. a public GitHub-release binary passes checksum/version/runtime smoke on a
    native host;
15. README/stable install documentation reflects published `0.2.0`;
16. no M055–M057 historical closure evidence is rewritten;
17. closure evidence contains the exact tag object/commit, workflow run IDs,
    package publication ledger, artifact checksums, public-install results,
    release URL/identity, limitations, and final verdict.

## Stop conditions

Stop before publication and write a corrective plan if:

- candidate qualification requires a production/workflow/manifest change;
- `0.2.0` already exists unexpectedly on any required Rust registry surface;
- the intended tag name exists but points elsewhere;
- crates.io ownership/credentials are unavailable;
- a required crate's package cannot resolve solely from registry dependencies;
- tag-triggered workflow behavior differs from the qualified M056/M057 DAG;
- artifact checksum/provenance identity cannot be established.

Defer an optional language package rather than blocking the required release
if its registry namespace, credentials, wheel matrix, or consumer smoke is not
release-ready.

## Closure evidence

Create:

`plans/closure/M058-v0-2-0-release-publication-and-distribution-closure.md`

Record at minimum:

- frozen candidate SHA;
- ordinary CI run ID and 14-job census;
- pre-tag release-dispatch run ID;
- annotated tag object SHA and peeled commit SHA;
- tag-triggered release workflow run ID and full job census;
- exact artifact filenames and SHA-256 values;
- Rust publication order, timestamps/registry verification, and any retries;
- optional language-package publish/defer decisions and evidence;
- public GitHub Release identity;
- fresh crates.io install/external consumer results;
- fresh GitHub artifact smoke;
- post-publication documentation commit;
- confirmation that v0.1.0 and M055–M057 historical records were not
  rewritten.

## Follow-on rule

M058 activates no automatic feature successor.

After clean closure, `v0.2.0` is the current published Eggchaos release.
Further feature work resumes from a fresh post-v0.2.0 planning decision;
release defects discovered after publication require a patch-release
corrective rather than mutation of the published `0.2.0` artifacts.
