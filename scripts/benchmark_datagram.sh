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
direct_p95 = statistics.median(values("direct_udp_echo", "p95_micros"))
proxy_p95 = statistics.median(values("fixed_target_empty_plan", "p95_micros"))
throughput_ratio = proxied / direct
p95_ratio = proxy_p95 / max(direct_p95, 1)
summary = {
    "datagram_budget": "pass",
    "platform": report["platform"],
    "arch": report["arch"],
    "direct_median_datagrams_s": round(direct, 2),
    "empty_plan_median_datagrams_s": round(proxied, 2),
    "empty_plan_throughput_ratio": round(throughput_ratio, 4),
    "direct_median_p95_us": round(direct_p95, 2),
    "empty_plan_median_p95_us": round(proxy_p95, 2),
    "p95_latency_ratio": round(p95_ratio, 4),
    "thresholds": {"minimum_throughput_ratio": 0.45, "maximum_p95_ratio": 2.5},
}
if throughput_ratio < 0.45 or p95_ratio > 2.5:
    summary["datagram_budget"] = "fail"
    print(json.dumps(summary, sort_keys=True))
    sys.exit(1)
print(json.dumps(summary, sort_keys=True))
PY
