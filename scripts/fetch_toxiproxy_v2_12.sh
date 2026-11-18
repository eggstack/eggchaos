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
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/eggchaos-toxiproxy-v2.12.0.XXXXXX")"
install_dir=""
cleanup() {
  rm -rf "$work_dir"
  if [ -n "$install_dir" ]; then rm -rf "$install_dir"; fi
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
chmod 700 "$work_dir"

staged_binary="$work_dir/$asset"
curl --proto '=https' --proto-redir '=https' --fail --location --silent --show-error \
  "https://github.com/Shopify/toxiproxy/releases/download/v2.12.0/$asset" \
  --output "$staged_binary"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$staged_binary" | cut -d ' ' -f 1)"
else
  actual="$(shasum -a 256 "$staged_binary" | cut -d ' ' -f 1)"
fi
if [ "$actual" != "$expected" ]; then
  echo "SHA-256 mismatch for $asset: expected $expected, got $actual" >&2
  exit 1
fi
chmod +x "$staged_binary"
version="$("$staged_binary" -version 2>&1 || true)"
case "$version" in
  *'version 2.12.0'*) ;;
  *) echo "unexpected oracle version: $version" >&2; exit 1 ;;
esac
if [ -d "$dest" ]; then
  echo "oracle destination is a directory: $dest" >&2
  exit 1
fi
dest_parent="$(dirname "$dest")"
mkdir -p "$dest_parent"
install_dir="$(mktemp -d "$dest_parent/.eggchaos-toxiproxy-v2.12.0.XXXXXX")"
chmod 700 "$install_dir"
install_binary="$install_dir/$asset"
mv -f "$staged_binary" "$install_binary"
mv -f "$install_binary" "$dest"
printf '%s\n' "$dest"
