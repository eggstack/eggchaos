#!/usr/bin/env sh
# M048 Tier A ownership: the cheap M047 Git-state provenance contract
# (test_bench_provenance.sh) belongs here because it only needs Git and
# stdlib Python and runs in seconds. The release-mode M047 artifact
# qualification (test_bench_provenance_artifacts.sh) intentionally
# stays CI-only — it would compile/run release benchmarks on every
# local full check. Hosted Tier B lives in the dedicated
# `performance-provenance` CI job.
set -eu
sh scripts/tests/test_bench_provenance.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
