#!/usr/bin/env sh
set -eu
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
cargo build --workspace --release
cargo audit --deny warnings
cargo deny check advisories licenses bans sources
cargo package -p eggchaos-core --allow-dirty
cargo package -p eggchaos-core --list --allow-dirty
cargo package -p eggchaos-server --list --allow-dirty
cargo package -p eggchaos-eggfetch --list --allow-dirty
cargo package -p eggchaos-toxiproxy --list --allow-dirty
cargo package -p eggchaos-cli --list --allow-dirty
cargo build --release --locked --package eggchaos-cli
./scripts/release-artifact-smoke.sh
cargo package -p eggchaos-eggfetch --allow-dirty
cargo package -p eggchaos-toxiproxy --allow-dirty
