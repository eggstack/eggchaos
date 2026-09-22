#!/usr/bin/env sh
set -eu
cargo test -p eggchaos-toxiproxy --all-features
oracle_bin="${TOXIPROXY_SERVER:-}"
if [ -z "$oracle_bin" ] && command -v toxiproxy-server >/dev/null 2>&1; then oracle_bin="$(command -v toxiproxy-server)"; fi
if [ -n "$oracle_bin" ]; then
  version="$($oracle_bin --version 2>&1 || true)"
  case "$version" in
    *2.12.0*) printf '%s\n' '{"translation":"pass","oracle":"toxiproxy-server 2.12.0"}' ;;
    *) printf '%s\n' '{"translation":"pass","oracle":"present but not pinned to 2.12.0"}' ;;
  esac
else
  printf '%s\n' '{"translation":"pass","oracle":"unavailable","differential":"incomplete"}'
fi
