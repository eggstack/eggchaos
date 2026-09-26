#!/usr/bin/env sh
set -eu
cargo fmt --manifest-path benchmarks/Cargo.toml -- --check
cargo run --manifest-path benchmarks/Cargo.toml --release --quiet --bin eggchaos-benchmarks
