# M058 — v0.2.0 Release Publication and Distribution — Closure

- Frozen candidate: `b6a277d5ad4267bd602bc15a4333b14322057b90` (`b6a277d`)
- Verdict: **closed**. All 17 acceptance criteria met. `v0.2.0` is the current published Eggchaos release.

## Pre-tag gate on the exact candidate (`b6a277d`, clean tree)

Local (all pass):

- `python3 scripts/check_planning_state.py --check` → 60 milestones, 4 docs match
- `python3 scripts/check_version_coherence.py --check` → workspace `0.2.0`, 8 crates
- `sh scripts/tests/test_release_tag_version.sh` → pass (incl. 5 negative tests)
- `./scripts/check.sh` → pass
- `./scripts/check_openapi.sh` → 21 paths / 36 ops
- `./scripts/check_python_client.sh`, `./scripts/check_typescript_client.sh`, `./scripts/check_python_native.sh` → pass
- `RUSTUP_TOOLCHAIN=stable ./scripts/qualify_rust_api.sh` → pass (`cargo-semver-checks 0.50.0`)
- `./scripts/release-smoke.sh` → pass (order `core->experiment/eggfetch->protocol->server/toxiproxy/cli->embed`)
- `./scripts/qualify_language_clients.sh`, `./scripts/qualify_python_native.sh`, `./scripts/qualify_eggfetch.sh`, `./scripts/release-artifact-smoke.sh` → pass
- `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh` → 9 targets pass
- `qualify_toxiproxy_v2_12.sh` (mandatory oracle 2.12.0, checksum-verified) → differential pass
- `qualify_toxiproxy_post_v2_12.sh` (mandatory oracle `40f7fd31`) → differential pass

Hosted on the exact SHA:

- Ordinary CI push `36931770121` → **success 15/15** (3 `check` + 1 `performance-provenance` + 8 `language-clients` + 2 `python-native` + 1 `api-gate`)
- Release `workflow_dispatch` `36931798010` → **success 7/7** (`release-contract` + `qualify` + five-target `artifacts`, incl. Sigstore attestation generation + native-host hosted verification)

## Package graph verification (WP4)

- `cargo package --list` for core/experiment/eggfetch/protocol (+ dependents at publish time) → clean manifests
- `cargo publish --dry-run --locked` green for `eggchaos-core` pre-publication and for every dependent immediately before its real publish, against already-published predecessors
- Intra-workspace deps carry exact `version = "0.2.0"` registry reqs; no requirement weakening
- crates.io preflight 2026-10-01: no `0.2.0` existed on any of the eight names; `experiment`/`protocol`/`embed` were new names (404) as expected for the required scope

## Language-package decisions (WP1, owner-explicit 2026-10-01)

| Package | Registry | Disposition |
| --- | --- | --- |
| `eggchaos-client` | PyPI | **defer** — name unclaimed (404); M058 does not allocate a new public namespace implicitly |
| `@eggstack/eggchaos-client` | npm | **defer** — no scope/ownership/provenance go |
| `eggchaos-native` | PyPI | **defer by default** — alpha pilot, no release-grade wheel matrix |

Sources at the tag stay `0.2.0`-aligned. Deferral is recorded in `docs/release-notes-v0.2.0.md` and the GitHub Release notes.

## Tag (WP6)

- Annotated `v0.2.0`: object `15746dc2f12e3ddc5557c7afc43d19289fd32472`, peels to `b6a277d5ad4267bd602bc15a4333b14322057b90`
- Preconditions verified: no local/remote `v0.2.0` existed, tree clean at the frozen candidate, workspace exactly `0.2.0`
- Pushed tag only; tag never moved

## Tag-triggered release (WP7)

- Tag CI `36935298504` → **success 15/15** on `b6a277d`
- Tag release `36935298484` → **success 7/7** on the tag (`release-contract` proves tag `0.2.0` == workspace `0.2.0`; `qualify` + all five artifact legs green)
- Downloaded tag artifacts: exactly the 5 binary/checksum pairs below; every sidecar validates; filenames encode `v0.2.0` + triple; native-host binary reports `{"api":"v1","version":"0.2.0"}`

## Artifacts (WP7/WP10)

| File | SHA-256 |
| --- | --- |
| `eggchaos-v0.2.0-x86_64-unknown-linux-gnu` | `3f6b8f2eb3e93aa124ad21290930f968219f3e15e178ce3630fe03e547a7fd00` |
| `eggchaos-v0.2.0-aarch64-unknown-linux-gnu` | `658ad08b124fb98cd86d0bbf5ae21c839766456edda7cb68e5eff86ae6e0618c` |
| `eggchaos-v0.2.0-x86_64-apple-darwin` | `9626d319dccec5d091f18026a65a2c524099f63dd79aa8d12969e2bb64a2aef7` |
| `eggchaos-v0.2.0-aarch64-apple-darwin` | `c05733fd6bd286eae256ea0e6b33c41325a75db7818288727b13ae1c0e871b56` |
| `eggchaos-v0.2.0-x86_64-pc-windows-msvc.exe` | `ee67e3f15d9699bfb23429bb0d3525fb3d914bb99be925f5a15ae11103537c60` |

Each binary additionally carries a Sigstore attestation (`gh attestation verify <binary> --repo eggstack/eggchaos`); checksums remain the primary contract.

## Rust publication ledger (WP8, all `--locked` from the exact tagged source)

Order executed: `core -> experiment -> eggfetch -> protocol -> server -> toxiproxy -> cli -> embed` (each: dry-run against published predecessors → publish → index resolution verified before the dependent).

- `eggchaos-core 0.2.0`, `eggchaos-experiment 0.2.0` (new name), `eggchaos-eggfetch 0.2.0`, `eggchaos-protocol 0.2.0` (new name), `eggchaos-server 0.2.0`, `eggchaos-toxiproxy 0.2.0`, `eggchaos-cli 0.2.0`, `eggchaos-embed 0.2.0` (new name) — all publicly listed; no retries, no yanks
- Post-publish registry census: core/eggfetch/server/toxiproxy/cli at [`0.1.0`, `0.2.0`]; experiment/protocol/embed at [`0.2.0`]

## GitHub Release (WP10)

- `eggchaos v0.2.0` on immutable tag `v0.2.0`: https://github.com/eggstack/eggchaos/releases/tag/v0.2.0
- Exactly the 10 qualified tag-workflow files attached (5 binaries + 5 `.sha256`); no local rebuilds
- Notes cover additions, targets, MSRV 1.89+, strict/opt-in Toxiproxy distinction, fixed-target limits, crates.io installs, language deferrals, checksum verification

## Fresh-publication verification (WP11)

- `cargo install eggchaos-cli --version 0.2.0 --locked` into an isolated root from the public index → installed; `--json version` → `0.2.0`; `--help` OK
- Fresh binary: minimal TCP + UDP proxy startup/health, `proxy list` (`smoke`), `datagram proxy list` (`udp-smoke`), `reset` → all pass (via `release-artifact-smoke.sh` against `qualification/release/eggchaos.toml`)
- Public-release download (`releases/download/v0.2.0`, aarch64-apple-darwin): checksum OK, same artifact smoke pass
- External Cargo fixture depending on `eggchaos-core/server/eggfetch/toxiproxy = "0.2.0"` resolves from the registry and builds locked
- No language package published, so no registry-install smoke applies; deferrals documented

## Post-publication documentation (WP12)

- `README.md`: latest published release `0.2.0`, `cargo install eggchaos-cli --version 0.2.0 --locked`; "do not install 0.2.0" wording removed
- `architecture/tooling-distribution.md`, `architecture/overview.md`, `plans/roadmap.md`: release-state prose says `v0.2.0` published; `v0.1.0` kept as immutable history
- M055–M057 historical records untouched; post-tag docs commit recorded below

## Limitations

- Tag-workflow `qualify` replays the release lane on hosted runners; wall-clock-sensitive assertions carry their committed tolerances and are not sole evidence (deterministic unit/proptest/fuzz evidence is in the local gate)
- `reset_peer`/hard-reset stays best-effort/platform-qualified; ordinary `poll_shutdown` is not TCP RST
- Language registries deferred (see table); `eggchaos-native` remains an alpha pilot

## Final verdict

`v0.2.0` is published and verified on all required surfaces. M058 activates no automatic feature successor; post-release work resumes from a fresh planning decision, and release defects require a patch-release corrective.
