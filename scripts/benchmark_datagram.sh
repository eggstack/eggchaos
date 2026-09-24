#!/usr/bin/env sh
set -eu

manifest_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../benchmarks" && pwd)
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

cargo fmt --manifest-path "$manifest_dir/Cargo.toml" -- --check
cargo run --manifest-path "$manifest_dir/Cargo.toml" --release --quiet --bin datagram >"$output"
python3 - "$output" <<'PY'
import json
import platform
import statistics
import subprocess
import sys

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
report["candidate_sha"] = subprocess.check_output(
    ["git", "rev-parse", "HEAD"], text=True
).strip()
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
