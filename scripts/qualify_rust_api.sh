#!/usr/bin/env sh
# M059 WP6: Rust public-API regression gate.
#
# Two complementary baselines (cargo-semver-checks 0.50.0, run on a
# current stable toolchain — never the 1.89 MSRV compiler):
#
# 1. Activation snapshot 0f8a8eb: the final candidate must introduce
#    ZERO public-API drift relative to activation. Any failure here is
#    an M059 regression and fails the gate.
# 2. Qualified M057 candidate 818e567: diagnostic census. The only
#    permitted delta is exactly the four explicitly decided
#    pre-release extensions below; any additional finding fails.
#
# M059 compatibility decision (recorded in the M059 closure evidence):
# the three M057-to-activation extensions are frozen v0.2.0 surface
# requirements that cannot be restored without contradicting
# correctness invariants, and 0.2.0 is unpublished at this API
# (crates.io carries only 0.1.0). The qualified baseline advances to
# the immutable v0.2.0 tag at publication, as the M059 plan
# anticipates. This gate pins that census mechanically: a fourth
# finding, or any M059-own delta, fails closed.
#
# Decided M057 census (lint :: crate :: item):
# - enum_variant_added :: eggchaos-core :: ValidationError:DurationTooLong
# - enum_variant_added :: eggchaos-core :: ValidationError:TooManyFaults
# - enum_variant_added :: eggchaos-core :: ValidationError:GenerationOverflow
#   (bounded-limit failure modes: 24h duration cap, 256-stage ceiling,
#   generation overflow. Removing them would delete documented
#   failure modes owned by the bounded-everything invariant.)
# - constructible_struct_adds_field :: eggchaos-experiment :: CompiledScenarioV2.total_duration_ns
#   (M058-frozen Scenario V2 fingerprint input; removing it
#   contradicts the frozen release surface.)
# - constructible_struct_adds_field :: eggchaos-toxiproxy :: ProxyInput.toxics
# - constructible_struct_adds_field :: eggchaos-toxiproxy :: Proxy.logger
#   (v2.12 oracle-compatibility presentation: tolerated request field
#   and emitted response field for strict differential parity;
#   removing either risks the qualified 50/50 corpus.)
set -eu

fail() { echo "FAIL: $1" >&2; exit 1; }

M057="818e5674f2efaf96ec8effda81cef1dfa7a48614"
ACTIVATION="0f8a8ebc8b8f734951624be32362ee512b9e3d5e"
TOOL_VERSION="0.50.0"
CRATES="eggchaos-core eggchaos-experiment eggchaos-protocol eggchaos-server eggchaos-toxiproxy eggchaos-eggfetch eggchaos-embed"

command -v cargo >/dev/null 2>&1 || fail "cargo not on PATH (run on a current stable toolchain)"
VERSION="$(cargo semver-checks --version 2>/dev/null || true)"
case "$VERSION" in
  *" $TOOL_VERSION"*) ;;
  *) fail "need cargo-semver-checks $TOOL_VERSION, got: $VERSION" ;;
esac
[ "$(rustc -vV | sed -n 's/^release: //p' | cut -d. -f1-2)" != "1.89" ] \
  || fail "API gate must run on a current stable toolchain, not the 1.89 MSRV compiler"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT INT TERM

# 1. Activation comparison: zero drift permitted.
for crate in $CRATES; do
  echo "--- $crate vs activation $ACTIVATION"
  if cargo semver-checks check-release -p "$crate" --all-features \
      --baseline-rev "$ACTIVATION" > "$WORK/$crate-activation.log" 2>&1; then
    echo "ok: no API drift vs activation"
  else
    grep -E "^--- failure" "$WORK/$crate-activation.log" || true
    fail "$crate drifted vs activation (see above)"
  fi
done

# 2. M057 census: failures must equal exactly the decided set.
for crate in $CRATES; do
  echo "--- $crate vs M057 $M057"
  cargo semver-checks check-release -p "$crate" --all-features \
      --baseline-rev "$M057" > "$WORK/$crate-m057.log" 2>&1 || true
done
python3 - "$WORK" <<'PY' || exit 1
import re
import sys
from pathlib import Path

work = Path(sys.argv[1])
expected = {
    ("eggchaos-core", "enum_variant_added", "variant ValidationError:DurationTooLong"),
    ("eggchaos-core", "enum_variant_added", "variant ValidationError:TooManyFaults"),
    ("eggchaos-core", "enum_variant_added", "variant ValidationError:GenerationOverflow"),
    ("eggchaos-experiment", "constructible_struct_adds_field", "field CompiledScenarioV2.total_duration_ns"),
    ("eggchaos-toxiproxy", "constructible_struct_adds_field", "field ProxyInput.toxics"),
    ("eggchaos-toxiproxy", "constructible_struct_adds_field", "field Proxy.logger"),
}

found = set()
current = None
for crate in ["eggchaos-core", "eggchaos-experiment", "eggchaos-protocol",
              "eggchaos-server", "eggchaos-toxiproxy", "eggchaos-eggfetch",
              "eggchaos-embed"]:
    text = (work / f"{crate}-m057.log").read_text(encoding="utf-8", errors="replace")
    for line in text.splitlines():
        m = re.match(r"--- failure (\S+):", line)
        if m:
            current = (crate, m.group(1))
            continue
        m = re.match(r"\s+(\S(?:.*\S)?) in \S+:\d+\s*$", line)
        if m and current and current[0] == crate:
            item = m.group(1)
            if item.startswith(("variant ", "field ", "function ", "struct ",
                                "enum ", "trait ", "method ")):
                found.add((crate, current[1], item))

extra = found - expected
missing = expected - found
if extra:
    print(f"UNDECIDED M057 findings (gate fails): {sorted(extra)}")
    sys.exit(1)
if missing:
    print(f"decided census item no longer reported (investigate): {sorted(missing)}")
    sys.exit(1)
print(f"M057 census matches the decided set ({len(found)} findings, no others)")
PY

echo "{\"rust_api_gate\":\"pass\",\"tool\":\"cargo-semver-checks $TOOL_VERSION\"}"
