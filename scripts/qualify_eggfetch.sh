#!/usr/bin/env sh
set -eu
cargo test -p eggchaos-eggfetch --all-features
cargo test -p eggchaos-server --all-features
