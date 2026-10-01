#!/usr/bin/env sh
# M048 Tier A ownership: the cheap M047 Git-state provenance contract
# (test_bench_provenance.sh) belongs here because it only needs Git and
# stdlib Python and runs in seconds. The release-mode M047 artifact
# qualification (test_bench_provenance_artifacts.sh) intentionally
# stays CI-only — it would compile/run release benchmarks on every
# local full check. Hosted Tier B lives in the dedicated
# `performance-provenance` CI job.
#
# M053 ownership: the planning-state drift guard
# (test_planning_state.sh: fixture + --check + deliberate-drift
# negative test + wiring assertion) is also wired here. It needs only
# Python stdlib and well under a second, so it can run on every local
# full check. The bare --check is additionally wired into the
# `language-clients` CI job so removing the local check still trips
# an independent check.
#
# M055 ownership: the version-coherence guard
# (test_version_coherence.sh: fixture + --check + deliberate-mismatch
# negative test + wiring assertion) is wired here for the same reason.
# It needs only Python stdlib and well under a second. The bare
# --check is additionally wired into the `language-clients` CI job so
# removing the local check still trips an independent check.
#
# M059 ownership: the workspace lint-inheritance guard
# (test_lint_inheritance.sh: fixture + --check + wiring assertion) is
# wired here for the same reason. It needs only Python stdlib and well
# under a second. The bare --check is additionally wired into the
# `language-clients` CI job so removing the local check still trips
# an independent check.
#
# M059 ownership: the first-party lockfile-coherence guard
# (test_lock_coherence.sh: fixture + --check + wiring assertion) is
# wired here for the same reason. It needs only Python stdlib and well
# under a second. The bare --check is additionally wired into the
# `language-clients` CI job so removing the local check still trips
# an independent check.
#
# M059 ownership: the immutable-action / least-privilege guard
# (test_action_pins.sh: fixture + --check + wiring assertion) is wired
# here for the same reason. It needs only Python stdlib and well under
# a second. The bare --check is additionally wired into the
# `language-clients` CI job so removing the local check still trips
# an independent check.
set -eu
sh scripts/tests/test_bench_provenance.sh
sh scripts/tests/test_planning_state.sh
sh scripts/tests/test_version_coherence.sh
sh scripts/tests/test_lint_inheritance.sh
sh scripts/tests/test_lock_coherence.sh
sh scripts/tests/test_action_pins.sh
sh scripts/tests/test_release_tag_version.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
