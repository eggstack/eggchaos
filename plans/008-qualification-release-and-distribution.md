# M008 — Qualification, Release, and Distribution

Status: active
Depends on: M006, M007  
Role: first release gate

## Objective

Turn the implemented project into a defensible first release by reconciling the entire roadmap against exact-commit evidence: cross-platform behavior, compatibility, performance, security, API/docs state, crate publication, binaries, and reproducible installation artifacts.

M008 is not a feature-expansion milestone.

## User-visible outcome

Eggchaos has a first tagged release candidate, expected initially as `v0.1.0` unless the repository owner chooses another version, with:

- documented native Rust/API/CLI surfaces;
- qualified Toxiproxy v2.12 compatibility matrix;
- qualified Eggfetch adapter;
- Linux/macOS/Windows release binaries;
- checksums;
- crates.io-ready package graph;
- reproducible CI/release commands;
- explicit limitations and non-goals;
- measured no-fault overhead rather than an unsupported performance claim.

## Preconditions

M006 and M007 are both closed.

Before beginning M008, perform a planning/documentation census. If implementation diverged materially from roadmap/ADRs, write corrective plans first rather than forcing a release around stale documentation.

## Scope

M008 owns:

- final verification matrix execution;
- corrective fixes discovered by qualification, if narrow;
- fuzz/property soak;
- cross-platform CI;
- performance baseline/budget;
- security/dependency review;
- public API/docs reconciliation;
- package metadata;
- crate publish order validation;
- binary release workflow;
- checksum/provenance artifacts;
- installation documentation;
- final compatibility/support matrix;
- first release closure.

It does not add UDP, proxy chaining, language bindings, or new Toxiproxy-main features.

## Full verification gate

Execute and record the current equivalent of:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
```

If supported features are intentionally mutually exclusive, replace `--all-features` with a documented exhaustive supported profile matrix.

Also run:

- MSRV build/test at Rust 1.89 or documented revised MSRV;
- release build;
- security/advisory audit;
- dependency license check if project policy requires it;
- Toxiproxy qualification command;
- Eggfetch integration qualification;
- fuzz/property targets for a bounded but meaningful duration;
- release artifact smoke tests.

Every command in closure evidence names the exact candidate commit.

## Cross-platform matrix

Target initial binary/support validation:

- Linux x86_64;
- Linux aarch64;
- macOS aarch64;
- macOS x86_64 where CI runner/toolchain remains available;
- Windows x86_64.

If a target cannot be built/tested in CI, mark it experimental/unverified rather than silently listing it as supported.

At minimum, platform-specific reset behavior must be separately recorded.

Linux aarch64 should be suitable for common ARM SBC/server deployment, but SBC performance qualification itself may remain post-v1 unless target hardware is available.

## Fault qualification sweep

Re-run the complete `plans/reference/verification-matrix.md`.

For every release-baseline fault record:

- config validation;
- deterministic behavior;
- bounds/backpressure;
- byte conservation/discard accounting;
- shutdown;
- live mutation;
- standalone runtime;
- native API/CLI representation;
- compatibility mapping where applicable.

Any release-baseline fault with unresolved correctness evidence blocks release or must be explicitly removed from the release claim before tagging.

## Toxiproxy compatibility gate

Pin and record official v2.12.0 oracle identity.

Publish a user-facing matrix with:

- routes;
- toxics;
- defaults;
- API-shape status;
- behavioral status;
- platform qualifications;
- known divergences;
- unsupported current-main features.

Do not say “full Toxiproxy compatible” if reset or any route/toxic remains only intent-compatible.

The current-main `packet_loss` feature is not pulled into M008 merely because it exists upstream.

## Eggfetch integration gate

Record exact `eggfetch-core` version and feature profiles.

Re-run:

- H1;
- HTTPS/SNI;
- H2 qualification;
- pooled live update;
- timeout/retry;
- redaction.

Documentation must explain that faults operate at the physical stream level, not HTTP logical request/stream level.

## Performance qualification

### Baselines

Measure at least:

1. direct echo/no proxy;
2. bare `eggress-relay` where comparable;
3. eggchaos fixed-target relay with empty plans;
4. representative individual faults;
5. representative combined plan;
6. Eggfetch adapter empty policy.

Record:

- CPU/hardware;
- OS;
- Rust version;
- release profile;
- payload/concurrency;
- sample methodology;
- throughput;
- latency distribution;
- peak buffered bytes/high-water;
- idle memory/binary size where practical.

### Freeze release budgets from evidence

The roadmap deliberately does not specify an invented percentage.

At M008:

- inspect repeated stable measurements;
- set a no-fault regression budget for future releases;
- document whether the budget is absolute or relative to bare Eggress on the same machine;
- keep deliberate-fault waiting time out of “overhead” claims.

If empty-plan overhead is unexpectedly large, investigate before release rather than choosing a permissive budget to hide it.

## Security review

Review at least:

- admin loopback default;
- public-admin explicit opt-in/auth;
- secret redaction;
- bounded JSON/TOML;
- bounded fault buffers;
- bounded connections/history;
- absence of arbitrary shell/process execution;
- absence of unintended open forward-proxy behavior;
- no TLS interception;
- panic surfaces from hostile config/API inputs;
- dependency advisories;
- unsafe-code census;
- release artifact integrity.

If `unsafe_code` exists despite the initial deny policy, release is blocked until it is covered by an explicit ADR/audit or removed.

## Public API review

Because this is a new crate family, avoid prematurely freezing accidental implementation detail.

Before v0.1.0:

- enumerate public Rust items;
- remove/restrict internals that do not need support commitments;
- ensure non-exhaustive enums are used where appropriate;
- document error stability expectations;
- document config/API schema versioning;
- verify crate names/features are intentional;
- generate docs without warnings.

A semver/public-API snapshot tool may be introduced if consistent with other Eggstack repos.

## Documentation census

Root README should accurately cover:

- what eggchaos is;
- standalone quickstart;
- Rust embedding;
- native CLI/API;
- fault semantics;
- reproducibility;
- Toxiproxy compatibility;
- Eggfetch integration;
- security;
- limitations/non-goals.

Architecture docs should match actual crate ownership.

Planning registry must not contain stale “blocked” states for work that is in fact closed, nor claim closure without evidence.

## Crate publication graph

Expected dependency order, subject to actual M001 topology:

1. `eggchaos-core`;
2. `eggchaos-server`;
3. `eggchaos-toxiproxy` and `eggchaos-eggfetch` as independent adapters;
4. `eggchaos-cli`.

Before publishing:

- use versioned crates.io dependencies as required;
- verify package contents with `cargo package --list`;
- run `cargo package`/publish dry-run equivalents for every publishable crate;
- ensure README/license/repository/docs metadata;
- ensure no local path-only dependency makes the published package unusable;
- decide whether server/adapter crates are all public/publishable or intentionally private.

Do not publish placeholder adapter crates that contain no useful supported API.

## Binary release artifacts

The `eggchaos` binary should be built for verified targets.

Artifacts should include:

- platform/arch in filename;
- version;
- checksum file;
- license/readme as appropriate.

Prefer the same general Eggstack distribution conventions used by sibling Rust repos. If a shared updater/installer crate such as eggup is stable by M008, evaluate it as a separate explicit integration decision rather than adding a moving dependency during release qualification.

A one-command installer may be added only if its checksum/platform selection path is verified. It is not allowed to bypass artifact verification.

## Release workflow

Use manual/tag-triggered release rather than automatic publication from every main push.

Recommended sequence:

1. candidate commit passes all qualification;
2. closure/docs reconciled;
3. tag signed/created according to project policy;
4. CI builds release artifacts;
5. verify checksums/smokes;
6. publish crates in dependency order;
7. create GitHub release;
8. verify fresh install from crates.io/release binary.

Do not tag first and discover package graph errors afterward if a dry run can catch them.

## Artifact smoke tests

For every binary target available to CI:

- `eggchaos version`;
- `eggchaos --help`;
- start ephemeral fixed-target proxy;
- invoke native health/API through CLI;
- run one latency/limit fault;
- clean shutdown.

For package consumers:

- temporary external Rust project depending on published/packaged `eggchaos-core`;
- temporary external project using `eggchaos-eggfetch` if published.

This catches accidental reliance on workspace-only features.

## Plans/closure reconciliation

M008 closure must contain a table for M000–M008:

- plan;
- final state;
- implementation/closure SHA;
- evidence path;
- known deferred work.

Future roadmap items remain future; do not create fake closure for them.

Archive only truly superseded documents.

## Ordered work packages

Execute in this order:

1. **WP1 — State census:** reconcile M000–M007 implementation, registry, ADRs, docs, supported features, and unresolved findings; create corrective plans first if the state is materially inconsistent.
2. **WP2 — Full correctness/security matrix:** run workspace/MSRV/cross-platform/property/fuzz/security gates and repair only narrow qualification defects.
3. **WP3 — External qualification:** rerun pinned Toxiproxy v2.12 corpus/client smokes and exact Eggfetch H1/HTTPS/H2 integration evidence on the release candidate.
4. **WP4 — Performance characterization:** measure direct/Eggress/empty-chaos/fault/adapter baselines and freeze a future no-fault regression budget from repeated evidence.
5. **WP5 — Public API/documentation freeze:** review exposed Rust/API/config/CLI surface, semver posture, support matrix, examples, limitations, and generated docs.
6. **WP6 — Packaging graph:** run package-content/dry-run/external-consumer fixtures for each publishable crate in dependency order.
7. **WP7 — Binary distribution:** build verified target artifacts, checksums, installer path if justified, and per-artifact smoke tests.
8. **WP8 — Release closure:** produce the M000–M008 evidence census, verify the exact candidate commit/tag, publish only after all gates pass, and then close M008.

## Acceptance criteria

M008 closes only when:

- all required qualification matrix rows have evidence;
- supported CI targets are green;
- Toxiproxy v2.12 matrix is published and oracle-backed;
- Eggfetch integration is qualified;
- performance baseline and future no-fault regression budget are recorded;
- security/dependency review has no unresolved release-blocking findings;
- public API/docs match implementation;
- crates package successfully for intended publication;
- release binaries/checksums build and smoke-test;
- fresh external-consumer smoke succeeds;
- planning registry/closure state is reconciled;
- exact release candidate commit is identified.

## Stop/rejection conditions

Do not release if:

- closure depends on source inspection instead of missing execution evidence;
- compatibility oracle was not pinned/run;
- no-fault overhead regression remains unexplained;
- public admin exposure can be enabled unsafely by accident;
- unsafe code exists without approved audit;
- package artifacts depend on workspace-local paths that will fail downstream;
- reset behavior is advertised more strongly than platform evidence supports;
- CI-supported platform claims exceed actual tested targets;
- docs claim UDP/packet-level behavior not implemented.

If a blocker is narrow, write a numbered corrective plan after M008 (or amend M008 before execution) rather than hiding it in the release notes.

## Closure evidence

Create `plans/closure/M008-qualification-release-and-distribution-closure.md` containing:

- release candidate SHA/tag;
- full command/platform matrix;
- Toxiproxy oracle/corpus artifact;
- Eggfetch qualification evidence;
- performance report/budget;
- security/dependency audit;
- package dry-run/publish artifacts;
- binary checksums/smokes;
- external consumer test;
- M000–M008 reconciliation;
- final limitations.

Then set M008 -> `closed`.

Post-M008 feature work requires a fresh planning pass using the future roadmap table in `plans/registry.md`.
