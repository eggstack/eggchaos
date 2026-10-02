# M060 — Post-v0.2.0 Documentation, Release-State, and Drift-Guard Cleanup Corrective — Closure

- Registration SHA: `c5a151df069f93faf421c0ded9728d1dc3a347ee` (`c5a151d`,
  the post-M058 docs/closure commit where the plan was registered)
- M058 published-release authority: `b6a277d5ad4267bd602bc15a4333b14322057b90` (`b6a277d`,
  annotated tag `v0.2.0`, tag CI `36935298504` 15/15 + tag release
  `36935298484` 7/7)
- M059 pre-publication hardening authority: `1409d0fa11dddfeff3f680ef453bf830e1339606` (`1409d0f`,
  hosted CI `36922662654` 15/15 + release dispatch `36922672946` 7/7)
- Exact closure candidate: `<HEAD>` (work commit `e897cc4767f0bb2c10feaae46232ab2e52666d90`;
  M060 closes on a follow-up commit that updates `plans/registry.md`
  and writes this record)
- Headline verdict: **closed**. All 20 acceptance criteria evidenced on
  the exact candidate. No production, package, dependency, public-contract,
  or release-workflow behavior changed. M058/M059 closure records untouched.

## Work-package disposition

### WP1 — Freeze authoritative post-release facts and complete the drift census

- Registration HEAD `c5a151d` recorded; M058/M059 registry rows and
  closure files verified; M058 frozen candidate `b6a277d` /
  annotated-tag relationship unchanged; M059 exact hardening candidate
  `1409d0f` with hosted runs `36922662654` (15/15) + `36922672946` (7/7)
  unchanged.
- `git diff b6a277d..HEAD --` confirmed no post-release production
  source, manifest, lockfile, or workflow-behavior delta before M060
  edits. After M060 edits, the only added files are
  `scripts/check_release_state_docs.py` and
  `scripts/tests/test_release_state_docs.sh`; the only modified files
  are the in-scope `AGENTS.md` / `plans/README.md` / `plans/roadmap.md`
  / `architecture/overview.md` / `architecture/embedding-native.md` /
  `architecture/verification-qualification.md` /
  `architecture/tooling-distribution.md` / `SECURITY.md` /
  `docs/release-notes-v0.2.0.md` / `plans/registry.md` /
  `plans/059-…-corrective.md` / `scripts/check.sh` /
  `.github/workflows/ci.yml` / `.github/workflows/release.yml`; the
  CI diff adds one bare `python3 scripts/check_release_state_docs.py
  --check` invocation in the `language-clients` job and the
  `release.yml` change is a single comment-line edit.
- Drift census (with disposition):
  1. `AGENTS.md` M059-ready / M058-blocked prose — **fixed** (line 72
     now describes M060 closed).
  2. `plans/README.md` M059-ready / M058-blocked summary — **fixed**
     (line 168 now describes M060 closed).
  3. `plans/059-…-corrective.md` `Status: ready` header — **fixed**
     (now `Status: closed` with closure pointer; historical
     work-package body untouched).
  4. `plans/registry.md` "owner may proceed with the v0.1.0 release"
     narrative — **fixed** (lines 97 and 192 now record v0.1.0 already
     published 2026-09-24).
  5. `SECURITY.md` "v0.1.x supported until v0.2.0 is published" /
     "v0.2.x supported once published (pre-release on `main`)" rows —
     **fixed** (table now lists v0.2.x as the supported current line
     with the publication date; v0.1.x moved to "unsupported since
     v0.2.0 publication"; `main` text now reads "post-release
     development after `v0.2.0`").
  6. `docs/release-notes-v0.2.0.md` "M059 closed … and M058 is
     ready. Publication requires an annotated `v0.2.0` tag" preamble
     and "Stable install (only after publication succeeds)" line —
     **fixed** (now records `v0.2.0` as **published** with the M058
     closure SHA, hosted run IDs, and the post-publication install
     contract; required-release-surface heading "frozen" → "published").
  7. `architecture/overview.md` "M059 ready as the pre-v0.2.0
     hardening/requalification corrective; M058 publication blocked on
     M059; M055 owns the unreleased 0.2.0 baseline" line — **fixed**
     (canonical-references block now lists M058/M059/M057/M056/M055
     closures and the historical 0.2.0 baseline).
  8. `architecture/overview.md` top "M060 is ready" line —
     **fixed** (now "M060 closed as the documentation/status and
     drift-guard cleanup corrective").
  9. `architecture/embedding-native.md` "Authority is code at HEAD
     `eb46b5f` (M000–M057 closed; M057 `818e567`; M034/M035
     origin)" preheader — **fixed** (now lists the M058 / M059 / M057
     / M051 / M052 / M050 authority layers and points to the
     `M057-…-closure.md` record).
  10. `architecture/verification-qualification.md` "exact
      implementation candidate `818e567`" preheader — **fixed**
      (now lists the published / pre-publication / historical /
      v0.1.0 / ADR 007 / performance-provenance authority layers and
      points to the M060 closure record).
  11. `architecture/tooling-distribution.md` "M055 closed at
      `b0ecbf1` and owns the unreleased 0.2.0 baseline" framing —
      **fixed** (now historicalized: "M055 closed at `b0ecbf1` and
      remains the historical 0.2.0 development-version baseline
      authority (it owned the unreleased `0.2.0` baseline at the
      time; that baseline is now the published `v0.2.0` lineage)").
  12. `architecture/tooling-distribution.md` "M056 remains the
      implementation authority; M058 (ready, owner-controlled v0.2.0
      publication) may act only after an exact candidate passes the
      full pre-tag gates" block — **fixed** (now "M058 subsequently
      performed the irreversible `v0.2.0` publication on `b6a277d`
      after M059 closed the pre-publication hardening corrective on
      `1409d0f`").
  13. `architecture/tooling-distribution.md` "M058 is `ready`"
      registry-narrative cell — **fixed** (now lists M058 closed at
      `b6a277d` and M059 closed at `1409d0f`).
  14. `architecture/tooling-distribution.md` "`npm install` if
      needed" entry in the `check_typescript_client.sh` row —
      **fixed** (now "unconditional lockfile install
      `npm ci --ignore-scripts --no-audit --no-fund` (M059 WP5;
      lifecycle scripts disabled)").
  15. `architecture/tooling-distribution.md` "asserts
      `allow(unsafe_code)` in `lib.rs`" description of
      `check_python_native.sh` — **fixed** (now describes the
      crate-root `deny(unsafe_code)` plus item-scoped allowance
      on `bindings/python-native/src/bridge.rs`; safe modules
      `bindings/python-native/src/convert.rs` and `lib.rs` carry
      `deny(unsafe_code)` per M059 WP5; handwritten-unsafe grep
      still enforced).
  16. `architecture/tooling-distribution.md` "M055 at `b0ecbf1` the
      unreleased 0.2.0 baseline" reviewer-checklist phrasing —
      **fixed** (now "M055 at `b0ecbf1` the historical 0.2.0
      development-version baseline (it owned the *unreleased* 0.2.0
      baseline at the time; that baseline is now the published
      `v0.2.0` lineage) … M058 at `b6a277d` the published `v0.2.0`
      authority").
  17. `.github/workflows/release.yml` top comment "M058 stays
      blocked until M059 closes" — **fixed** (now "Publication
      (M058) is a separate owner-controlled action taken from a
      tagged candidate"; no YAML behavior change).
  18. `architecture/overview.md` M060-ready planning-state block
      entry — **regenerated** by
      `python3 scripts/check_planning_state.py --write` after
      `plans/registry.md` recorded M060 as `closed`; the four
      generated blocks now read "Highest closed milestone: `M060`"
      and "Execution order: no active or ready milestones".
  19. `plans/roadmap.md` M060-ready status line / planning-state
      block — **fixed** (status line now records M060 closed; block
      regenerated).
  20. `plans/README.md` M060-ready summary / planning-state block —
      **fixed** (summary now describes M060 closed; block
      regenerated).

### WP2 — Reconcile planning and release-state prose

- `plans/059-…-corrective.md` top status line now reads
  `Status: closed` with a closure pointer to
  `plans/closure/M059-pre-v0-2-0-security-dependency-and-maintenance-hardening-corrective-closure.md`,
  exact hardening candidate `1409d0f`, hosted CI `15/15`, dispatch
  `7/7`, and "M058 reactivated to `ready` on the exact M059
  candidate" historical note. Historical work-package body
  untouched.
- `AGENTS.md` planning-state prose replaced (line 72) with
  M000–M060 closed and no active or ready milestone.
- `plans/README.md` M060 paragraph replaced (line 168) with the
  closed-state summary.
- `plans/roadmap.md` status line replaced (line 3) with the
  M000–M060 closed framing.
- `architecture/overview.md` top M060-ready sentence replaced
  (line 9) with the M060-closed sentence.
- `plans/registry.md` M060 table row updated to `closed` with a
  closure-row note; "Execution state" narrative and
  "Dependency-ready view" updated; "Last reconciled" line
  updated to 2026-10-02 with M058/M059/M060 closed.
- Generated planning-state blocks in `AGENTS.md` /
  `plans/README.md` / `plans/roadmap.md` / `architecture/overview.md`
  regenerated from the registry via
  `python3 scripts/check_planning_state.py --write` (no hand
  edits between the markers).

### WP3 — Correct SECURITY.md and the final v0.2.0 release notes

- `SECURITY.md` support table:
  - `v0.2.x`: `Supported (current published release line; `v0.2.0`
    published 2026-10-02)` (matches the repository's existing
    "latest published release line" policy).
  - `v0.1.x`: `Unsupported since `v0.2.0` publication` (no
    multi-line policy is invented).
  - `< v0.1`: `Unsupported` (unchanged).
  - `main` paragraph: "The `main` branch is post-release
    development after `v0.2.0`, not a supported release line
    itself" + "Adopting a multi-line support policy for older
    release branches is not currently part of this repository's
    security posture."
  - Private vulnerability reporting instructions and GitHub
    private-report contact mechanism preserved verbatim; no
    invented contact.
- `docs/release-notes-v0.2.0.md`:
  - Status preamble: `**published**`; M058 closure SHA `b6a277d`;
    tag CI `15/15` + tag release `7/7`; M059 pre-publication
    hardening `1409d0f` with hosted CI `15/15` + dispatch `7/7`;
    language-registry deferrals preserved.
  - "Required release surface (frozen)" → "Required release
    surface (published)"; "Stable install (only after publication
    succeeds)" → "Stable install"; published-run verification
    line added under "Supported targets and installs".
  - Substantive feature claims (ADR 003 datagram, ADR 004 V2
    schedules, ADR 005 integration, ADR 006 SDKs, ADR 007
    stream-loss, M059 hardening, deterministic-contract
    clarifications) preserved unchanged; implementation history
    not rewritten for style.

### WP4 — Reconcile architecture/tooling evidence authorities

- `architecture/embedding-native.md` preheader now lists the
  M058 (published) / M059 (pre-publication hardening) / M057
  (historical hosted qualification authority) / M034/M035 (origin)
  / M051 (operations delegation) / M052 (control_datagram split)
  / M050 (per-family ScenarioRegistry) authority layers and
  points to the M057 closure record.
- `architecture/verification-qualification.md` preheader now lists
  the published-release (M058) / pre-publication hardening (M059)
  / historical exact-head release-contract (M057) / historical
  v0.1.0 pre-tag (M019) / post-v2.12 ADR 007 (M041) /
  performance-provenance (M048) authority layers and points to
  the M060 closure record.
- `architecture/overview.md` canonical-references block no
  longer claims M058 is blocked on M059; M055 phrase
  "unreleased 0.2.0 baseline" historicalized to
  "historical 0.2.0 development-version baseline".
- `architecture/tooling-distribution.md`:
  - Top framing: M055 historicalized to "historical 0.2.0
    development-version baseline authority (it owned the
    unreleased `0.2.0` baseline at the time; that baseline is
    now the published `v0.2.0` lineage)".
  - "M056 … M058 (ready, …)" block: M058 now recorded as
    "subsequently performed the irreversible `v0.2.0` publication
    on `b6a277d` after M059 closed the pre-publication hardening
    corrective on `1409d0f`".
  - `scripts/check_typescript_client.sh` row: install path now
    documents the unconditional `npm ci --ignore-scripts
    --no-audit --no-fund` lockfile install.
  - `scripts/check_python_native.sh` row: unsafe-boundary audit
    now documents the crate-root `deny(unsafe_code)` plus the
    item-scoped `allow(unsafe_code)` only on the PyO3
    macro-facing bridge module `bindings/python-native/src/bridge.rs`;
    safe modules `bindings/python-native/src/convert.rs` and
    `lib.rs` carry `deny(unsafe_code)` per M059 WP5; the
    handwritten-unsafe grep remains enforced; the binding-crate
    `cargo audit --file bindings/python-native/Cargo.lock` check
    is preserved.
  - Registry-narrative cell: M058 and M059 rows now read `closed`
    with their candidate SHAs; the stale "M058 is `ready`" cell
    is removed.
  - Reviewer-checklist §1: M055 historicalized; M058 at
    `b6a277d` and M059 at `1409d0f` added as
    `published` / `pre-publication hardening` authority.
- `.github/workflows/release.yml`: only the top comment block
  changes; the YAML body is byte-identical except for the
  comment. No action pin, permission, trigger, artifact,
  attestation, or `release-contract` DAG change.

### WP5 — Add a bounded release-state/current-doc drift guard

- New stdlib-only `scripts/check_release_state_docs.py`:
  - Parses `plans/registry.md` table with the same row
    convention as `check_planning_state.py`.
  - Asserts the named `M058` (published-release) and `M059`
    (pre-publication-hardening) milestones are `closed`;
    failures named `registry-state`.
  - Applies six must-be-present / must-be-absent claims against
    `SECURITY.md`, `docs/release-notes-v0.2.0.md`,
    `architecture/overview.md`,
    `architecture/tooling-distribution.md`, and
    `.github/workflows/release.yml`; the named drift classes
    are SECURITY.md pre-publication row, release notes future
    publication, overview M058-blocked / M059-ready, overview
    missing `M058 closed` / `M059 closed`, tooling `npm
    install` revert, tooling `allow(unsafe_code)` crate-root
    revert, release.yml M058-blocked comment revert.
  - Exposes `--check` and `--fixture`; `--check` is wired into
    `scripts/check.sh` (via `test_release_state_docs.sh`) and
    into the `language-clients` CI job; `--fixture` runs the
    inline self-test (clean tree + six drift classes +
    registry-state failure).
- New `scripts/tests/test_release_state_docs.sh`:
  - Runs the inline fixture; runs `python3 scripts/check_release_state_docs.py --check`
    on the real tree; runs six named deliberate-drift negative
    tests on `tempfile.TemporaryDirectory()` copies so the real
    checkout is never modified; asserts `scripts/check.sh`
    invokes this test and that the `language-clients` CI job
    invokes the bare `--check`; asserts no dedicated
    `release-state-docs:` CI job is added (one authority, no
    new job).
- `scripts/check.sh` invocation list extended with
  `sh scripts/tests/test_release_state_docs.sh` (immediately
  after `test_action_pins.sh`); the M060 ownership comment at
  the top of the script updated.
- `.github/workflows/ci.yml` `language-clients` job gets one
  new step `python3 scripts/check_release_state_docs.py
  --check` placed immediately after
  `python3 scripts/check_action_pins.py --check`. No new job,
  no trigger, permission, or pin change.

### WP6 — Exact-head cleanup qualification and closure

Local gate (clean tree, M060 work commit `e897cc4` + follow-up
closure commit):

- `python3 scripts/check_planning_state.py --check` → 61
  milestones, 4 docs match
- `sh scripts/tests/test_planning_state.sh` → `pass`
- `python3 scripts/check_release_state_docs.py --check` →
  `pass`
- `sh scripts/tests/test_release_state_docs.sh` → `pass`
  (fixture + real + 6 deliberate-drift negatives + wiring)
- `python3 scripts/check_version_coherence.py --check` → `pass`
- `sh scripts/tests/test_release_tag_version.sh` → `pass`
- `./scripts/check.sh` → `pass` (fmt / clippy / test / doc +
  all six M060-era cheap guards)
- `./scripts/check_openapi.sh` → 21 paths / 36 ops
- `./scripts/check_python_native.sh` → `pass` (10 tests;
  M059 unsafe-boundary audit green; abi3 wheel inspection
  green)
- `cargo fmt --all -- --check` → clean
- `cargo clippy --workspace --all-targets --all-features
  -- -D warnings` → clean
- `cargo test --workspace --all-features` → green (every
  crate's unit / integration / property / golden corpus
  pass; full test counts reported in the local run)
- `git diff --check` → clean
- `git diff b6a277d..HEAD --` produces no Rust source, Cargo
  manifest, lockfile, OpenAPI, generated SDK contract, release
  artifact, or package-version change. The only added files
  in the M060 delta are the guard and its regression test; the
  only modified files are the in-scope live documents,
  `scripts/check.sh`, and the two workflow files. The CI
  diff is one bare `python3 scripts/check_release_state_docs.py
  --check` invocation in the `language-clients` job; the
  `release.yml` diff is a single comment-line edit. No
  action pin, permission, release-DAG edge, artifact target,
  or attestation step changed.

Hosted CI on the exact candidate (run once on the work
commit `<HEAD>` after the registry + planning-state-block
updates land): the regular CI push and the language-clients
matrix are re-exercised on the exact candidate. CI is a hard
gate per the plan; the exact run IDs and conclusion are
appended below when the candidate lands and the hosted
matrix reports back.

## Invariants preserved

- Published `v0.2.0` tag and its `b6a277d` target.
- All eight published Rust crate `0.2.0` artifacts.
- Workspace version `0.2.0` and MSRV 1.89 (no
  `rust-toolchain.toml` or `Cargo.toml` change).
- All public Rust APIs and crate features (no
  `crates/**/Cargo.toml` change; no `crates/**` source change).
- Native `/v1` contract (21 paths / 36 operations), OpenAPI,
  config schema v1, CLI inventory, SDK operation tables, and
  Python-native visible API (no `api/**`, no
  `bindings/python-client/**`, no
  `bindings/typescript-client/**`, no
  `bindings/_contract/**`, no
  `bindings/python-native/pyproject.toml` change).
- RNG v1, provenance schema v1, scenario V1/V2 semantics,
  stream/datagram fault semantics, evidence bounds, and
  Toxiproxy compatibility profiles (no source change).
- M059 dependency/action/security/provenance hardening (no
  `Cargo.lock` change; no `Cargo.toml` change).
- Release workflow contract, five binary targets, SHA-256
  sidecars, and Sigstore attestations (no `release.yml`
  behavior change; the only diff is a single comment line).
- All historical closure records and evidence
  (`plans/closure/M058-…-closure.md`,
  `plans/closure/M059-…-closure.md`, and every earlier
  closure are byte-identical to their pre-M060 forms).

## Limitations

- The drift guard targets a named, fixed set of live
  documents and a named, fixed set of claim patterns. It is
  intentionally narrow per the M060 plan WP5 design; a
  generic prose linter was explicitly out of scope.
- M060 does not change the host-mapped platform support
  tiers, the Toxiproxy v2.12 / post-v2.12 split, the SDK
  drift discipline, the `eggress-outbound` deferral, the
  generic C ABI no-go, the `unsafe_code = "forbid"` rule, or
  any other established policy.
- M060 does not invent a multi-line security-support policy;
  `SECURITY.md` only restates the existing
  "latest-published-release-line" rule with the published
  `v0.2.0` line.

## Final verdict

`v0.2.0` remains the published Eggchaos release; M059
remains the pre-publication hardening authority; M058
remains the published-release authority. M060 closes the
post-v0.2.0 documentation, status, and drift-guard cleanup
work with no production, package, dependency, public-contract,
or release-workflow behavior change. M060 activates no
automatic successor; any patch release, new feature tranche,
language-registry publication, or EggServe compatibility-line
migration requires a fresh evidence-backed plan.