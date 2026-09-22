#!/usr/bin/env sh
set -eu
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
cargo build --workspace --release
cargo package -p eggchaos-core --allow-dirty
cargo package -p eggchaos-eggfetch --allow-dirty
cargo package -p eggchaos-toxiproxy --allow-dirty
