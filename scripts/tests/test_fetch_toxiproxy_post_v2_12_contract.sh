#!/usr/bin/env sh
# M041 regression: freeze the post-v2.12 fetcher stdout contract.
#
# Proves:
# - default mode prints exactly one usable executable path;
# - `--path-only` prints exactly one usable executable path;
# - `--json` parses as JSON and contains the M041 oracle identity fields;
# - default/path output contains no JSON prefix/suffix;
# - JSON mode contains no extra stdout diagnostics;
# - `--help` prints usage to stderr and exits 0;
# - unknown flags are rejected with a diagnostic and exit code 2.
#
# The script's heavy build path requires an already-built oracle plus an
# exact Go toolchain. This test only runs the full contract checks
# when a real cached oracle already exists under the developer/system
# TMPDIR; otherwise it runs the argument-parsing and pattern checks
# and reports `partial:contract-only` so CI without an oracle still
# observes the arg-parsing regressions.
set -eu

fail() { echo "FAIL: $1" >&2; exit 1; }

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
fetcher="$script_dir/../fetch_toxiproxy_post_v2_12.sh"
[ -x "$fetcher" ] || fail "$fetcher is not executable"
qualifier="$script_dir/../qualify_toxiproxy_post_v2_12.sh"
[ -x "$qualifier" ] || fail "$qualifier is not executable"

# 1. Static pattern checks on the shipped scripts.
if ! grep -q 'fetch_toxiproxy_post_v2_12.sh --path-only' "$qualifier"; then
  fail "qualify_toxiproxy_post_v2_12.sh must use --path-only explicitly"
fi
# The script must still emit JSON metadata when --json is requested.
if ! grep -q '"requested_toolchain"' "$fetcher"; then
  fail "fetcher must emit requested_toolchain in --json mode"
fi
if ! grep -q '"oracle_path"' "$fetcher"; then
  fail "fetcher must emit oracle_path in --json mode"
fi
# Contract documentation must remain in the script header.
if ! grep -q "default (no flag)" "$fetcher"; then
  fail "fetcher stdout contract comment is missing"
fi
# M041 forbids a default of --json; the default must remain --path-only.
# The old contract parsed `mode="${1:-}"` and fell through to `--json`.
if grep -q 'mode="\${1:-}"' "$fetcher"; then
  fail "fetcher default mode parsing regressed (still uses old mode=\"\${1:-}\" pattern)"
fi
if ! grep -q 'mode="--path-only"' "$fetcher"; then
  fail "fetcher must default to --path-only"
fi
echo "pattern checks ok"

# 2. --help prints usage to stderr and exits 0 (no oracle needed).
set +e
help_err="$("$fetcher" --help 2>&1 1>/dev/null)"
help_rc=$?
set -e
[ "$help_rc" = "0" ] || fail "--help exited $help_rc, want 0"
case "$help_err" in
  *usage*) ;;
  *) fail "--help stderr missing usage text: $help_err" ;;
esac
echo "--help ok"

# 3. Unknown flags are rejected with a diagnostic and exit code 2.
set +e
bogus_err="$("$fetcher" --bogus 2>&1 1>/dev/null)"
bogus_rc=$?
set -e
[ "$bogus_rc" = "2" ] || fail "--bogus exited $bogus_rc, want 2"
case "$bogus_err" in
  *unknown*|*flag*)
    ;;
  *)
    fail "--bogus stderr missing diagnostic: $bogus_err"
    ;;
esac
echo "--bogus rejected ok"

# 4. End-to-end stdout contract checks require a real cached oracle.
real_root="${TMPDIR:-/tmp}/eggchaos-toxiproxy-post-v2-12-40f7fd31"
real_oracle="$real_root/toxiproxy-server"
if [ ! -x "$real_oracle" ]; then
  echo "end-to-end stdout contract skipped (no prebuilt oracle at $real_oracle)"
  echo '{"fetcher_contract":"partial:pattern-and-args-only"}'
  exit 0
fi

# Count substantive stdout lines: a heredoc preserves any embedded
# newline, and `wc -l` reports newline-terminated lines. We use the
# variable directly so the trailing newline (if any) is preserved.
count_lines() {
  printf '%s\n' "$1" | wc -l | tr -d ' '
}

# 4a. Default mode prints exactly one executable path.
default_out="$("$fetcher" 2>/dev/null || true)"
[ -n "$default_out" ] || fail "default mode produced no stdout"
default_path="${default_out%$'\n'}"
[ -x "$default_path" ] || fail "default mode path is not executable: $default_path"
case "$default_out" in
  *'{'*'}'*)
    fail "default mode stdout contains JSON content: $default_out"
    ;;
esac
default_linecount="$(count_lines "$default_out")"
[ "$default_linecount" = "1" ] || fail "default mode produced $default_linecount lines, want 1"
echo "default mode ok (path=$default_path)"

# 4b. --path-only prints exactly one executable path.
path_out="$("$fetcher" --path-only 2>/dev/null || true)"
[ -n "$path_out" ] || fail "--path-only mode produced no stdout"
path_path="${path_out%$'\n'}"
[ -x "$path_path" ] || fail "--path-only mode path is not executable: $path_path"
case "$path_out" in
  *'{'*'}'*)
    fail "--path-only mode stdout contains JSON content: $path_out"
    ;;
esac
path_linecount="$(count_lines "$path_out")"
[ "$path_linecount" = "1" ] || fail "--path-only mode produced $path_linecount lines, want 1"
echo "--path-only mode ok (path=$path_path)"

# 4c. --json parses as JSON with the M041 oracle identity fields.
json_out="$("$fetcher" --json 2>/dev/null || true)"
[ -n "$json_out" ] || fail "--json mode produced no stdout"
json_linecount="$(count_lines "$json_out")"
[ "$json_linecount" = "1" ] || fail "--json mode produced $json_linecount lines, want 1"
echo "$json_out" | python3 -c '
import json, sys
obj = json.loads(sys.stdin.read())
required = ["requested_toolchain", "resolved_go_version", "resolved_gotoolchain",
            "source_commit", "source_sha256", "oracle_path", "oracle_version"]
missing = [k for k in required if k not in obj]
assert not missing, "missing fields: " + ",".join(missing)
assert obj["oracle_path"], "oracle_path must be non-empty"
assert obj["source_commit"] == "40f7fd31bee529d824116bd2a11a9e3425e904ec", \
    "unexpected source_commit: " + obj["source_commit"]
assert obj["oracle_version"].startswith("toxiproxy-server version"), \
    "unexpected oracle_version: " + obj["oracle_version"]
'
echo "--json mode ok (parsed, all required fields present)"

# 4d. JSON mode must not emit any stderr diagnostics.
json_stderr="$("$fetcher" --json 2>&1 1>/dev/null || true)"
[ -z "$json_stderr" ] || fail "--json leaked diagnostics to stderr: $json_stderr"
echo "--json stderr clean ok"

echo '{"fetcher_contract":"pass"}'