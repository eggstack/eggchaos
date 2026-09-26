#!/usr/bin/env sh
# Pinned post-v2.12 Toxiproxy oracle: fetch the exact upstream commit
# `40f7fd31bee529d824116bd2a11a9e3425e904ec` from
# `Shopify/toxiproxy`, verify its source archive checksum, build the
# server with a recorded Go toolchain, and print the executable path.
#
# Mirrors the discipline of `fetch_toxiproxy_v2_12.sh` (per-release binary
# vs. here per-commit source). The post-v2.12 profile is opt-in and never
# claims equivalence with a moving upstream `main`.
#
# Stdout contract (M041):
# - default (no flag):           executable path on stdout (shell command
#                                substitution: `TOXIPROXY_POST_V2_12_SERVER="$(..."`).
# - `--path-only [DEST]`:        same as default, accepts an explicit DEST.
# - `--json [DEST]`:             one JSON metadata record on stdout.
# - any other flag:              diagnostic to stderr, exits 2.
# Diagnostics always go to stderr. The script never mixes JSON metadata and
# the executable path on the same stdout line.
set -eu

commit="40f7fd31bee529d824116bd2a11a9e3425e904ec"
requested_toolchain="${EGGCHAOS_POST_V2_12_GO_TOOLCHAIN:-go1.23.0}"
mode="--path-only"
dest=""
case "${1:-}" in
  "")
    mode="--path-only"
    ;;
  --path-only)
    mode="--path-only"
    shift
    if [ "$#" -gt 0 ]; then dest="$1"; shift; fi
    ;;
  --json)
    mode="--json"
    shift
    if [ "$#" -gt 0 ]; then dest="$1"; shift; fi
    ;;
  --help|-h)
    cat <<'USAGE' >&2
usage: fetch_toxiproxy_post_v2_12.sh [--path-only|--json] [DEST]
  default and --path-only print the oracle executable path on stdout
  --json prints a single JSON metadata record on stdout
  diagnostics are emitted on stderr
USAGE
    exit 0
    ;;
  -*)
    echo "fetch_toxiproxy_post_v2_12.sh: unknown flag '$1' (use --path-only or --json)" >&2
    exit 2
    ;;
  *)
    # Bare positional argument: treat as an explicit DEST in the
    # path-only contract so documented shell substitution with a
    # custom path still works.
    mode="--path-only"
    dest="$1"
    shift
    ;;
esac
if [ "$#" -gt 0 ]; then
  echo "fetch_toxiproxy_post_v2_12.sh: unexpected extra arguments: $*" >&2
  exit 2
fi
default_dest="${TMPDIR:-/tmp}/eggchaos-toxiproxy-post-v2-12-40f7fd31/toxiproxy-server"
if [ -z "$dest" ]; then dest="$default_dest"; fi
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
resolved_go="$(GOTOOLCHAIN="$requested_toolchain" go version 2>/dev/null)" || {
  echo "unable to resolve requested Go toolchain $requested_toolchain" >&2; exit 1;
}
resolved_toolchain="$(GOTOOLCHAIN="$requested_toolchain" go env GOTOOLCHAIN 2>/dev/null)" || {
  echo "unable to query requested Go toolchain $requested_toolchain" >&2; exit 1;
}
case "$resolved_go" in "go version $requested_toolchain"*) ;; *)
  echo "Go toolchain mismatch: requested=$requested_toolchain resolved=$resolved_go" >&2; exit 1;; esac
( cd "$build_dir/cmd/server" && GOTOOLCHAIN="$requested_toolchain" go build -o "$dest" . )
chmod +x "$dest"
# Source-built upstream reports `version git`; assert the oracle identity
# rather than a real release tag.
version_output="$("$dest" -version 2>&1 || true)"
if ! printf '%s' "$version_output" | grep -q 'toxiproxy-server version'; then
  echo "post-v2.12 oracle -version output unexpected: $version_output" >&2
  exit 1
fi
if [ "$mode" = "--json" ]; then
  # JSON output is the only metadata mode; the executable path must
  # never appear on stdout in this mode (M041).
  printf '{"requested_toolchain":"%s","resolved_go_version":"%s","resolved_gotoolchain":"%s","source_commit":"%s","source_sha256":"%s","oracle_path":"%s","oracle_version":"%s"}\n' \
    "$requested_toolchain" "$resolved_go" "$resolved_toolchain" "$commit" "$actual" "$dest" "$version_output"
else
  # Path-only contract: stdout is exactly one executable path line so
  # documented `TOXIPROXY_POST_V2_12_SERVER="$(...)"` substitution and
  # the explicit `--path-only` flag both keep working.
  printf '%s\n' "$dest"
fi
