# Eggchaos v0.2.1 release notes

Status: **published**. `v0.2.1` is the current Eggchaos release (annotated
tag `v0.2.1` on candidate `b0bc9b1c3aeac6855ea4b82ea71a78f141197828`, GitHub
Release [eggchaos v0.2.1](https://github.com/eggstack/eggchaos/releases/tag/v0.2.1)).

`v0.2.1` is a patch release in the v0.2 series. It updates aligned
first-party package metadata and dependency maintenance while preserving the
published v0.2 native API, configuration schema, deterministic contracts, and
Toxiproxy compatibility profiles.

## Changes

- Refreshes the Rust dependency graph, including the RustSec-mitigating
  `rustix` update from the consolidated Dependabot changes.
- Aligns all eight Rust workspace crates, the standalone native crate, and
  source metadata for the Python and TypeScript clients at `0.2.1`.
- Updates the direct EggServe primitives requirement to `0.2.3`. The current
  used Eggstack dependencies are `eggress-relay 1.0.12`, `eggfetch-core
  0.3.0`, `eggserve-server 0.5.0`, and `eggserve-primitives 0.2.3`.
- Repairs the README quickstart and updates release, package, and dependency
  documentation to match the shipped workflow and current dependency graph.

No Rust runtime source, native wire operation, configuration schema, RNG
contract, OpenAPI surface, artifact target, or release publication control
changed for this patch. The Apple release builds move from GitHub's retired
macOS 14 runners to macOS 15 without changing their targets or artifact
contracts.

## Distribution

The Rust workspace crates are published on crates.io at `0.2.1` in this
dependency order: `eggchaos-core`, `eggchaos-experiment`, `eggchaos-eggfetch`,
`eggchaos-protocol`, `eggchaos-server`, `eggchaos-toxiproxy`, `eggchaos-cli`,
`eggchaos-embed`.

The GitHub Release includes five qualified CLI binaries and their SHA-256
sidecars for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`x86_64-apple-darwin`, `aarch64-apple-darwin`, and
`x86_64-pc-windows-msvc`. Artifacts come from the tag-triggered release
workflow; use the sidecars to verify downloads.

Install the CLI with:

```sh
cargo install eggchaos-cli --version 0.2.1 --locked
```

Eggchaos requires Rust 1.89 or newer when building from source. It remains a
fixed-target relay, not a forward proxy or TLS-intercepting proxy. Strict
Toxiproxy v2.12 compatibility remains the default; the pinned post-v2.12
`packet_loss` profile remains opt-in.

The Python remote client, TypeScript remote client, and native Python pilot
remain source-versioned at `0.2.1` but are not published to PyPI/npm by this
release.
