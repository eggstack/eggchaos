#!/usr/bin/env sh
set -eu

runs="${EGGCHAOS_FUZZ_RUNS:-10000}"
(cd fuzz && cargo fuzz run plan_json --sanitizer none -- -runs="$runs")
printf '%s\n' '{"fuzz":"pass","target":"plan_json"}'
