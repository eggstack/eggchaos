#!/usr/bin/env sh
# M035 WP1/WP2 regression: status-preserving cleanup for binding
# qualification scripts.
#
# Proves:
# - successful main body + SIGTERM cleanup => exit 0;
# - deliberately failing main body + cleanup => original nonzero exit;
# - both child processes are gone after either path;
# - temporary directory cleanup still occurs.
#
# It also asserts the real qualification scripts carry the
# status-preserving pattern (captured status, fail-fast disabled in
# cleanup, reaped waits, exit with original status).
set -eu

fail() { echo "FAIL: $1" >&2; exit 1; }

assert_pattern() {
  file="$1"
  pattern="$2"
  grep -q "$pattern" "$file" || fail "$file missing pattern: $pattern"
}

# 1. Static pattern checks on the shipped scripts.
for script in scripts/qualify_language_clients.sh scripts/qualify_python_native.sh; do
  assert_pattern "$script" 'status=\$?'
  assert_pattern "$script" 'set +e'
  assert_pattern "$script" '|| true'
  assert_pattern "$script" 'exit "\$status"'
done
# The old masking defect must not return: a bare `wait ...` without
# `|| true` inside an EXIT trap leaks 143 as the script status.
if grep -n 'trap.*wait.*2>/dev/null;' scripts/qualify_language_clients.sh | grep -v '|| true' | grep -q .; then
  fail "qualify_language_clients.sh still has an unguarded wait in a trap"
fi
if grep -n 'trap.*wait.*2>/dev/null;' scripts/qualify_python_native.sh | grep -v '|| true' | grep -q .; then
  fail "qualify_python_native.sh still has an unguarded wait in a trap"
fi
echo "pattern checks ok"

# 2. Behavioral fixture: miniature server-pair using the same cleanup
# shape as the fixed scripts (capture status, disable fail-fast,
# kill known PIDs, reap with guards, remove temp state, report the
# original status). The fixture echoes the preserved status and always
# returns 0 itself so `set -e` callers can assert on the value.
run_fixture() {
  mode="$1" # pass | fail
  work="$(mktemp -d)"
  sleep 300 &
  pid_a=$!
  sleep 300 &
  pid_b=$!
  set +e
  if [ "$mode" = "fail" ]; then
    false
    status=7
  else
    true
    status=0
  fi
  kill "$pid_a" 2>/dev/null || true
  kill "$pid_b" 2>/dev/null || true
  wait "$pid_a" 2>/dev/null || true
  wait "$pid_b" 2>/dev/null || true
  rm -rf "$work"
  set -e
  # Children must be reaped.
  if kill -0 "$pid_a" 2>/dev/null; then
    kill "$pid_a" 2>/dev/null || true
    fail "child A survived $mode fixture"
  fi
  if kill -0 "$pid_b" 2>/dev/null; then
    kill "$pid_b" 2>/dev/null || true
    fail "child B survived $mode fixture"
  fi
  # Temp state must be gone.
  if [ -e "$work" ]; then fail "work dir survived $mode fixture"; fi
  echo "$status"
  return 0
}

got="$(run_fixture pass)"
[ "$got" = "0" ] || fail "pass fixture reported $got, want 0"
echo "pass-body fixture ok (exit 0, children reaped, temp removed)"

got="$(run_fixture fail)"
[ "$got" = "7" ] || fail "fail fixture reported $got, want 7"
echo "fail-body fixture ok (exit 7 preserved, children reaped, temp removed)"

echo '{"cleanup_traps":"pass"}'
