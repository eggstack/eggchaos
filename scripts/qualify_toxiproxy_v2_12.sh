#!/usr/bin/env sh
# M012 qualification: translation suite plus the pinned-oracle differential
# corpus. Fails on any unexpected divergence. Without the pinned v2.12.0
# oracle binary it reports differential as incomplete (exit 0) rather than
# treating source inspection as differential proof.
#
# Usage: TOXIPROXY_SERVER=/path/to/pinned/v2.12.0 ./scripts/qualify_toxiproxy_v2_12.sh
set -eu
cargo test -p eggchaos-toxiproxy --all-features
oracle_bin="${TOXIPROXY_SERVER:-}"
if [ -z "$oracle_bin" ] && command -v toxiproxy-server >/dev/null 2>&1; then oracle_bin="$(command -v toxiproxy-server)"; fi
if [ -z "$oracle_bin" ]; then
  printf '%s\n' '{"translation":"pass","oracle":"unavailable","differential":"incomplete"}'
  exit 0
fi
version="$($oracle_bin --version 2>&1 || true)"
case "$version" in
  *2.12.0*) ;;
  *)
    printf '%s\n' '{"translation":"pass","oracle":"present but not pinned to 2.12.0","differential":"incomplete"}'
    exit 0
    ;;
esac
export TOXIPROXY_SERVER="$oracle_bin"
if cargo test -p eggchaos-toxiproxy --all-features --test differential -- --nocapture 2>&1 | tee /tmp/qualify_m012.log | grep -q '"failed":0'; then
  printf '%s\n' '{"translation":"pass","oracle":"toxiproxy-server 2.12.0","differential":"pass"}'
else
  printf '%s\n' '{"translation":"pass","oracle":"toxiproxy-server 2.12.0","differential":"FAIL"}'
  exit 1
fi
