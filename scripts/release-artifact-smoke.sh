#!/usr/bin/env sh
set -eu

binary="${1:-target/release/eggchaos}"
config="${2:-qualification/release/eggchaos.toml}"
log_path="${TMPDIR:-/tmp}/eggchaos-release-smoke.$$.log"

"$binary" serve --config "$config" >"$log_path" 2>&1 &
process_id=$!
cleanup() {
    kill "$process_id" 2>/dev/null || true
    wait "$process_id" 2>/dev/null || true
    rm -f "$log_path"
}
trap cleanup EXIT INT TERM

admin_url=""
attempt=0
while [ "$attempt" -lt 50 ]; do
    if grep -q 'eggchaos listening; admin=' "$log_path" 2>/dev/null; then
        admin_address=$(sed -n 's/.*admin=//p' "$log_path" | tail -1)
        admin_url="http://$admin_address"
        if curl -fsS "$admin_url/v1/health" >/dev/null 2>&1; then
            break
        fi
    fi
    attempt=$((attempt + 1))
    sleep 0.1
done

test -n "$admin_url"
curl -fsS "$admin_url/v1/health" | grep -q '"running":true'
"$binary" --admin "$admin_url" --json proxy list | grep -q '"name":"smoke"'
"$binary" --admin "$admin_url" --json datagram proxy list | grep -q '"name":"udp-smoke"'
"$binary" --admin "$admin_url" --json reset | grep -q '"reset":true'
"$binary" --json version | grep -q '"version":"0.1.0"'
printf '%s\n' '{"artifact_smoke":"pass"}'
