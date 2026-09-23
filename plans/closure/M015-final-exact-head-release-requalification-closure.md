# M015 Closure — Final Exact-HEAD Release Requalification: CLEAN VERDICT

Milestone: M015 (`015-final-exact-head-release-requalification.md`)
Candidate: `cd88b2290047497d6090f86c0389d9d0c3335e6f` (exact post-M014 HEAD; workflow/script/example-target fixes only — no fault, runtime, API, dependency-version, or test-logic change vs the M014 HEAD).
Implementation commits: `2c66f48` (release-workflow + package-proof fixes), `cd88b22` (stale example removal); this closure record follows in the same change family (planning records only).
Commands executed: see Verification.
Platforms: darwin/arm64 local gates; Ubuntu/macOS/Windows ordinary CI; Ubuntu release-qualification lane; five-target artifact matrix.
External oracle/version: pinned `toxiproxy-server version 2.12.0`, SHA-256 `aa299966b52f16a8594f1cd0d1e9049dc2e8fe2c04a90c19860e2719b2b95d15` (darwin/arm64 local oracle binary at `/tmp/oracle/toxiproxy-server`).
Evidence artifacts: CI run IDs/URLs, release-workflow run ID/URL + job IDs, artifact binaries + `.sha256` files + sizes, differential transcript (`/tmp/qualify_m012.log`), client-smoke transcripts (`/tmp/go-m015c.json`, `/tmp/py-m015c.json`), fuzz transcript, benchmark output.
Registry transition: M015 `ready` -> `closed`.
Next milestone activated: none — no further planned pre-tag implementation work. Tagging, crates.io publication, and GitHub release creation remain explicit owner decisions.

## Candidate freeze

- M015 candidate: `cd88b2290047497d6090f86c0389d9d0c3335e6f` ("M015: remove stale compat-server example shadowing compat_server (Windows link collision)").
- Parent chain from the M008 candidate: `645a761` (M008) -> `c42a198` (closure bookkeeping) -> `00d1ee4` (eggserve registry switch) -> `22136c0` (bounded H2/CI timeouts) -> `a8933f3` (deterministic H2 server close) -> `233202a`/`92f1f7a`/`5565c54`/`6f2eebd`/`33880db`/`e237daa` (M014/M015 scaffolding) -> `37dc28b` (M014 reconciliation) -> `47073c1` (M014 SHA annotation, planning-only) -> `2c66f48` (release-workflow + package-proof fixes) -> `cd88b22` (candidate).
- Workflow/config identity on the candidate: `.github/workflows/release.yml` with stable-toolchain cargo-fuzz install, aarch64 cross-linker env, and macos-14 x86_64 cross-build; `scripts/release-smoke.sh` with order-aware package proof.
- Working tree frozen: `git status` clean at dispatch; no uncommitted work. No fix landed after candidate selection; this closure record itself is the only follower and touches planning records only.

## Dedicated release workflow: GREEN

- Run: `https://github.com/eggstack/eggchaos/actions/runs/35810310455` (workflow_dispatch on `main` = candidate `cd88b22`; dispatched after the push, no intervening commits).
- Conclusion: `completed/success`. Qualification lane and artifact matrix both green.
- Qualify job (`107020191090`, 13m18s, ubuntu, all 13 steps green):
  - checkout, toolchain 1.89.0, cargo-audit install, cargo-deny install,
  - stable-toolchain install + `RUSTUP_TOOLCHAIN=stable cargo install cargo-fuzz --locked --version 0.13.2`,
  - `./scripts/release-smoke.sh` (fmt, clippy `-D warnings`, full workspace tests, docs, release build, audit, deny, core package, per-crate `--list`, CLI release build, artifact smoke, order proof),
  - `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh`,
  - `./scripts/qualify_toxiproxy_v2_12.sh` (translation suite; differential incomplete-without-oracle in CI by design — pinned-oracle differential evidence is the exact-candidate local run below, per M008/M013 precedent),
  - `./scripts/qualify_eggfetch.sh`,
  - `./scripts/release-artifact-smoke.sh`.
- Artifact jobs (all `completed/success`):
  - `107020191264` ubuntu-22.04 / x86_64-unknown-linux-gnu,
  - `107020191366` ubuntu-22.04 / aarch64-unknown-linux-gnu,
  - `107020191219` macos-14 / x86_64-apple-darwin (cross-build from ARM64 host),
  - `107020191242` macos-14 / aarch64-apple-darwin,
  - `107020191294` windows-2022 / x86_64-pc-windows-msvc.

## Ordinary CI gate: GREEN

- Run `35810065730` on candidate `cd88b22`: `completed/success` — `check (ubuntu-latest)`, `check (macos-latest)`, and `check (windows-latest)` all success (fmt, clippy `-D warnings`, full workspace tests, docs, audit, deny).
- Release-workflow success did not excuse ordinary CI: both are green on the same exact SHA.

## External qualification (exact candidate, darwin/arm64)

### Toxiproxy pinned v2.12.0 oracle

- `TOXIPROXY_SERVER=/tmp/oracle/toxiproxy-server ./scripts/qualify_toxiproxy_v2_12.sh`: `{"translation":"pass","oracle":"toxiproxy-server 2.12.0","differential":"pass"}`.
- Differential transcript: 47 passed, 0 failed, 4 declared normalizations (disjoint-bind listen replacement; f64 number canonicalization; toxicity clamping; degenerate zero coalescing) — identical normalization set to M008/M012/M013.
- Oracle identity: `toxiproxy-server version 2.12.0`, SHA-256 `aa299966b52f16a8594f1cd0d1e9049dc2e8fe2c04a90c19860e2719b2b95d15`, matching the M012/M013 baseline records.
- Declared M012/M013 compatibility surface not regressed.

### Client smokes (fresh, against `compat_server` built from the candidate)

- Go pinned client `github.com/Shopify/toxiproxy/v2@v2.12.0` (go1.27.1): 13 steps, 0 failed (`/tmp/go-m015c.json`).
- Independent Python-stdlib client (Python 3.14.2, urllib only): 12 steps, 0 failed (`/tmp/py-m015c.json`). The historical "13/13" shorthand covered the same 12-step script; exact step counts are reported here.
- Server under test: `cargo run -p eggchaos-toxiproxy --example compat_server` from the candidate tree (the documented example; the stale `compat-server` duplicate was removed in this candidate family — see repairs).

### Eggfetch

- `./scripts/qualify_eggfetch.sh`: all suites pass — adapter lib 3, regression 10 (H1 keep-alive live update, HTTPS trust/rejection, H2 concurrency, blackhole, mid-response termination, shaping, redial, dial errors, disconnect termination), server 37, toxiproxy lib 10.
- The post-M008 deterministic H2 test-transport fix (`a8933f3`) is covered: the H2 regression tests pass on the exact candidate locally and in all three ordinary-CI platforms.

## Fuzz, security, and package gates (exact candidate)

- `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh`: `{"fuzz":"pass","target":"plan_json"}` (10,000 runs, 0 crashes; `fuzz/artifacts` empty; run-generated corpus byproducts removed, only tracked `fuzz/corpus/plan_json/seed.json` retained).
- `cargo audit --deny warnings`: exit 0 (205 crate dependencies, 1264 advisories loaded, no vulnerabilities).
- `cargo deny check advisories licenses bans sources`: advisories ok, bans ok, licenses ok, sources ok (registry-only; stale eggserve git allowlist removed in M014).
- `cargo package -p eggchaos-core --allow-dirty`: PASS (`eggchaos-core-0.1.0.crate` builds).
- `cargo package -p <each> --list --allow-dirty` for all five crates (core, server, eggfetch, toxiproxy, cli): PASS.
- Order proof (new in `scripts/release-smoke.sh`, passing in the release workflow): every intra-workspace path dependency requires the workspace version `^0.1.0` from the registry, so the documented publish order core -> eggfetch -> server/toxiproxy/cli resolves. Dependents' full `cargo package` can only succeed after predecessors publish (Cargo registry resolution); that post-publication check is an owner release-step action. Nothing is published in M015.
- `cargo metadata --format-version 1 --locked` resolves; `grep 'source = "git' Cargo.lock` empty (zero git sources); `cargo tree --locked` registry-only for all Eggstack seams (`eggserve-*` 0.2.0, `eggfetch-core` 0.2.0, `eggress-relay` 1.0.7).

## Artifact evidence

From release run `35810310455` (downloaded to `/tmp/m015-artifacts`, names carry the `vmain` dispatch-ref prefix; tag-push builds will carry the version):

| Target | Job | Binary | Size (bytes) | SHA-256 |
| --- | --- | --- | --- | --- |
| x86_64-unknown-linux-gnu | 107020191264 | `eggchaos-vmain-x86_64-unknown-linux-gnu` | 6068168 | `1fa64f163efa00c12b6f3a66a16451f6c29d83120f2241f86702a2d4bddf68c4` |
| aarch64-unknown-linux-gnu | 107020191366 | `eggchaos-vmain-aarch64-unknown-linux-gnu` | 5293880 | `3739e358d7f887f6384c16d8c93fb299aa0ce811b34c74bd33e6f2c1a7f84b88` |
| x86_64-apple-darwin | 107020191219 | `eggchaos-vmain-x86_64-apple-darwin` | 5694616 | `7951f6d784e59797ceab20f46b0bc9ace13831486be8fa0b47044bdedecd4793` |
| aarch64-apple-darwin | 107020191242 | `eggchaos-vmain-aarch64-apple-darwin` | 5181472 | `f5a343383b238c82882ea3bdd025d628ebcf8ea1b7f2ffb3307fc3c8494f39ba` |
| x86_64-pc-windows-msvc | 107020191294 | `eggchaos-vmain-x86_64-pc-windows-msvc.exe` | 6763520 | `9216842545a6b98dad1f2826b365c0ab97501df74b97af5745685c37b614ce6e` |

- Each artifact ships with a matching `.sha256` file (recorded above verbatim).
- Linux binaries verified ELF (`x86-64` / `ARM aarch64`, `file` magic); ubuntu x86_64 binary runtime smoke-tested by `release-artifact-smoke.sh` inside the green qualify lane. macOS/Windows/aarch64-Linux artifacts are build-verified with checksums; foreign execution smoke is not claimed (same standard as M008).

## Performance sanity

- `./scripts/benchmark.sh` on the candidate (darwin/arm64, 8 MiB, 3 rounds): empty-plan 1921.8 MiB/s vs bare relay 2340.6 MiB/s, ratio 0.821 ≥ 0.70 budget. PASS.
- An earlier same-day run measured 0.878; both clear the frozen M008 budget (empty-plan mean ≥ 70% of same-session bare relay; fault delay excluded).
- The candidate family contains no hot-path changes vs M008 (workflow YAML, packaging script, one example file, test bounds, dependency registry switch), so the sanity measurement — not a full matrix rerun — is the proportionate evidence per the plan.

## Documentation/version census

- Workspace version `0.1.0`, edition 2021, `rust-version = "1.89"`; CI/release toolchains pinned `1.89.0` (product gates) with the single documented exception (cargo-fuzz binary built under current stable; fuzz target still gated on 1.89.0).
- `docs/toxiproxy.md` divergences, `docs/architecture.md` hard-reset/platform separation, `docs/configuration.md` schema-v1 wording, and root README release-state wording all match the implementation and the M008 limitations record. No "fully compatible" or packet-level overclaim.
- No current text calls `645a761` the final release candidate: every mention is historical with M015 named as the final pre-tag exact-commit authority (verified by grep census).
- Owner-action separation is explicit in the M008 closure, M014 closure, root README, plans README, roadmap, and registry: qualification is complete; tag/publication/GitHub-release creation are owner decisions after M015.
- Registry identifies M015 as `closed` with this record; no `blocked` pre-tag milestone remains.

## Repairs found and fixed during M015 (narrow, workflow/example only)

Three latent release-workflow defects and one repo defect were found because the dedicated release workflow had never been dispatched before (0 prior runs). All are test/workflow/example-tree issues; none changes fault semantics, runtime behavior, APIs, or dependency versions. Each fix reselected the candidate and reran every affected gate per the plan's preconditions:

1. `cargo install cargo-fuzz --locked --version 0.13.2` under pinned rustc 1.89.0 fails deterministically (`cargo-platform@0.3.3 requires rustc 1.91`). Fixed in `2c66f48`: build the cargo-fuzz binary with current stable (`RUSTUP_TOOLCHAIN=stable`), still driving the 1.89.0-gated fuzz target. Evidence: first release attempt run `35807919903` qualify failure log; green rerun `35810310455`.
2. Linux aarch64 artifact linked with host `cc` (`file in wrong format`). Fixed in `2c66f48`: `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc` job env. Evidence: attempt-1 aarch64 job log; green rerun job `107020191366`.
3. `macos-13` Intel runner never picks up the x86_64-apple-darwin artifact job (queued indefinitely; image retired). Fixed in `2c66f48`: build on `macos-14` (ARM64 host cross-compiles `--target x86_64-apple-darwin`). Evidence: attempt-1 stuck queue (run cancelled after evidence capture); green rerun job `107020191219`.
4. `scripts/release-smoke.sh` demanded full `cargo package` of dependents pre-publication, which Cargo cannot satisfy before `eggchaos-core` 0.1.0 exists on the registry (`no matching package named eggchaos-core found`, reproduced locally). Fixed in `2c66f48`: order-aware proof (core full package + per-crate `--list` + intra-workspace `^0.1.0` version-req assertion). Evidence: local reproduction; green in release run `35810310455`.
5. Duplicate example targets `compat_server.rs` (current, documented) and `compat-server.rs` (stale) normalize to the same output name and collide at link time on Windows (`LNK1104`, ordinary CI failure on `2c66f48`, run `35809699186`). Fixed in `cd88b22` (the frozen candidate): deleted the stale file; all references already pointed at `compat_server`. Evidence: Windows job log; green ordinary-CI rerun `35810065730` on all three platforms.

Superseded evidence (kept for traceability, not combined into the verdict): release attempt `35807919903` (cancelled after failure capture: qualify infra failure + aarch64 link failure + retired-runner queue) and ordinary-CI run `35809699186` (Windows link collision). The verdict rests solely on the frozen candidate `cd88b22` runs above.

## Acceptance-criteria verdict

M015 acceptance is met on the single exact candidate `cd88b22`: ordinary Linux/macOS/Windows CI green; dedicated release workflow green including the qualification lane (release smoke, fuzz 10k, Toxiproxy translation, Eggfetch, artifact smoke) and every required artifact job (5/5 targets with checksums); Toxiproxy 47/47 differential plus Go 13/13 and Python 12/12 smokes; fuzz/security/package gates green with an order-publishable registry graph; artifacts/checksums/target evidence recorded; performance 0.821 within the 0.70 budget; docs identify the M015 candidate as the final pre-tag authority; no unresolved correctness or workflow finding remains.

## Follow-on activation

M015 -> `closed`. No further planned pre-tag implementation work. By explicit owner decision the repository may now create the `v0.1.0` tag, publish crates in the verified order (core -> eggfetch -> server/toxiproxy/cli), and create the GitHub release with the qualified artifacts/checksums. Post-publication, dependents' full `cargo package` should be re-verified as an owner release-step action.
