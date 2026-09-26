#!/usr/bin/env sh
# M047 stream benchmark wrapper: injects shared provenance into the
# stdout case report and stderr probe report via EGGCHAOS_BENCH_PROVENANCE_JSON.
#
# Console default (developer exploratory):
#   ./scripts/benchmark.sh
#
# Canonical retained-evidence artifact mode (both documents retained):
#   EGGCHAOS_STREAM_BENCH_OUTPUT=qualification/performance/<stream>.json \
#   EGGCHAOS_STREAM_PROBE_OUTPUT=qualification/performance/<probes>.json \
#   ./scripts/benchmark.sh
# With only EGGCHAOS_STREAM_BENCH_OUTPUT set, stdout goes to the file and
# probes stay on stderr.
#
# Authoritative guard:
#   EGGCHAOS_BENCH_REQUIRE_CLEAN=1 ./scripts/benchmark.sh
#   ./scripts/benchmark.sh --require-clean
# Dirty source state fails before benchmark execution in this mode.
# Without it, dirty runs proceed but are marked authoritative:false with a
# source fingerprint plus a stderr warning (exploratory, not exact-candidate).
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)

require_clean="${EGGCHAOS_REQUIRE_CLEAN:-${EGGCHAOS_BENCH_REQUIRE_CLEAN:-}}"
stream_output="${EGGCHAOS_STREAM_BENCH_OUTPUT:-}"
probe_output="${EGGCHAOS_STREAM_PROBE_OUTPUT:-}"
for arg in "$@"; do
  case "$arg" in
    --require-clean) require_clean=1 ;;
    *) echo "benchmark.sh: unknown argument '$arg'" >&2; exit 2 ;;
  esac
done

if [ -n "$stream_output" ] && [ -n "$probe_output" ]; then
  provenance_envelope=$(python3 "$script_dir/bench_provenance.py" --json --exclude "$stream_output" --exclude "$probe_output")
elif [ -n "$stream_output" ]; then
  provenance_envelope=$(python3 "$script_dir/bench_provenance.py" --json --exclude "$stream_output")
elif [ -n "$probe_output" ]; then
  provenance_envelope=$(python3 "$script_dir/bench_provenance.py" --json --exclude "$probe_output")
else
  provenance_envelope=$(python3 "$script_dir/bench_provenance.py" --json)
fi
provenance_json=$(printf '%s' "$provenance_envelope" | python3 -c "import json,sys; print(json.dumps(json.load(sys.stdin)['provenance'], sort_keys=True))")
worktree=$(printf '%s' "$provenance_json" | python3 -c "import json,sys; print(json.loads(sys.stdin.read())['worktree'])")
head_sha=$(printf '%s' "$provenance_json" | python3 -c "import json,sys; v=json.loads(sys.stdin.read())['head_sha']; print(v if v else 'unknown')")

if [ "$worktree" != "clean" ]; then
  fingerprint=$(printf '%s' "$provenance_json" | python3 -c "import json,sys; v=json.loads(sys.stdin.read())['source_fingerprint']; print(v if v else 'none')")
  if [ -n "$require_clean" ] && [ "$require_clean" != "0" ]; then
    echo "benchmark.sh: refusing canonical evidence run on dirty source tree (head=$head_sha fingerprint=$fingerprint)" >&2
    exit 2
  fi
  echo "benchmark.sh: WARNING dirty source worktree (head=$head_sha fingerprint=$fingerprint): report will carry authoritative:false and must not be called exact-candidate evidence" >&2
fi

export EGGCHAOS_BENCH_PROVENANCE_JSON="$provenance_json"

cargo fmt --manifest-path "$repo_root/benchmarks/Cargo.toml" -- --check
if [ -n "$stream_output" ] && [ -n "$probe_output" ]; then
  cargo run --manifest-path "$repo_root/benchmarks/Cargo.toml" --release --quiet --bin eggchaos-benchmarks >"$stream_output" 2>"$probe_output"
elif [ -n "$stream_output" ]; then
  cargo run --manifest-path "$repo_root/benchmarks/Cargo.toml" --release --quiet --bin eggchaos-benchmarks >"$stream_output"
elif [ -n "$probe_output" ]; then
  cargo run --manifest-path "$repo_root/benchmarks/Cargo.toml" --release --quiet --bin eggchaos-benchmarks 2>"$probe_output"
else
  cargo run --manifest-path "$repo_root/benchmarks/Cargo.toml" --release --quiet --bin eggchaos-benchmarks
fi
