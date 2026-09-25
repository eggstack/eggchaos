#!/usr/bin/env sh
# M033 cross-language qualification: real loopback server + equivalent
# Python (sync/async) and TypeScript flows, then package artifact builds.
set -eu
cargo build -p eggchaos-cli --locked
WORK="$(mktemp -d)"
SERVER_PID=""
AUTH_PID=""
# Status-preserving cleanup: capture the qualification exit before reaping
# intentionally SIGTERM-terminated child servers. `wait` observes 143 for
# SIGTERM children; that status must never escape as the script status.
cleanup() {
  status=$?
  set +e
  if [ -n "${SERVER_PID:-}" ]; then kill "$SERVER_PID" 2>/dev/null || true; fi
  if [ -n "${AUTH_PID:-}" ]; then kill "$AUTH_PID" 2>/dev/null || true; fi
  if [ -n "${SERVER_PID:-}" ]; then wait "$SERVER_PID" 2>/dev/null || true; fi
  if [ -n "${AUTH_PID:-}" ]; then wait "$AUTH_PID" 2>/dev/null || true; fi
  rm -rf "$WORK"
  exit "$status"
}
trap cleanup EXIT INT TERM
cat > "$WORK/eggchaos.toml" <<'EOF'
version = 1
seed = 11

[admin]
bind = "127.0.0.1:0"
EOF
cat > "$WORK/eggchaos-auth.toml" <<'EOF'
version = 1
seed = 12

[admin]
bind = "127.0.0.1:0"
auth_token = "correct-token"
EOF
./target/debug/eggchaos serve --config "$WORK/eggchaos.toml" > "$WORK/server.log" 2>&1 &
SERVER_PID=$!
./target/debug/eggchaos serve --config "$WORK/eggchaos-auth.toml" > "$WORK/auth-server.log" 2>&1 &
AUTH_PID=$!
ADMIN=""
for _ in $(seq 1 100); do
  ADMIN="$(grep -o 'admin=[^ ]*' "$WORK/server.log" | tail -1 | cut -c7- || true)"
  if [ -n "$ADMIN" ]; then break; fi
  sleep 0.1
done
AUTH=""
for _ in $(seq 1 100); do
  AUTH="$(grep -o 'admin=[^ ]*' "$WORK/auth-server.log" | tail -1 | cut -c7- || true)"
  if [ -n "$AUTH" ]; then break; fi
  sleep 0.1
done
if [ -z "$ADMIN" ] || [ -z "$AUTH" ]; then
  echo "server did not report an admin address"; cat "$WORK/server.log" "$WORK/auth-server.log"; exit 1
fi
EGGCHAOS_ADMIN_URL="http://$ADMIN" EGGCHAOS_AUTH_URL="http://$AUTH" \
  EGGCHAOS_BASE_URL="http://$ADMIN" \
  python3 -m pytest bindings/python-client/tests/ -q
(cd bindings/typescript-client && EGGCHAOS_BASE_URL="http://$ADMIN" EGGCHAOS_AUTH_URL="http://$AUTH" npm test --silent)
(cd bindings/python-client && python3 -m build --sdist --wheel --outdir "$WORK/pydist" .)
(cd bindings/typescript-client && npm pack --silent --pack-destination "$WORK")
ls "$WORK/pydist" "$WORK"/*.tgz
echo '{"language_clients":"pass"}'
