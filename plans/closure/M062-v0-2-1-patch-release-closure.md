# M062 v0.2.1 patch release closure

## Verdict

**Closed.** Eggchaos `v0.2.1` was published on 2026-11-13 from the exact
qualified candidate. All acceptance criteria in
[`062-v0-2-1-patch-release.md`](../062-v0-2-1-patch-release.md) are met.

## Candidate and tag

- Qualified release candidate and tag target:
  `b0bc9b1c3aeac6855ea4b82ea71a78f141197828`
- Annotated tag: `v0.2.1`
- Tag object: `27e7af53f78abdc08489bb2681e4b31f93af6b25`
- Documentation/status closure commit:
  `d94e30335903639620d28aaf3822d505516d8cf3`
- Hosted CI for that closure commit:
  [run 43526078850](https://github.com/eggstack/eggchaos/actions/runs/43526078850),
  green 15/15 on the exact commit.
- Tag-triggered release workflow:
  [run 43520281822](https://github.com/eggstack/eggchaos/actions/runs/43520281822)
- GitHub Release, published `2026-11-13T22:56:59Z`:
  <https://github.com/eggstack/eggchaos/releases/tag/v0.2.1>

The release-tag check confirmed that the annotated tag peeled directly to the
qualified candidate. The tag was not moved or recreated.

## Hosted qualification

| Gate | Run | Result | Candidate |
| --- | --- | --- | --- |
| Ordinary hosted CI | [43516207980](https://github.com/eggstack/eggchaos/actions/runs/43516207980) | 15/15 jobs passed | `b0bc9b1c3aeac6855ea4b82ea71a78f141197828` |
| Pre-tag release dispatch | [43517314319](https://github.com/eggstack/eggchaos/actions/runs/43517314319) | `release-contract`, `qualify`, and all five artifact jobs passed | `b0bc9b1c3aeac6855ea4b82ea71a78f141197828` |
| Tag-triggered release | [43520281822](https://github.com/eggstack/eggchaos/actions/runs/43520281822) | `release-contract`, `qualify`, and all five artifact jobs passed | `b0bc9b1c3aeac6855ea4b82ea71a78f141197828` |

The release qualification included `release-smoke.sh`, the datagram budget
benchmark, 10,000 fuzz cases, the required pinned Toxiproxy v2.12.0 differential,
Eggfetch and server tests, and the final release artifact smoke check. Every
step passed.

The first pre-tag dispatch (`43515578102`) targeted the retired `macos-14`
runner and cancelled its Apple artifact legs. M062 moved both Apple builds to
GitHub's supported `macos-15` image without changing target triples or artifact
contracts. The replacement pre-tag dispatch and the final tag-triggered run
passed on the same candidate.

## Eggstack dependency versions

Cargo registry releases were checked on 2026-11-13. Every used Eggstack crate
was current in the release candidate and its lockfiles:

| Crate | Version | Use |
| --- | --- | --- |
| `eggress-relay` | `1.0.12` | Bidirectional relay primitive; Eggchaos does not use the broader Eggress proxy/runtime crates. |
| `eggfetch-core` | `0.3.0` | CLI control HTTP and the Eggfetch `Dialer` integration. |
| `eggserve-server` | `0.5.0` | Admin HTTP/1 server. |
| `eggserve-primitives` | `0.2.3` | Admin HTTP primitives; direct workspace requirement raised from `0.2.2`. |

## Crates.io publication

All eight Rust workspace crates were published with `cargo publish --locked`
in dependency order. Cargo package verification succeeded for each crate,
and each publish completed after registry visibility:

1. `eggchaos-core 0.2.1`
2. `eggchaos-experiment 0.2.1`
3. `eggchaos-eggfetch 0.2.1`
4. `eggchaos-protocol 0.2.1`
5. `eggchaos-server 0.2.1`
6. `eggchaos-toxiproxy 0.2.1`
7. `eggchaos-cli 0.2.1`
8. `eggchaos-embed 0.2.1`

## GitHub release artifacts

The release contains these five binaries and their `.sha256` sidecars. The
listed SHA-256 value is the binary digest checked against its sidecar before
upload. Each binary and sidecar came from the successful tag-triggered
workflow; no local rebuild was attached.

| Target | Binary | SHA-256 |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | `eggchaos-v0.2.1-x86_64-unknown-linux-gnu` | `d41edadf45e67229879d1191798f78263141502caed2ddf37361523c8b85b6e6` |
| `aarch64-unknown-linux-gnu` | `eggchaos-v0.2.1-aarch64-unknown-linux-gnu` | `1416aa00ba4b4ed79e19e27f7d5b2cfc45abb71b80ee265dffbc8b2d461f481c` |
| `x86_64-apple-darwin` | `eggchaos-v0.2.1-x86_64-apple-darwin` | `1a2f2d1ba85cfcf481daf4972a60ac65f5148a7177ac3cfb6aae1a28c930a130` |
| `aarch64-apple-darwin` | `eggchaos-v0.2.1-aarch64-apple-darwin` | `c2634c96e295b35ec18a94a2db522e3f09d690b47572f66d5d6dea04be1fff23` |
| `x86_64-pc-windows-msvc` | `eggchaos-v0.2.1-x86_64-pc-windows-msvc.exe` | `c6d2dfdcb787f341f91d896fb0e1de17f6ce2743904b0d40b386493b40570ff8` |

GitHub's uploaded asset digests confirm all ten release files are present:
five target binaries and five sidecars.

## Fresh install and repository checks

- A separate `CARGO_HOME` downloaded the published `eggchaos-cli 0.2.1` from
  crates.io and installed it with `cargo install --locked`.
- `/tmp/eggchaos-cli-install-v0.2.1/bin/eggchaos --json version` returned
  `{"api":"v1","version":"0.2.1"}`.
- Before tag creation, version coherence, all four lockfile coherence checks,
  planning state, release-state docs, OpenAPI drift, package contents, and
  ordinary hosted CI passed on the release candidate.
- Current release, security support, dependency, planning, and architecture
  documentation have been reconciled. `v0.2.0` remains historical, and
  PyPI, npm, and native Python publication remain deferred under M058.
- The published GitHub Release notes were synchronized with
  `docs/release-notes-v0.2.1.md` after adding its current-publication status.

## Follow-on

M062 activates no automatic successor. A defect in a published `0.2.1`
artifact requires a new patch release; the immutable tag and crates.io
versions must not be rewritten.
