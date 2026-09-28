# M055 — Post-v0.1.0 Release-State Reconciliation and v0.2.0 Development Baseline — Closure

Status: `closed` on exact implementation candidate
`b0ecbf1cde5be23523c73ac70dc766deb47d50b3`.

## Historical release baseline (immutable, verified, unchanged)

- Annotated tag `v0.1.0` resolves to commit
  `81994dbc427365f1dfdaabfa39bdf077850e69ec`
  (`git rev-parse v0.1.0^{commit}`), tagged 2026-09-24.
- GitHub release `eggchaos v0.1.0`, published `2026-09-24T04:07:46Z`,
  carries five target binaries plus SHA-256 sidecars
  (`eggchaos-v0.1.0-{aarch64-apple-darwin,
  aarch64-unknown-linux-gnu, x86_64-apple-darwin,
  x86_64-pc-windows-msvc.exe, x86_64-unknown-linux-gnu}` + `.sha256`).
- M019 (`ca527db`) remains the final v0.1.0 pre-tag qualification
  authority; M020–M054 are post-v0.1.0 development and are not described
  as part of the v0.1.0 candidate anywhere in this change.
- No tag was moved, rewritten, recreated, or created
  (`git tag --list` shows only `v0.1.0`); no package was published; no
  GitHub release was created.

## Version decision

Next development package version frozen as plain `0.2.0` (not a Cargo
prerelease string). Unchanged by design: native control `/v1`, config
schema v1, RNG SplitMix64-v1, provenance schema v1, compiler-semantics
v1, OpenAPI `1.0.0` document version, strict v2.12 default profile,
opt-in post-v2.12 `packet_loss` profile, all 36 native operations, all
public Rust symbols, Scenario V1/V2 semantics and capacities.

## Manifest census (WP1 before → WP2/WP3 after)

Before (registration baseline `ea12798`, confirmed by pre-change `rg`):

- `[workspace.package].version = "0.1.0"` (root `Cargo.toml`);
- 14 intra-workspace `version = "0.1.0"` requirements across the 8
  workspace crates (experiment→core; protocol→core/experiment;
  server→core/experiment/protocol; cli→server; toxiproxy→core/server;
  eggfetch→core + dev experiment→experiment; embed→core/experiment/
  protocol/server);
- `bindings/python-client/pyproject.toml` `0.1.0`;
- `bindings/typescript-client/package.json` + `package-lock.json`
  (top-level and `packages[""]`) `0.1.0`;
- `bindings/python-native/pyproject.toml` `0.1.0`;
- `bindings/python-native/Cargo.toml` package `0.1.0` + 2 local
  eggchaos requirements `0.1.0`;
- `benchmarks/Cargo.toml` (separate, `publish = false`) package
  `0.1.0` + 3 local eggchaos requirements `0.1.0`.

After (candidate `b0ecbf1`): every item above reads `0.2.0`; post-change
`rg` reports no remaining `version = "0.1.0"` in any of these manifests.
`Cargo.lock`, `benchmarks/Cargo.lock`,
`bindings/python-native/Cargo.lock`, and `fuzz/Cargo.lock` regenerated;
each lock diff is a pure `0.1.0 → 0.2.0` version bump for local packages
only (no external dependency change). No first-party package claims an
independent version cadence, so aligned `0.2.0` stands with no exception.

Classified as historical fact and preserved unchanged: `v0.1.0` tag/release
references in plans/closures/provenance JSON, M019 authority wording,
OpenAPI document `1.0.0`, config/RNG/compiler schema versions, and the
README registry-install commands (now explicitly headed "Latest published
release: 0.1.0").

## Work packages

- WP1: tag target resolved, release identity recorded (`gh release view
  v0.1.0`), full `0.1.0` inventory classified per plan §WP1; historical
  facts preserved.
- WP2: workspace `0.2.0`; all 8 crates resolve to `0.2.0` (`cargo
  metadata --locked` clean); ordered publish path deps retained;
  publish order `core -> experiment/eggfetch -> protocol ->
  server/toxiproxy/cli -> embed` unchanged; lockfiles regenerated.
- WP3: Python client, TypeScript client (+ lockfile), Python-native
  Python + Rust manifests, and local requirements all `0.2.0`.
- WP4: README distinguishes "Latest published release: 0.1.0"
  (registry installs) from "Development/main" (unreleased `0.2.0`
  source build, with an explicit do-not-install-0.2.0 note);
  `architecture/overview.md` no longer carries stale current-main
  wording; M019 described as final **v0.1.0 pre-tag** authority;
  `tooling-distribution.md` "Pre-release 0.1.0" replaced with the
  published-vs-development account; `embedding-native.md` snippets
  track `0.2.0`; `plans/README.md` records the published v0.1.0 actions
  as done history.
- WP5: new `scripts/check_version_coherence.py` (stdlib-only, no
  network, no mutation) derives the workspace version and checks all 8
  workspace crates, every intra-workspace path requirement, both remote
  clients, Python-native Python+Rust manifests, and the benchmarks
  crate. `scripts/tests/test_version_coherence.sh` pins the inline
  fixture (1 clean + 9 stale classes), the real-tree `--check`, a
  deliberate-mismatch negative test, and dual wiring (local
  `scripts/check.sh` + independent bare `--check` in the CI
  `language-clients` job, no new job).
- WP6: new `scripts/check_release_tag_version.sh` (release-only) always
  runs manifest coherence, then on tag refs requires `tag minus
  leading v == workspace version`, failing before expensive steps;
  dispatch runs skip only the tag comparison. Wired as the first
  `qualify` step in `release.yml` (before `release-smoke.sh`);
  `scripts/tests/test_release_tag_version.sh` pins match/mismatch/
  dispatch behavior, guard-before-smoke ordering, tag-derived artifact
  naming, and the absence of tag-creation/publication steps. The guard
  test additionally runs in the CI `check` job (Linux/macOS gate, same
  as M048 Tier A).
- WP7: `release-smoke.sh` order proof was already version-generic
  (`cargo metadata --locked`, asserts `^<workspace_version>`) and now
  reports `workspace_version 0.2.0`; `release-artifact-smoke.sh` no
  longer hard-codes `0.1.0` — it derives the expected binary version
  from the workspace manifest. Full `cargo package --list` for all 8
  publishable crates green; nothing published.
- WP8: this record.

## Evidence (all on exact candidate `b0ecbf1` unless noted)

- `python3 scripts/check_version_coherence.py` →
  `{"version_coherence":"pass","workspace_version":"0.2.0",
  "workspace_crates":8}`; `--fixture` 10/10 classes behave as specified.
- `./scripts/check.sh` → green (provenance Tier A, planning-state
  guard, version-coherence guard, release-tag guard, fmt, clippy
  `-D warnings`, workspace tests, doc).
- `./scripts/check_openapi.sh` → `{"openapi":"pass","paths":21,
  "operations":36}`.
- `./scripts/check_python_client.sh` → pass (12 tests + drift gate).
- `./scripts/check_typescript_client.sh` → pass (contract +
  cross-language + drift gate).
- `./scripts/qualify_language_clients.sh` → pass; artifacts built as
  `eggchaos_client-0.2.0.*` and `@eggstack/eggchaos-client-0.2.0.tgz`
  (built, not published).
- `./scripts/qualify_python_native.sh` → pass (conformance + overhead).
- `./scripts/qualify_eggfetch.sh` → pass (eggfetch + server suites).
- `./scripts/release-smoke.sh` → pass, including
  `{"order_proof":"pass","workspace_version":"0.2.0",...}` and the
  derived-version artifact smoke.
- `./scripts/release-artifact-smoke.sh` (standalone) → pass; live
  binary reports `{"api":"v1","version":"0.2.0"}`.
- `TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)"
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1
  ./scripts/qualify_toxiproxy_v2_12.sh` → `differential:pass`, 50/50
  vs pinned v2.12.0 oracle.
- `TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh)"
  EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1
  ./scripts/qualify_toxiproxy_post_v2_12.sh` → `differential:pass`
  vs pinned `40f7fd31`.
- Hosted CI on exact candidate: run `36479111546` — see verdict below.

## Hosted CI verdict

Run `36479111546` on exact candidate `b0ecbf1`: **success, 14/14 jobs**
(3 `check` incl. the new M055 tag/version structural guard on
Linux/macOS, 1 `performance-provenance`, 8 `language-clients` incl. the
new bare `check_version_coherence.py --check`, 2 `python-native`). No
failures, no retries.

## Unpublished statement

`0.2.0` remains unpublished on the closure commit: no `v0.2.0` tag, no
crates.io/PyPI/npm publication, no GitHub release, no asset upload.
`eggchaos version` reporting `0.2.0` from a source build is development
metadata, not a release claim.

## Owner-facing release handoff (not executed)

When the owner chooses to publish a `0.2.0` successor:

1. Re-run the WP8 gate on a fresh exact candidate (this closure
   qualifies `b0ecbf1` only) with hosted CI green.
2. Create annotated tag `v0.2.0` (the new release guard will reject any
   tag whose version disagrees with the workspace).
3. Publish crates in order-proof order: `core -> experiment/eggfetch ->
   protocol -> server/toxiproxy/cli -> embed` (`scripts/release-smoke.sh`
   asserts the requirement graph; dependents' full `cargo package`
   succeeds only after predecessors publish).
4. Publish `eggchaos-client 0.2.0` (PyPI), `@eggstack/eggchaos-client
   0.2.0` (npm), and the `eggchaos-native 0.2.0` wheel/sdist
   (maturin, per `qualify_python_native.sh` /
   `build_python_native_artifacts.sh`).
5. Create the GitHub release with the 5-target artifacts + SHA-256
   sidecars produced by the release workflow.
6. As a separate release-closeout change, flip the README stable-install
   commands from `0.1.0` to `0.2.0` (M055 deliberately leaves the
   published-0.1.0 install path in place).

## Follow-on rule

M055 activates no automatic feature successor. The repository is a
coherent unreleased 0.2.0 development baseline. Scenario enumeration,
egress-outbound chaining, new fault models, additional native bindings,
and downstream EggReplay/EggProbe product adapters remain separate
feature decisions. No milestone is ready, active, or blocked after this
closure.
