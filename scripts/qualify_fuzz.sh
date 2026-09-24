#!/usr/bin/env sh
set -eu

runs="${EGGCHAOS_FUZZ_RUNS:-10000}"
for target in plan_json datagram_plan_json datagram_transitions native_config native_control_json fault_evidence_json policy_transitions toxiproxy_attributes scenario_v2; do
  (cd fuzz && cargo fuzz run "$target" --sanitizer none -- -runs="$runs")
  printf '{"fuzz":"pass","target":"%s","runs":%s}\n' "$target" "$runs"
done
printf '%s\n' '{"fuzz":"pass","targets":9}'
