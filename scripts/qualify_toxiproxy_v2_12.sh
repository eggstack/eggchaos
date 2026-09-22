#!/usr/bin/env sh
set -eu
cargo test -p eggchaos-toxiproxy --all-features
if command -v toxiproxy-server >/dev/null 2>&1; then
  version="$(toxiproxy-server --version 2>&1 || true)"
  case "$version" in
    *2.12.0*) printf '%s\n' '{"translation":"pass","oracle":"toxiproxy-server 2.12.0"}' ;;
    *) printf '%s\n' '{"translation":"pass","oracle":"present but not pinned to 2.12.0"}' ;;
  esac
else
  printf '%s\n' '{"translation":"pass","oracle":"unavailable","differential":"incomplete"}'
fi
