#!/usr/bin/env sh
# M047 WP7 regression: artifact-level provenance schema checks.
#
# Runs shortened benchmark workloads through the canonical wrappers and
# asserts: stream case JSON + probe JSON carry the shared provenance object
# with identical content; datagram JSON carries it with
# candidate_sha == provenance.head_sha; authoritative is true exactly for a
# clean source-relevant tree; no absolute local paths leak into retained
# JSON; legacy sample/case fields remain; the datagram budget parser accepts
# the annotated report; and a direct `cargo run` bypass emits explicitly
# non-authoritative unavailable provenance.
#
# Workloads are shortened via the existing size knobs without changing case
# semantics; no throughput values are asserted.
set -eu

fail() { echo "FAIL: $1" >&2; exit 1; }

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT HUP INT TERM

HEAD_SHA=$(git -C "$REPO_ROOT" rev-parse HEAD)
COLLECTOR_WORKTREE=$(python3 "$REPO_ROOT/scripts/bench_provenance.py" --json \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['provenance']['worktree'])")
if [ "$COLLECTOR_WORKTREE" = "clean" ]; then EXPECT_AUTH="true"; else EXPECT_AUTH="false"; fi

# --- stream: full short run through the wrapper, both documents retained ----
STREAM_OUT="$WORK/stream.json"
PROBE_OUT="$WORK/probes.json"
EGGCHAOS_BENCH_BYTES=131072 EGGCHAOS_BENCH_ROUNDS=1 \
  EGGCHAOS_STREAM_BENCH_OUTPUT="$STREAM_OUT" \
  EGGCHAOS_STREAM_PROBE_OUTPUT="$PROBE_OUT" \
  sh "$REPO_ROOT/scripts/benchmark.sh" >/dev/null 2>"$WORK/stream.stderr" \
  || fail "wrapped short stream run failed"

python3 - "$STREAM_OUT" "$PROBE_OUT" "$HEAD_SHA" "$EXPECT_AUTH" <<'PY'
import json, sys
stream_path, probe_path, head_sha, expect_auth = sys.argv[1:5]
with open(stream_path, encoding="utf-8") as f:
    stream = json.load(f)
with open(probe_path, encoding="utf-8") as f:
    probe = json.load(f)
for doc, name in ((stream, "stream cases"), (probe, "stream probes")):
    prov = doc.get("provenance")
    assert isinstance(prov, dict), f"{name} missing provenance object"
    assert prov.get("schema") == 1, f"{name} schema"
    assert prov.get("head_sha") == head_sha, f"{name} head_sha == HEAD"
    assert str(prov.get("authoritative")).lower() == expect_auth, \
        f"{name} authoritative={prov.get('authoritative')} want {expect_auth}"
    assert prov.get("worktree") in ("clean", "dirty"), f"{name} worktree"
    if prov.get("worktree") == "dirty":
        assert isinstance(prov.get("source_fingerprint"), str) and len(prov["source_fingerprint"]) == 64, \
            f"{name} dirty run needs 64-hex fingerprint"
assert stream["provenance"] == probe["provenance"], "case/probe provenance must be identical"
assert isinstance(stream.get("cases"), list) and stream["cases"], "legacy cases array"
for case in stream["cases"]:
    for key in ("name", "mean_seconds", "throughput_mib_s", "samples_seconds",
                "input_bytes", "bytes_received_target"):
        assert key in case, f"legacy case field {key} missing in {case.get('name')}"
assert probe.get("format") == 2, "legacy probe format"
assert isinstance(probe.get("probes"), list) and probe["probes"], "legacy probes array"
for p in probe["probes"]:
    assert "name" in p and "nanos_per_iter" in p, "legacy probe fields"
PY
echo "stream/probe provenance ok (authoritative=$EXPECT_AUTH)"

# --- no absolute paths or secrets in retained JSON -----------------------------
if grep -q "$REPO_ROOT" "$STREAM_OUT"; then fail "stream JSON leaks repo path"; fi
if grep -q "$REPO_ROOT" "$PROBE_OUT"; then fail "probe JSON leaks repo path"; fi
if grep -q "$HOME" "$STREAM_OUT"; then fail "stream JSON leaks HOME"; fi
if grep -qi "token\|secret" "$STREAM_OUT"; then fail "stream JSON mentions secrets"; fi
echo "no-leak checks ok"

# --- direct cargo bypass emits unavailable non-authoritative provenance ---------
BYPASS_OUT="$WORK/bypass.json"
EGGCHAOS_BENCH_BYTES=65536 EGGCHAOS_BENCH_ROUNDS=1 EGGCHAOS_BENCH_CASE=eggchaos_live_empty_plan \
  cargo run --manifest-path "$REPO_ROOT/benchmarks/Cargo.toml" --release --quiet --bin eggchaos-benchmarks \
  >"$BYPASS_OUT" 2>/dev/null
python3 - "$BYPASS_OUT" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as f:
    doc = json.load(f)
prov = doc.get("provenance")
assert isinstance(prov, dict), "bypass report missing provenance"
assert prov.get("authoritative") is False, "bypass must not be authoritative"
assert prov.get("collector") == "unavailable", "bypass must be marked unavailable"
PY
echo "bypass unavailable-provenance ok"

# --- datagram: short full run through the wrapper -------------------------------
DATAGRAM_OUT="$WORK/datagram.json"
EGGCHAOS_DATAGRAM_BENCH_DATAGRAMS=50 EGGCHAOS_DATAGRAM_BENCH_ROUNDS=1 \
  EGGCHAOS_DATAGRAM_BENCH_OUTPUT="$DATAGRAM_OUT" \
  sh "$REPO_ROOT/scripts/benchmark_datagram.sh" >/dev/null 2>"$WORK/datagram.stderr" \
  || fail "wrapped short datagram run failed (budget or harness)"
python3 - "$DATAGRAM_OUT" "$HEAD_SHA" "$EXPECT_AUTH" <<'PY'
import json, sys
path, head_sha, expect_auth = sys.argv[1:4]
with open(path, encoding="utf-8") as f:
    report = json.load(f)
prov = report.get("provenance")
assert isinstance(prov, dict), "datagram missing provenance object"
assert prov.get("schema") == 1, "datagram schema"
assert report.get("candidate_sha") == prov.get("head_sha") == head_sha, \
    "candidate_sha == provenance.head_sha == HEAD"
assert str(prov.get("authoritative")).lower() == expect_auth, "datagram authoritative"
assert isinstance(report.get("samples"), list) and report["samples"], "legacy samples"
assert "scheduler" in report, "legacy scheduler probes"
assert "cpu_model" in report and "rustc" in report, "legacy host fields"
names = {s["name"] for s in report["samples"]}
for required in ("direct_udp_echo", "fixed_target_empty_plan", "bare_fixed_target_relay",
                 "direct_windowed", "bare_windowed", "empty_plan_windowed"):
    assert required in names, f"legacy sample {required} missing"
PY
echo "datagram provenance ok (authoritative=$EXPECT_AUTH)"
if grep -q "$REPO_ROOT" "$DATAGRAM_OUT"; then fail "datagram JSON leaks repo path"; fi

# --- authoritative guard behavior matches tree state ------------------------------
if [ "$EXPECT_AUTH" = "true" ]; then
  EGGCHAOS_BENCH_REQUIRE_CLEAN=1 EGGCHAOS_BENCH_BYTES=65536 EGGCHAOS_BENCH_ROUNDS=1 \
    EGGCHAOS_BENCH_CASE=eggchaos_live_empty_plan \
    EGGCHAOS_STREAM_BENCH_OUTPUT="$WORK/clean-guard.json" \
    sh "$REPO_ROOT/scripts/benchmark.sh" >/dev/null 2>&1 \
    || fail "require-clean must pass on a clean tree"
  echo "require-clean passes on clean tree ok"
else
  if EGGCHAOS_BENCH_REQUIRE_CLEAN=1 EGGCHAOS_BENCH_BYTES=65536 EGGCHAOS_BENCH_ROUNDS=1 \
    EGGCHAOS_BENCH_CASE=eggchaos_live_empty_plan \
    EGGCHAOS_STREAM_BENCH_OUTPUT="$WORK/dirty-guard.json" \
    sh "$REPO_ROOT/scripts/benchmark.sh" >/dev/null 2>&1; then
    fail "require-clean must fail on a dirty tree"
  fi
  echo "require-clean refuses dirty tree ok"
fi

echo '{"bench_provenance_artifacts":"pass"}'
