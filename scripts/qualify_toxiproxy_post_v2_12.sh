#!/usr/bin/env sh
# M038 qualification gate: exact-candidate post-v2.12 pinned oracle +
# opt-in `post-v2.12-2026-09-25` profile differential vs the source-build
# `40f7fd31` snapshot.
#
# Strict v2.12 continues to pass via the existing `qualify_toxiproxy_v2_12.sh`
# gate. The post-v2.12 gate is opt-in: when
# `EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1` is set and no oracle is available,
# the script exits 1. Without that env var the script reports
# `differential:incomplete` (exit 0) so developer mode does not falsely
# flag a pass.
set -eu

# Reap any stale post-v2.12 oracle from a prior qualification run so the
# new oracle can bind the same admin port. Strict v2.12 / Toxiproxy
# qualifiers use different ports and are unaffected.
pgrep -f 'toxiproxy-post-v2-12.*toxiproxy-server|toxiproxy-server.*-port 18748' 2>/dev/null \
  | xargs -r kill -9 2>/dev/null || true
sleep 1

cargo test -p eggchaos-toxiproxy --all-features >/dev/null

oracle="$(./scripts/fetch_toxiproxy_post_v2_12.sh 2>/dev/null || true)"
if [ -z "$oracle" ] || [ ! -x "$oracle" ]; then
  if [ "${EGGCHAOS_REQUIRE_POST_V2_12_ORACLE:-0}" = "1" ]; then
    echo '{"differential":"incomplete","oracle":"missing"}' >&2
    exit 1
  fi
  echo '{"differential":"incomplete","oracle":"missing"}'
  exit 0
fi
version="$( "$oracle" -version 2>&1 || true )"
case "$version" in
  *toxiproxy-server*) ;;
  *)
    if [ "${EGGCHAOS_REQUIRE_POST_V2_12_ORACLE:-0}" = "1" ]; then
      echo "post-v2.12 oracle version probe failed: $version" >&2
      exit 1
    fi
    echo '{"differential":"incomplete","oracle":"version-mismatch"}'
    exit 0
    ;;
esac
commit="40f7fd31bee529d824116bd2a11a9e3425e904ec"
echo "{\"oracle\":\"$commit\",\"binary\":\"$oracle\"}"
# Differential corpus for the post-v2.12 profile: packet_loss create /
# read / update / remove, edge cases, statistical intent checks. The
# strict v2.12 regression stays on its existing pinned oracle (M012 +
# M013).
TOXIPROXY_POST_V2_12_SERVER="$oracle" \
TOXIPROXY_POST_V2_12_COMMIT="$commit" \
EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1 \
  cargo test -p eggchaos-toxiproxy --all-features --test post_v212_differential -- --nocapture \
  > /tmp/post-v212.log 2>&1 || { tail -40 /tmp/post-v212.log >&2; exit 1; }
grep -q 'DIFFERENTIAL_SUMMARY.*"failed":0' /tmp/post-v212.log \
  || { echo "post-v2.12 differential failed" >&2; tail -20 /tmp/post-v212.log >&2; exit 1; }
echo '{"translation":"pass","differential":"pass"}'
