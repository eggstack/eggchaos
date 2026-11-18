#!/usr/bin/env sh
# M065 behavior checks for private fetch staging, stdout contracts,
# checksum-before-install, HTTPS-only redirects, and cleanup.
set -eu

fail() { echo "FAIL: $1" >&2; exit 1; }
count_lines() { wc -l < "$1" | tr -d ' '; }

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
v212_fetcher="$repo_root/scripts/fetch_toxiproxy_v2_12.sh"
post_fetcher="$repo_root/scripts/fetch_toxiproxy_post_v2_12.sh"
base_tmp="${TMPDIR:-/tmp}"
[ -d "$base_tmp" ] || mkdir -p "$base_tmp"
test_root=$(mktemp -d "$base_tmp/eggchaos-fetch-staging-test.XXXXXX")
cleanup() { rm -rf "$test_root"; }
trap cleanup EXIT HUP INT TERM
staging="$test_root/tmp"
mock_bin="$test_root/mock-bin"
mkdir -p "$staging" "$mock_bin"

export FETCH_CURL_ARGS_LOG="$test_root/curl-args.log"
export FETCH_CURL_OUTPUT_LOG="$test_root/curl-output.log"
export FETCH_CURL_MODE_LOG="$test_root/curl-mode.log"
export FETCH_V212_EXPECTED="$("$repo_root/scripts/toxiproxy_v2_12_expected_sha256.sh")"
export FETCH_POST_EXPECTED=26351cc70792f1c3391bdd1d376974a79a756b349ec1abfa8b8f1524c042b13d
: > "$FETCH_CURL_ARGS_LOG"
: > "$FETCH_CURL_OUTPUT_LOG"
: > "$FETCH_CURL_MODE_LOG"

cat > "$mock_bin/curl" <<'MOCK_CURL'
#!/bin/sh
set -eu
request="$*"
printf '%s\n' "$request" >> "$FETCH_CURL_ARGS_LOG"
output=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output) shift; output="$1" ;;
  esac
  shift
done
[ -n "$output" ] || exit 2
printf '%s\n' "$output" >> "$FETCH_CURL_OUTPUT_LOG"
directory=${output%/*}
case "$(uname -s)" in
  Darwin) mode=$(stat -f '%Lp' "$directory") ;;
  *) mode=$(stat -c '%a' "$directory") ;;
esac
printf '%s\n' "$mode" >> "$FETCH_CURL_MODE_LOG"
if [ "${FETCH_CURL_BLOCK:-0}" = 1 ]; then
  : > "$FETCH_CURL_BLOCK_MARKER"
  sleep 2
  exit 0
fi
case "$request" in
  *codeload.github.com*) printf 'mock source archive\n' > "$output" ;;
  *)
    cat > "$output" <<'MOCK_V212_ORACLE'
#!/bin/sh
printf '%s\n' 'Toxiproxy version 2.12.0'
MOCK_V212_ORACLE
    ;;
esac
MOCK_CURL

cat > "$mock_bin/sha256sum" <<'MOCK_SHA256'
#!/bin/sh
set -eu
if [ "${FETCH_HASH_MODE:-valid}" = mismatch ]; then
  hash=0000000000000000000000000000000000000000000000000000000000000000
else
  case "$1" in
    *.tar.gz) hash="$FETCH_POST_EXPECTED" ;;
    *) hash="$FETCH_V212_EXPECTED" ;;
  esac
fi
printf '%s  %s\n' "$hash" "$1"
MOCK_SHA256

cat > "$mock_bin/tar" <<'MOCK_TAR'
#!/bin/sh
set -eu
extract_to=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -C) shift; extract_to="$1" ;;
  esac
  shift
done
[ -n "$extract_to" ] || exit 2
mkdir -p "$extract_to/toxiproxy-40f7fd31bee529d824116bd2a11a9e3425e904ec/cmd/server"
MOCK_TAR

cat > "$mock_bin/go" <<'MOCK_GO'
#!/bin/sh
set -eu
case "$1" in
  version) printf 'go version %s mock/host\n' "$GOTOOLCHAIN" ;;
  env) printf '%s\n' "$GOTOOLCHAIN" ;;
  build)
    shift
    output=""
    while [ "$#" -gt 0 ]; do
      case "$1" in
        -o) shift; output="$1" ;;
      esac
      shift
    done
    [ -n "$output" ] || exit 2
    cat > "$output" <<'MOCK_POST_ORACLE'
#!/bin/sh
printf '%s\n' 'toxiproxy-server version git'
MOCK_POST_ORACLE
    chmod +x "$output"
    ;;
  *) echo "unexpected mock go arguments: $*" >&2; exit 2 ;;
esac
MOCK_GO
chmod +x "$mock_bin"/*

# v2.12 path mode succeeds only after the host checksum and version checks.
v212_dest="$test_root/install-v212/oracle"
if ! TMPDIR="$staging" PATH="$mock_bin:$PATH" FETCH_HASH_MODE=valid \
  "$v212_fetcher" "$v212_dest" > "$test_root/v212.out" 2> "$test_root/v212.err"; then
  fail "v2.12 fetch failed: $(cat "$test_root/v212.err")"
fi
[ "$(count_lines "$test_root/v212.out")" = 1 ] || fail "v2.12 fetch must print one path"
[ "$(cat "$test_root/v212.out")" = "$v212_dest" ] || fail "v2.12 stdout path changed"
[ -x "$v212_dest" ] || fail "v2.12 fetch did not install an executable"
[ ! -s "$test_root/v212.err" ] || fail "v2.12 success wrote diagnostics"

# Post-v2.12 default, --path-only, and --json stdout modes use the mocked
# pinned archive and the exact Go toolchain identity without network access.
post_default="$staging/eggchaos-toxiproxy-post-v2-12-40f7fd31/toxiproxy-server"
if ! TMPDIR="$staging" PATH="$mock_bin:$PATH" FETCH_HASH_MODE=valid \
  "$post_fetcher" > "$test_root/post-default.out" 2> "$test_root/post-default.err"; then
  fail "post-v2.12 default fetch failed: $(cat "$test_root/post-default.err")"
fi
[ "$(count_lines "$test_root/post-default.out")" = 1 ] || fail "post default must print one path"
[ "$(cat "$test_root/post-default.out")" = "$post_default" ] || fail "post default stdout path changed"
[ -x "$post_default" ] || fail "post default did not install an executable"
[ ! -s "$test_root/post-default.err" ] || fail "post default wrote diagnostics"

post_path="$test_root/path-only/oracle"
if ! TMPDIR="$staging" PATH="$mock_bin:$PATH" FETCH_HASH_MODE=valid \
  "$post_fetcher" --path-only "$post_path" > "$test_root/post-path.out" 2> "$test_root/post-path.err"; then
  fail "post-v2.12 --path-only failed: $(cat "$test_root/post-path.err")"
fi
[ "$(count_lines "$test_root/post-path.out")" = 1 ] || fail "--path-only must print one path"
[ "$(cat "$test_root/post-path.out")" = "$post_path" ] || fail "--path-only stdout path changed"
[ -x "$post_path" ] || fail "--path-only did not install an executable"
[ ! -s "$test_root/post-path.err" ] || fail "--path-only wrote diagnostics"

post_json="$test_root/json/oracle"
if ! TMPDIR="$staging" PATH="$mock_bin:$PATH" FETCH_HASH_MODE=valid \
  "$post_fetcher" --json "$post_json" > "$test_root/post-json.out" 2> "$test_root/post-json.err"; then
  fail "post-v2.12 --json failed: $(cat "$test_root/post-json.err")"
fi
[ "$(count_lines "$test_root/post-json.out")" = 1 ] || fail "--json must print one record"
[ ! -s "$test_root/post-json.err" ] || fail "--json wrote diagnostics"
python3 -c '
import json, sys
with open(sys.argv[1], encoding="utf-8") as stream:
    obj = json.loads(stream.read())
assert obj["requested_toolchain"] == "go1.23.0", obj
assert obj["resolved_go_version"].startswith("go version go1.23.0"), obj
assert obj["resolved_gotoolchain"] == "go1.23.0", obj
assert obj["source_commit"] == "40f7fd31bee529d824116bd2a11a9e3425e904ec", obj
assert obj["source_sha256"] == "26351cc70792f1c3391bdd1d376974a79a756b349ec1abfa8b8f1524c042b13d", obj
assert obj["oracle_path"] == sys.argv[2], obj
assert obj["oracle_version"].startswith("toxiproxy-server version"), obj
' "$test_root/post-json.out" "$post_json"
[ -x "$post_json" ] || fail "--json did not install an executable"
echo "fetcher stdout contracts ok (v2.12 path; post default/path-only/JSON)"

# On checksum failure, neither fetcher may emit a path or create a partial
# destination (including its parent). The test writes only inside test_root.
for profile in v212 post; do
  dest="$test_root/mismatch-$profile/oracle"
  if [ "$profile" = v212 ]; then script="$v212_fetcher"; else script="$post_fetcher"; fi
  set +e
  TMPDIR="$staging" PATH="$mock_bin:$PATH" FETCH_HASH_MODE=mismatch \
    "$script" "$dest" > "$test_root/mismatch-$profile.out" 2> "$test_root/mismatch-$profile.err"
  status=$?
  set -e
  [ "$status" -ne 0 ] || fail "$profile checksum mismatch unexpectedly succeeded"
  [ ! -s "$test_root/mismatch-$profile.out" ] || fail "$profile checksum failure leaked stdout"
  grep -qi 'mismatch' "$test_root/mismatch-$profile.err" \
    || fail "$profile checksum failure diagnostic missing"
  [ ! -e "$dest" ] || fail "$profile checksum failure left a partial executable"
  [ ! -e "$test_root/mismatch-$profile" ] \
    || fail "$profile checksum failure created its install parent"
done
echo "checksum mismatch is fail-closed with no partial install"

# A TERM during a download exits with the documented signal status and removes
# private staging without creating the caller's destination parent.
signal_dest="$test_root/signal/oracle"
export FETCH_CURL_BLOCK_MARKER="$test_root/curl-blocked"
TMPDIR="$staging" PATH="$mock_bin:$PATH" FETCH_HASH_MODE=valid FETCH_CURL_BLOCK=1 \
  "$v212_fetcher" "$signal_dest" > "$test_root/signal.out" 2> "$test_root/signal.err" &
signal_pid=$!
attempt=0
while [ ! -e "$FETCH_CURL_BLOCK_MARKER" ]; do
  kill -0 "$signal_pid" 2>/dev/null || fail "fetcher exited before the signal probe started"
  attempt=$((attempt + 1))
  [ "$attempt" -lt 100 ] || fail "fetcher did not reach the mocked download"
  sleep 0.05
done
kill -TERM "$signal_pid"
set +e
wait "$signal_pid"
signal_status=$?
set -e
[ "$signal_status" = 143 ] || fail "TERM exit status was $signal_status, expected 143"
[ ! -e "$signal_dest" ] || fail "TERM left a partial executable at the destination"
[ ! -e "${signal_dest%/*}" ] || fail "TERM created the destination parent before verification"
echo "TERM during download exits 143 and cleans private staging"

# Confirm every download used a unique 0700 staging directory restricted to
# HTTPS, and that success and failure paths removed all of those directories.
while IFS= read -r artifact; do
  case "$artifact" in
    "$staging"/eggchaos-toxiproxy-v2.12.0.*/*|"$staging"/eggchaos-toxiproxy-post-v2-12.*/*) ;;
    *) fail "artifact was not staged in a private temporary directory: $artifact" ;;
  esac
  [ ! -e "${artifact%/*}" ] || fail "staging directory survived: ${artifact%/*}"
done < "$FETCH_CURL_OUTPUT_LOG"
artifact_count=$(count_lines "$FETCH_CURL_OUTPUT_LOG")
unique_count=$(sort -u "$FETCH_CURL_OUTPUT_LOG" | wc -l | tr -d ' ')
[ "$artifact_count" = "$unique_count" ] || fail "temporary staging paths were reused"
while IFS= read -r mode; do
  [ "$mode" = 700 ] || fail "staging directory mode was $mode, expected 700"
done < "$FETCH_CURL_MODE_LOG"
while IFS= read -r args; do
  case "$args" in
    *"--proto =https"*"--proto-redir =https"*) ;;
    *) fail "curl did not restrict redirect protocol to HTTPS: $args" ;;
  esac
done < "$FETCH_CURL_ARGS_LOG"
for parent in \
  "$test_root/install-v212" \
  "$staging/eggchaos-toxiproxy-post-v2-12-40f7fd31" \
  "$test_root/path-only" \
  "$test_root/json"; do
  for install_dir in "$parent"/.eggchaos-toxiproxy-*.*; do
    [ ! -e "$install_dir" ] || fail "install staging directory survived: $install_dir"
  done
done
echo "private unique staging, HTTPS-only redirects, and cleanup ok"
