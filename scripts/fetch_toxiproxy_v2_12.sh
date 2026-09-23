#!/usr/bin/env sh
# Fetch the official v2.12.0 server binary and verify its release checksum.
set -eu

dest="${1:-${TMPDIR:-/tmp}/eggchaos-toxiproxy-v2.12.0/toxiproxy-server}"
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
case "$(uname -s):$(uname -m)" in
  Linux:x86_64) asset=toxiproxy-server-linux-amd64 ;;
  Linux:aarch64) asset=toxiproxy-server-linux-arm64 ;;
  Darwin:arm64|Darwin:aarch64) asset=toxiproxy-server-darwin-arm64 ;;
  Darwin:x86_64) asset=toxiproxy-server-darwin-amd64 ;;
  *) echo "unsupported oracle host: $(uname -s) $(uname -m)" >&2; exit 2 ;;
esac

expected="$("$script_dir/toxiproxy_v2_12_expected_sha256.sh")"
mkdir -p "$(dirname "$dest")"
curl --fail --location --silent --show-error \
  "https://github.com/Shopify/toxiproxy/releases/download/v2.12.0/$asset" \
  --output "$dest"
chmod +x "$dest"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$dest" | cut -d ' ' -f 1)"
else
  actual="$(shasum -a 256 "$dest" | cut -d ' ' -f 1)"
fi
if [ "$actual" != "$expected" ]; then
  echo "SHA-256 mismatch for $asset: expected $expected, got $actual" >&2
  rm -f "$dest"
  exit 1
fi
version="$("$dest" -version 2>&1 || true)"
case "$version" in
  *'version 2.12.0'*) ;;
  *) echo "unexpected oracle version: $version" >&2; rm -f "$dest"; exit 1 ;;
esac
printf '%s\n' "$dest"
