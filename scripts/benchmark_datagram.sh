#!/usr/bin/env sh
# M047 datagram benchmark wrapper: annotates the report with the shared
# provenance object from scripts/bench_provenance.py.
#
# Top-level `candidate_sha` is retained for compatibility and always equals
# `provenance.head_sha`; it names the base HEAD, not a clean-tree proof.
# Only `provenance.authoritative == true` (clean source-relevant tree)
# qualifies as exact-candidate evidence.
#
# Authoritative guard:
#   EGGCHAOS_BENCH_REQUIRE_CLEAN=1 ./scripts/benchmark_datagram.sh
# Dirty source state fails before benchmark execution in this mode.
# Without it, dirty runs proceed but are marked authoritative:false with a
# source fingerprint plus a stderr warning.
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
manifest_dir="$repo_root/benchmarks"
output="${EGGCHAOS_DATAGRAM_BENCH_OUTPUT:-}"
if [ -z "$output" ]; then
  output="$(mktemp)"
  cleanup_output=1
else
  cleanup_output=0
fi
cleanup() {
  if [ "$cleanup_output" = 1 ]; then rm -f "$output"; fi
}
trap cleanup EXIT HUP INT TERM

require_clean="${EGGCHAOS_REQUIRE_CLEAN:-${EGGCHAOS_BENCH_REQUIRE_CLEAN:-}}"
for arg in "$@"; do
  case "$arg" in
    --require-clean) require_clean=1 ;;
    *) echo "benchmark_datagram.sh: unknown argument '$arg'" >&2; exit 2 ;;
  esac
done

provenance_envelope=$(python3 "$script_dir/bench_provenance.py" --json --exclude "$output")
provenance_json=$(printf '%s' "$provenance_envelope" | python3 -c "import json,sys; print(json.dumps(json.load(sys.stdin)['provenance'], sort_keys=True))")
worktree=$(printf '%s' "$provenance_json" | python3 -c "import json,sys; print(json.loads(sys.stdin.read())['worktree'])")
head_sha=$(printf '%s' "$provenance_json" | python3 -c "import json,sys; v=json.loads(sys.stdin.read())['head_sha']; print(v if v else 'unknown')")

if [ "$worktree" != "clean" ]; then
  fingerprint=$(printf '%s' "$provenance_json" | python3 -c "import json,sys; v=json.loads(sys.stdin.read())['source_fingerprint']; print(v if v else 'none')")
  if [ -n "$require_clean" ] && [ "$require_clean" != "0" ]; then
    echo "benchmark_datagram.sh: refusing canonical evidence run on dirty source tree (head=$head_sha fingerprint=$fingerprint)" >&2
    exit 2
  fi
  echo "benchmark_datagram.sh: WARNING dirty source worktree (head=$head_sha fingerprint=$fingerprint): report will carry authoritative:false and must not be called exact-candidate evidence" >&2
fi

export EGGCHAOS_BENCH_PROVENANCE_JSON="$provenance_json"

cargo fmt --manifest-path "$manifest_dir/Cargo.toml" -- --check
cargo run --manifest-path "$manifest_dir/Cargo.toml" --release --quiet --bin datagram >"$output"
PROVENANCE_JSON="$provenance_json" python3 - "$output" <<'PY'
import json
import os
import platform
import statistics
import subprocess
import sys

provenance = json.loads(os.environ["PROVENANCE_JSON"])
with open(sys.argv[1], encoding="utf-8") as source:
    report = json.load(source)
try:
    cpu_model = subprocess.check_output(
        ["sysctl", "-n", "machdep.cpu.brand_string"], text=True,
        stderr=subprocess.DEVNULL,
    ).strip()
except (OSError, subprocess.CalledProcessError):
    cpu_model = platform.processor() or "unknown"
report["cpu_model"] = cpu_model
report["rustc"] = subprocess.check_output(["rustc", "--version"], text=True).strip()
# Legacy compatibility: candidate_sha names the base HEAD. It equals
# provenance.head_sha and is NOT sufficient proof of a clean candidate;
# authoritative == true is the exact-candidate signal.
report["candidate_sha"] = provenance["head_sha"]
report["provenance"] = provenance
assert report["candidate_sha"] == report["provenance"]["head_sha"]
with open(sys.argv[1], "w", encoding="utf-8") as destination:
    json.dump(report, destination, indent=2, sort_keys=True)
    destination.write("\n")
samples = report["samples"]
def values(name, field):
    return [sample[field] for sample in samples if sample["name"] == name]

direct = statistics.median(values("direct_udp_echo", "datagrams_per_second"))
proxied = statistics.median(values("fixed_target_empty_plan", "datagrams_per_second"))
bare = statistics.median(values("bare_fixed_target_relay", "datagrams_per_second"))
direct_p95 = statistics.median(values("direct_udp_echo", "p95_micros"))
proxy_p95 = statistics.median(values("fixed_target_empty_plan", "p95_micros"))
bare_p95 = statistics.median(values("bare_fixed_target_relay", "p95_micros"))
direct_win = statistics.median(values("direct_windowed", "datagrams_per_second"))
bare_win = statistics.median(values("bare_windowed", "datagrams_per_second"))
empty_win = statistics.median(values("empty_plan_windowed", "datagrams_per_second"))
throughput_ratio = proxied / direct
p95_ratio = proxy_p95 / max(direct_p95, 1)
# Topology-matched ratios isolate avoidable chaos-engine/runtime overhead from
# the unavoidable extra socket hop of any fixed-target proxy.
matched_throughput = proxied / bare
matched_p95 = proxy_p95 / max(bare_p95, 1)
matched_windowed = empty_win / bare_win
summary = {
    "datagram_budget": "pass",
    "platform": report["platform"],
    "arch": report["arch"],
    "direct_median_datagrams_s": round(direct, 2),
    "bare_relay_median_datagrams_s": round(bare, 2),
    "empty_plan_median_datagrams_s": round(proxied, 2),
    "empty_plan_throughput_ratio": round(throughput_ratio, 4),
    "direct_median_p95_us": round(direct_p95, 2),
    "bare_relay_median_p95_us": round(bare_p95, 2),
    "empty_plan_median_p95_us": round(proxy_p95, 2),
    "p95_latency_ratio": round(p95_ratio, 4),
    "matched_empty_over_bare_throughput": round(matched_throughput, 4),
    "matched_empty_over_bare_p95": round(matched_p95, 4),
    "direct_windowed_median_datagrams_s": round(direct_win, 2),
    "bare_windowed_median_datagrams_s": round(bare_win, 2),
    "empty_windowed_median_datagrams_s": round(empty_win, 2),
    "matched_windowed_empty_over_bare": round(matched_windowed, 4),
    "thresholds": {
        "minimum_throughput_ratio": 0.45,
        "maximum_p95_ratio": 2.5,
        "minimum_matched_throughput_ratio": 0.7,
        "maximum_matched_p95_ratio": 1.6,
        "minimum_matched_windowed_ratio": 0.7,
    },
}
if throughput_ratio < 0.45 or p95_ratio > 2.5:
    summary["datagram_budget"] = "fail"
if (matched_throughput < 0.7 or matched_p95 > 1.6
        or matched_windowed < 0.7):
    summary["datagram_budget"] = "fail"
    summary["matched_budget"] = "fail"
else:
    summary["matched_budget"] = "pass"
if summary["datagram_budget"] == "fail":
    print(json.dumps(summary, sort_keys=True))
    sys.exit(1)
print(json.dumps(summary, sort_keys=True))
PY
