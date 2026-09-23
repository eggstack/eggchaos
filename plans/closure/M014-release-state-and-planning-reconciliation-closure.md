# M014 Closure — Release State and Planning Reconciliation

Milestone: M014 (`014-release-state-and-planning-reconciliation.md`)
Candidate: reconciliation commit `37dc28bb44d74b5488264ae8db8be5ff05787879` (planning/docs/policy only; no production fault, runtime, or API change). Code baseline `e237daae92d42b3dc7898803fecf33cdcf0f78e7` with green ordinary CI; reconciliation changes verified by local full gates plus the stale-state grep gates below. This closure record follows in the same change family; the planning-only candidate-SHA annotation itself does not change code.
Implementation commits: reconciliation edits + this closure record (planning records only, plus one `deny.toml` policy tightening).
Commands executed: see Verification.
Platforms: darwin/arm64 local gates; Linux/macOS/Windows ordinary CI.
External oracle/version: none required for M014 (no Toxiproxy oracle surface; M012/M013 evidence preserved, not rerun).
Evidence artifacts: stale-state grep gates, `cargo metadata --locked`, `cargo tree --locked`, zero-git-source lock check, full workspace gates.
Registry transition: M014 `ready` -> `closed`; M015 `blocked` -> `ready`.
Next milestone activated: M015 (`ready`; sole remaining pre-tag gate).

## WP1 — State census (findings and disposition)

Enumerated stale current-state statements across `plans/`, `README.md`, `docs/`, workflows, and release notes:

1. `plans/009-core-fault-semantics-corrective.md` said `Status: ready` while the registry records M009 closed at `8c1373e90129e68e96379fc3277fade8ce087abd`. Fixed: marked `closed` with a historical closure note; candidate SHA preserved.
2. `plans/010-runtime-control-authority-corrective.md` said `Status: ready` while the registry records M010 closed at `3961e968e98948cfab1d0c99d3503ba1624e2e6`. Fixed as above.
3. `plans/011-live-state-scenario-observability-corrective.md` said `Status: blocked` while the registry records M011 closed at `acd0883`. Fixed as above.
4. `plans/012-toxiproxy-v2-12-parity-corrective.md` said `Status: blocked` while the registry records M012 closed at `a040ed7`. Fixed as above.
5. `plans/013-corrective-requalification-gate.md` said `Status: blocked` while the registry records M013 closed with a clean verdict at `9904490`. Fixed as above.
6. `plans/008-qualification-release-and-distribution.md`, `plans/README.md`, `plans/roadmap.md`, and `plans/registry.md` were already reconciled by commits `5565c54`, `6f2eebd`, `33880db`, `e237daa` (M008 `closed`, corrective chain completed history, `M014 -> M015` handoff). M014 additionally retires the `M014 -> M015` wording to `M015` alone and marks the §11A graph completed history.
7. `docs/configuration.md` said "The file/API surface is completed by M004. Until then …" in future tense while M004 is long closed. Fixed to present-tense schema-v1 + native API wording referencing the closed M004 and the M009–M013 requalification.
8. Root `README.md` said "The first-release qualification gate remains open until cross-platform, packaging, and release evidence is complete" with no mention of M008 closure or M015. Fixed to state M008 closed at `645a761`, M014 lineage, M015 final authority, and owner-decision tagging/publishing.
9. `plans/closure/M008-qualification-release-and-distribution-closure.md` §"Not done here" ended with "(core → eggfetch; server chain awaits upstream eggserve publication)" plus agent-voice "Say the word …". The parenthetical contradicted the same record's registry-switch paragraph (zero git sources, order-publishable). Fixed: order restated as core → eggfetch → server/toxiproxy/cli with registry `eggserve-*` 0.2.0; agent voice removed. Historical candidate `645a761` untouched.
10. `deny.toml` retained `allow-git = ["https://github.com/eggstack/eggserve.git"]` after the `00d1ee4` registry switch left zero git sources in `Cargo.lock`. Fixed: removed the stale allow entry, tightening sources to registry-only. Verified by `cargo deny check sources` passing and `grep 'source = "git' Cargo.lock` empty.
11. `plans/closure/M004-control-plane-cli-and-config-closure.md` notes "The EggServe crates are git-pinned because … not in the crates.io index." Classified as historical context at M004's candidate time; preserved, not rewritten. The later switch is recorded in the M008 closure lineage and below.
12. Remaining `git grep` hits for stale phrases are confined to `plans/014-…` itself describing the audit findings and verification commands, plus the M008 lineage note's deliberate historical phrase "git pins switched to crates.io". Classified as historical/descriptive, not live state.

No correctness or release-workflow defect was found. No new corrective plan was needed; per the plan's stop conditions, nothing was concealed by deletion — contradictory state was corrected with history preserved.

## WP2 — Planning reconciliation

- `plans/009` through `plans/013` now read `Status: closed` with historical closure notes pointing at their real candidates and closure records.
- `plans/008` remains `closed` with its historical closure note (M008 candidate `645a761…`, later commits explicitly excluded from that evidence).
- `plans/014` marked `closed` with a historical closure note; `plans/015` activated to `ready`.
- `plans/README.md` current-execution-order section now states M008/M009–M013/M014 closed and `M015` as the sole remaining pre-tag gate.
- `plans/roadmap.md` status line, §11A graph label ("Completed graph (historical)"), and remaining-chain wording updated to `M015` alone.
- `plans/registry.md` last-reconciled date `2026-09-23`; M014 `closed`, M015 `ready`; execution-state and dependency-ready views updated; no `blocked` pre-tag milestone remains.

## WP3 — Candidate lineage (post-M008, preserved candidate `645a761723f6fe10fda0975c74e33343e2700764`)

- `c42a198` — planning-only M008 closure bookkeeping (closure record, registry, performance-record formatting). No production, dependency, test, or workflow change.
- `00d1ee4` — dependency: `eggserve-*` git pins switched to crates.io `0.2.0` version-only deps; `Cargo.lock` registry-only (zero git sources). Planning note updated. Full workspace tests, audit, and deny re-passed on the registry graph.
- `22136c0` — test/workflow hardening: bounded H2 test awaits with explicit timeout phases and `timeout-minutes` caps on CI/release jobs. No production fault, runtime, or API change.
- `a8933f3` — test-only deterministic H2 server-close fix in `crates/eggchaos-eggfetch/src/lib.rs` (`#[cfg(test)]` module; bounded 5 s drain, then drop `connection` to close the transport regardless of pooled-client peer behavior). No production fault, runtime, or API change.
- `233202a`, `92f1f7a`, `5565c54`, `6f2eebd`, `33880db`, `e237daa` — planning-only M014/M015 scaffolding and reconciliation of registry, planning README, roadmap, and M008 plan state. No production, dependency, test, or workflow change.
- M014 reconciliation family (this closure's change): planning/docs wording plus the `deny.toml` stale-allow removal. No production, dependency-version, test-logic, or workflow change beyond the policy tightening (verified locally; `cargo deny check sources` passes).

M008 evidence remains tied to `645a761` and is not rewritten. Ordinary three-platform CI is green on the post-M008 HEAD (`e237daa`, run `35807266363`, success, 3m46s, 2026-09-23T01:40:31Z covering ubuntu/macos/windows). The dedicated release workflow has not yet been rerun on the post-M008 HEAD — that rerun is M015. No later commit is called release-qualified on ordinary CI alone.

## WP4 — Dependency/package census

- `Cargo.toml`: workspace version `0.1.0`, edition 2021, `rust-version = "1.89"`; `eggress-relay` (`eggress-relay`) `1.0.7`, `eggfetch-core` `0.2.0` with minimal H1 profile, `eggserve-primitives`/`eggserve-server` `0.2.0` version-only (no git rev).
- `Cargo.lock`: `eggserve-primitives`/`eggserve-server` `0.2.0` from `registry+https://github.com/rust-lang/crates.io-index` with checksums; `eggfetch-core` `0.2.0` registry; `eggress-relay` `1.0.7` registry; `grep -n 'source = "git' Cargo.lock` empty (zero git sources).
- `cargo metadata --format-version 1 --locked` resolves; `cargo tree --locked -i eggserve-server` shows the server/toxiproxy/cli chain on registry deps.
- `deny.toml` `[sources]` now registry-only (`unknown-registry = "deny"`, `unknown-git = "deny"`, single crates.io allowlist). `cargo deny check advisories licenses bans sources`: all ok.
- No `v0.1.0` tag, no crates.io publication, no GitHub release exists (verified `git tag --list` empty, `gh release list` empty). Owner-decision wording is consistent across M008 closure, root README, plans README, roadmap, and registry.
- Stale "publication blocked by unpublished EggServe" wording: removed from the M008 closure's live claim (now order-publishable core → eggfetch → server/toxiproxy/cli). The only remaining git-pin mention is explicitly historical lineage.

## WP5 — Support/release wording

- Rust 1.89: `Cargo.toml` `rust-version = "1.89"`, CI/release toolchains pinned `1.89.0`. Current.
- Version 0.1.0: workspace version consistent; pre-release (untagged) wording consistent. Current.
- EggServe 0.2.0 registry, Eggfetch 0.2.0, Eggress relay 1.0.7: verified above. Current.
- CI platforms: `.github/workflows/ci.yml` matrix ubuntu/macos/windows-latest with `timeout-minutes: 25`, fmt + clippy (`-D warnings`) + full workspace tests + docs + audit + deny. Current.
- Release targets: `.github/workflows/release.yml` artifact matrix x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu, x86_64-apple-darwin (macos-13), aarch64-apple-darwin (macos-14), x86_64-pc-windows-msvc (windows-2022), each with checksum packaging and `timeout-minutes` bounds; `workflow_dispatch` plus tag trigger. Matches the M015 five-target requirement. Current.
- Toxiproxy limitations: `docs/toxiproxy.md` divergences (clamping, coalescing, lowercase echo, ephemeral loopback listen, fixed-target/charset rejections, platform-qualified `reset_peer`, incomplete bandwidth/slicer/slow_close data-plane timing differential, no `packet_loss`) match the M008 closure's final limitations. No "fully compatible" overclaim. Current.
- Hard-reset qualification: `docs/architecture.md` separates abstract hard-reset requests from concrete platform capability and states `poll_shutdown` is never advertised as RST; M008 closure records RST as platform-qualified. Current (downgraded where evidence is platform-specific, not invented).
- Performance budget: M008 closure freezes empty-plan mean ≥ 70% of same-session bare relay from `qualification/performance/2026-09-22-macos-arm64-m008.json` (ratio 0.982 on that host); roadmap §12 records that M008 froze the budget from evidence rather than inventing one. No conflicting budget claim found in docs. Current.

## WP6 — Search-based stale-state gate

Executed on the reconciliation tree:

- `git grep -n -E 'M008 (is )?(active|ready)|release qualification.*paused|M009 and M010.*ready|corrective implementation active'`: only matches are inside `plans/014-…` describing the historical audit and the verification command itself. No live stale claim. Clean (historical/descriptive only).
- `git grep -n -E 'git.*eggserve|eggserve.*git|unpublished.*eggserve|eggserve.*unpublished'`: only the M014 verification command and the M008 lineage note's historical "git pins switched to crates.io" description. The `Cargo.toml`/`Cargo.lock` contain no git eggserve refs; `deny.toml` no longer allowlists eggserve git. Clean (historical/descriptive only).
- `git grep -n '^Status:' -- plans/*.md`: M000–M008 closed, M009–M013 closed, M014 closed, M015 ready. Canonical planning sources agree.

## WP7 — CI confirmation

- Ordinary CI on code baseline `e237daa`: run `35807266363` success (3m46s, 2026-09-23T01:40:31Z, push, ubuntu/macos/windows matrix). The prior `22136c0` failure (8m41s, stuck-H2 era) predates the `a8933f3` deterministic fix; `a8933f3` CI (run `35801207680`, 15m53s) succeeded.
- Local full gates on the reconciliation tree (darwin/arm64, Rust 1.89.0): `cargo fmt --all -- --check` PASS; `cargo clippy --workspace --all-targets --all-features -- -D warnings` PASS; `cargo test --workspace --all-features` PASS (core 43, server 37, toxiproxy lib 10, cli e2e 1, eggfetch lib 3 + regression 10, differential 1, doc suites ok); `cargo doc --workspace --all-features --no-deps` PASS (0 warnings); `cargo audit --deny warnings` exit 0 (205 deps); `cargo deny check advisories licenses bans sources` all ok (post-`deny.toml` tightening).
- `cargo metadata --format-version 1 --locked` resolves; `cargo tree --locked` registry-only for the Eggstack seams.
- The reconciliation commit's own CI run is the post-commit ordinary-CI record; M015 additionally requires green ordinary CI plus the dedicated release workflow on its frozen candidate, so any regression in the planning-only family would still gate the tag.

## Acceptance-criteria verdict

M014 acceptance is met: canonical planning sources agree (M001–M014 + M008 closed, M015 sole ready pre-tag milestone); the M008 plan is marked closed; completed corrective work is described as history, not pending; historical evidence remains tied to its real candidates (`645a761` for M008, `8c1373e`/`3961e96`/`acd0883`/`a040ed7`/`9904490` for M009–M013); post-M008 lineage is explicit commit-by-commit with production/dependency/test/planning classification; dependency/publication wording is current and registry-proven; stale current-state phrases are resolved or explicitly classified historical; no implementation defect is concealed (none found; stop conditions did not trigger); exact-commit closure evidence exists (baseline CI run ID plus local gates on the reconciliation tree); M015 is the sole ready pre-tag milestone.

## Follow-on activation

M014 -> `closed`; M015 -> `ready`. No tag, crate publication, or GitHub release occurs in M014. M015 freezes one exact post-cleanup HEAD and runs the dedicated release workflow, artifact matrix, Toxiproxy differential, Eggfetch qualification, fuzz/security/package gates, and performance/docs census before any owner tag/publish/release decision.
