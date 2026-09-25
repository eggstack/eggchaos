#!/usr/bin/env sh
# Pinned post-v2.12 Toxiproxy oracle: fetch the exact upstream commit
# `40f7fd31bee529d824116bd2a11a9e3425e904ec` from
# `Shopify/toxiproxy`, verify its source archive checksum, build the
# server with a recorded Go toolchain, and print the executable path.
#
# Mirrors the discipline of `fetch_toxiproxy_v2_12.sh` (per-release binary
# vs. here per-commit source). The post-v2.12 profile is opt-in and never
# claims equivalence with a moving upstream `main`.
set -eu

commit="40f7fd31bee529d824116bd2a11a9e3425e904ec"
dest="${1:-${TMPDIR:-/tmp}/eggchaos-toxiproxy-post-v2-12-40f7fd31/toxiproxy-server}"
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
archive_dir=$(dirname "$dest")
archive="$archive_dir/toxiproxy-post-v2-12-${commit}.tar.gz"
# The codeload archive unpacks as a single directory named after the
# commit; use that explicit directory rather than relying on a glob.
sources="$archive_dir/toxiproxy-${commit}"

# Committed source archive checksum. Pinned via M038; any change is a
# reproduction break, not a fixture refresh.
expected="26351cc70792f1c3391bdd1d376974a79a756b349ec1abfa8b8f1524c042b13d"

mkdir -p "$archive_dir"
if [ ! -f "$archive" ]; then
  curl --fail --location --silent --show-error \
    "https://codeload.github.com/Shopify/toxiproxy/tar.gz/$commit" \
    --output "$archive"
fi
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$archive" | cut -d ' ' -f 1)"
else
  actual="$(shasum -a 256 "$archive" | cut -d ' ' -f 1)"
fi
if [ "$actual" != "$expected" ]; then
  echo "toxiproxy post-v2.12 source archive checksum mismatch: got $actual expected $expected" >&2
  exit 1
fi
if [ ! -d "$sources" ]; then
  tar -xzf "$archive" -C "$archive_dir"
fi
build_dir="$sources"
if [ ! -d "$build_dir/cmd/server" ]; then
  echo "post-v2.12 source tree missing cmd/server: $build_dir" >&2
  exit 1
fi
( cd "$build_dir/cmd/server" && go build -o "$dest" . )
chmod +x "$dest"
# Source-built upstream reports `version git`; assert the oracle identity
# rather than a real release tag.
version_output="$("$dest" -version 2>&1 || true)"
if ! printf '%s' "$version_output" | grep -q 'toxiproxy-server version'; then
  echo "post-v2.12 oracle -version output unexpected: $version_output" >&2
  exit 1
fi
printf '%s\n' "$dest"
