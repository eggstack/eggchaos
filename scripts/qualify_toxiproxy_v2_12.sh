#!/usr/bin/env sh
# Translation and oracle differential qualification for Toxiproxy v2.12.0.
# Developer mode may report explicit incomplete evidence; release mode is
# fail-closed and requires a verified, explicitly supplied pinned binary.
set -eu

cargo test -p eggchaos-toxiproxy --all-features
strict="${EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE:-0}"
case "$strict" in
  0|1) ;;
  *) echo 'EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE must be 0 or 1' >&2; exit 2 ;;
esac
oracle_bin="${TOXIPROXY_SERVER:-}"
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ "$strict" = 1 ] && [ -z "$oracle_bin" ]; then
  echo 'strict Toxiproxy qualification requires TOXIPROXY_SERVER' >&2
  exit 1
fi
if [ -z "$oracle_bin" ] && [ "$strict" != 1 ] && command -v toxiproxy-server >/dev/null 2>&1; then
  oracle_bin="$(command -v toxiproxy-server)"
fi
if [ -z "$oracle_bin" ]; then
  printf '%s\n' '{"translation":"pass","oracle":"unavailable","differential":"incomplete"}'
  exit 0
fi
if [ ! -x "$oracle_bin" ]; then
  if [ "$strict" = 1 ]; then echo "oracle is not executable: $oracle_bin" >&2; exit 1; fi
  printf '%s\n' '{"translation":"pass","oracle":"invalid path","differential":"incomplete"}'
  exit 0
fi

expected=""
if expected="$("$script_dir/toxiproxy_v2_12_expected_sha256.sh" 2>/dev/null)"; then
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$oracle_bin" | cut -d ' ' -f 1)"
  else
    actual="$(shasum -a 256 "$oracle_bin" | cut -d ' ' -f 1)"
  fi
else
  actual=unsupported-host
fi
version="$("$oracle_bin" -version 2>&1 || true)"
if [ "$actual" != "$expected" ] || ! printf '%s' "$version" | grep -q 'version 2.12.0'; then
  if [ "$strict" = 1 ]; then
    echo "oracle identity verification failed (sha256=$actual, expected=$expected, version=$version)" >&2
    exit 1
  fi
  printf '%s\n' '{"translation":"pass","oracle":"not verified as pinned v2.12.0","differential":"incomplete"}'
  exit 0
fi

log="$(mktemp)"
trap 'rm -f "$log"' EXIT HUP INT TERM
if TOXIPROXY_SERVER="$oracle_bin" cargo test -p eggchaos-toxiproxy --all-features --test differential -- --nocapture >"$log" 2>&1; then
  cat "$log"
  if grep -q 'DIFFERENTIAL_SUMMARY .*"failed":0' "$log"; then
    printf '%s\n' '{"translation":"pass","oracle":"toxiproxy-server 2.12.0 (checksum verified)","differential":"pass"}'
  elif [ "$strict" = 1 ]; then
    echo 'strict differential run produced no clean summary' >&2
    exit 1
  else
    printf '%s\n' '{"translation":"pass","oracle":"toxiproxy-server 2.12.0","differential":"incomplete"}'
  fi
else
  cat "$log"
  printf '%s\n' '{"translation":"pass","oracle":"toxiproxy-server 2.12.0","differential":"FAIL"}'
  exit 1
fi
