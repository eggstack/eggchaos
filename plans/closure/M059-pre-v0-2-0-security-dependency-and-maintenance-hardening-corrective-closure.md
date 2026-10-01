# M059 — Pre-v0.2.0 Security, Dependency, and Maintenance Hardening Corrective — Closure

- Activation SHA: `0f8a8ebc8b8f734951624be32362ee512b9e3d5e`
- M057 qualified compatibility baseline: `818e5674f2efaf96ec8effda81cef1dfa7a48614`
- Exact closure candidate: `1409d0fa11dddfeff3f680ef453bf830e1339606` (`1409d0f`)
- Headline verdict: **closed**. All 30 acceptance criteria evidenced on the exact candidate. No tag or registry/GitHub publication occurred. M055–M057 closure records untouched. M058 reactivated to `ready`.

## Work-package disposition

- WP1 (baseline): native-admin auth regressions preserved (non-loopback + missing/empty token rejected, wrong Bearer rejected, correct token succeeds, token material redacted); `constant_time_equal` fixed-width SHA-256 digest baseline kept; four lockfiles carry none of the 2026-08-20 incident malicious/deleted versions (`cargo audit` clean on 210 root deps at closure).
- WP2 (lints): all eight ordinary workspace members carry `[lints] workspace = true`; `cargo clippy --workspace --all-targets --all-features -- -D warnings` green via `./scripts/check.sh`; `test_lint_inheritance.sh` pass. No public-item rename, no crate-wide new allow.
- WP3 (deps): six unused root declarations removed (`http-body-util`, `hyper`, `hyper-util`, `prometheus-client`, `rand`, `url`); first-party floors qualified within existing lines — root lock now `eggress-relay 1.0.11`, `eggfetch-core 0.2.1`, `eggserve-primitives 0.2.2`, `eggserve-server 0.2.1`; `eggserve-server 0.4.x` explicitly deferred. `socket2 0.5` direct branch removed in favor of `0.6.5` with green transport tests. Four lockfiles reconciled; `test_lock_coherence.sh` pass (local + CI-independent location).
- WP4 (Actions/monitoring): every external `uses:` pinned to a full 40-char SHA with release comment (`test_action_pins.sh` pass); least-privilege `contents: read` defaults on all four workflows; `persist-credentials: false` where no authenticated Git op follows; no `pull_request_target`. Scheduled `security.yml` (daily + dispatch) audits all four lockfiles + cargo-deny + lock-coherence guard, failing closed; PR `dependency-review.yml` with explicit threshold; Dependabot covers cargo `/`, `/bindings/python-native`, `/benchmarks`, `/fuzz`, `github-actions`, npm `/bindings/typescript-client`, pip `/.github/python-ci` (grouped weekly, bounded PRs).
- WP5 (language CI + FFI): TypeScript CI uses `npm ci --ignore-scripts --no-audit --no-fund` on the committed lock; Python CI tooling exact-pinned under `.github/python-ci`; maturin `--locked`. PyO3 boundary narrowed: crate root `deny(unsafe_code)`, item-scoped `allow` only on the macro-facing bridge (`bridge.rs`), pure helpers in `deny` modules (`convert.rs`); handwritten-unsafe audit green; structural regression pins the map.
- WP6 (API gate): `cargo-semver-checks 0.50.0` on stable toolchain, all-features, seven library crates. M057-to-activation diagnostic census = the decided 6 findings, no others (one is the intended `eggchaos-toxiproxy` compat-helper `Result` correction, classified false-positive-free and preserved). Final candidate: `{"rust_api_gate":"pass"}` against both M057 and activation baselines; dedicated `api-gate` CI job green hosted.
- WP7 (reporting + provenance): `SECURITY.md` names GitHub private vulnerability reporting (no invented contacts), supported-line table, no-public-issue rule. Release binaries carry Sigstore attestations (`actions/attest-build-provenance@4d10147…` v4.2.2) alongside unchanged SHA-256 sidecars; attestation/OIDC scopes (`id-token`/`attestations: write`) live only on the artifacts job. Hosted `gh attestation verify` of the native-host binary green (required adding `GH_TOKEN: ${{ github.token }}` — the only dispatch failure observed, on run `36919226670`, fixed in candidate `1409d0f`).
- WP8 (exact-head qualification): full gate below, all on `1409d0f`.

## Local exact-candidate evidence (`1409d0f`, clean tree)

- `python3 scripts/check_planning_state.py --check` → 60 milestones, 4 docs match
- `python3 scripts/check_version_coherence.py --check` → pass, workspace `0.2.0`, 8 crates
- `sh scripts/tests/test_release_tag_version.sh` → pass (incl. 5 negative tests)
- `./scripts/check.sh` → pass (fmt/clippy/test/doc + provenance/planning/version guards)
- `./scripts/check_openapi.sh` → pass, 21 paths / 36 ops
- `./scripts/check_python_client.sh` → pass; `./scripts/check_typescript_client.sh` → pass
- `./scripts/check_python_native.sh` → pass (10 tests)
- `RUSTUP_TOOLCHAIN=stable ./scripts/qualify_rust_api.sh` → pass (M057 census = decided 6)
- `./scripts/release-smoke.sh` → pass (order proof `core->experiment/eggfetch->protocol->server/toxiproxy/cli->embed`)
- `./scripts/qualify_language_clients.sh` → pass; `./scripts/qualify_python_native.sh` → pass
- `./scripts/qualify_eggfetch.sh` → pass; `./scripts/release-artifact-smoke.sh` → pass
- `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh` → pass, 9 targets
- `qualify_toxiproxy_v2_12.sh` (mandatory oracle, toxiproxy-server 2.12.0 checksum-verified) → differential pass
- `qualify_toxiproxy_post_v2_12.sh` (mandatory oracle, `40f7fd31`) → differential pass
- `test_lint_inheritance.sh` / `test_lock_coherence.sh` / `test_action_pins.sh` → pass
- `cargo audit --deny warnings` (210 deps, 1279 advisories) → clean; `cargo deny check advisories licenses bans sources` → ok
- All four workflow files YAML-parse; `security.yml` + `dependency-review.yml` manually verified runnable.

## Hosted exact-candidate evidence (head SHA `1409d0f…`)

- Ordinary CI push `36922662654` → **success 15/15** on the exact SHA (3 `check` + 1 `performance-provenance` + 8 `language-clients` + 2 `python-native` + 1 `api-gate`; the previously flaky `(ubuntu, 3.11, 22)` leg green after the spawned-server torn-read hardening in `336e592`).
- Release `workflow_dispatch` `36922672946` → **success 7/7** on the exact SHA (`release-contract` + `qualify` + five-target `artifacts`, incl. attestation generation + native-host hosted verification).
- Prior dispatch `36919226670` on `336e592` is superseded: `qualify` was green but the new verify step failed for missing `GH_TOKEN`; the fix is workflow-only and requalified above.

## No-release confirmation

- `git tag --list v0.2.0` empty locally; remote has only `v0.1.0`. No crates.io/PyPI/npm/GitHub-Release action taken. Workspace stays unreleased `0.2.0`.

## Action pin map (reviewed release → SHA)

- `actions/checkout` v7.0.1 → `3d3c42e…`
- `dtolnay/rust-toolchain` stable (master 2026-10-01) → `02cb101…` (toolchain stays exactly `1.89.0`)
- `Swatinem/rust-cache` v2.9.2 → `6323deb…`
- `actions/setup-python` v7.0.0 → `5fda3b9…`; `actions/setup-node` v7.0.0 → `8207627…`
- `actions/upload-artifact` v7.0.1 → `043fb46…`
- `actions/attest-build-provenance` v4.2.2 → `4d10147…`
- `actions/dependency-review-action` v5.0.0 → `a1d282b…`

## Final statement

Every M059 release-blocking criterion passed on the exact candidate with hosted CI + dispatch green after hardening. **M058 is reactivated to `ready`** and owns the irreversible `v0.2.0` tag, crates.io graph, GitHub Release, and fresh-install verification. No other successor is activated.
