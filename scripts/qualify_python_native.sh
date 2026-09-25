#!/usr/bin/env sh
# M034 binding qualification: remote/native conformance plus
# startup/control overhead measurements (recorded, not budgeted).
set -eu
cargo build -p eggchaos-cli --locked
WORK="$(mktemp -d)"
trap 'kill "${SERVER_PID:-}" 2>/dev/null; wait "${SERVER_PID:-}" 2>/dev/null; rm -rf "$WORK"' EXIT INT TERM
NATIVE_TARGET=""
if [ "$(python3 -c 'import platform; print(platform.machine())')" = "x86_64" ]; then
  NATIVE_TARGET="--target x86_64-apple-darwin"
fi
cat > "$WORK/eggchaos.toml" <<'EOF'
version = 1
seed = 11

[admin]
bind = "127.0.0.1:0"
EOF
./target/debug/eggchaos serve --config "$WORK/eggchaos.toml" > "$WORK/server.log" 2>&1 &
SERVER_PID=$!
ADMIN=""
for _ in $(seq 1 100); do
  ADMIN="$(grep -o 'admin=[^ ]*' "$WORK/server.log" | tail -1 | cut -c7- || true)"
  if [ -n "$ADMIN" ]; then break; fi
  sleep 0.1
done
if [ -z "$ADMIN" ]; then
  echo "server did not report an admin address"; cat "$WORK/server.log"; exit 1
fi
(cd bindings/python-native && python3 -m maturin build ${NATIVE_TARGET:-} --out "$WORK/wheels")
WHEEL="$(ls "$WORK"/wheels/*.whl | head -1)"
pip install --quiet --target "$WORK/pylibs" --no-deps "$WHEEL"
export PYTHONPATH="$WORK/pylibs"
EGGCHAOS_ADMIN_URL="http://$ADMIN" EGGCHAOS_CLIENT_PATH="$PWD/bindings/python-client" \
  python3 -m pytest bindings/python-native/tests/ -q
EGGCHAOS_ADMIN_URL="http://$ADMIN" EGGCHAOS_CLIENT_PATH="$PWD/bindings/python-client" \
  PYTHONPATH="$WORK/pylibs:$PWD/bindings/python-client" python3 - <<'EOF'
import json
import time
from statistics import mean

import eggchaos_native as native
from eggchaos_client import Client

ADMIN = __import__("os").environ["EGGCHAOS_ADMIN_URL"]

def bench(label, runs, func):
    samples = []
    for _ in range(runs):
        start = time.perf_counter()
        func()
        samples.append((time.perf_counter() - start) * 1000.0)
    result = {"label": label, "runs": runs, "mean_ms": round(mean(samples), 3),
              "max_ms": round(max(samples), 3)}
    print(json.dumps(result))
    return result

results = []
results.append(bench("native_startup_shutdown", 5,
                     lambda: native.Service(seed=1).close()))
service = native.Service(seed=1)
pcounter = [0]
def native_create_delete():
    pcounter[0] += 1
    name = f"bench{pcounter[0]}"
    service.create_proxy(name=name, listen="127.0.0.1:0", upstream="127.0.0.1:9")
    service.delete_proxy(name)
results.append(bench("native_create_delete_proxy", 10, native_create_delete))
service.create_proxy(name="bench", listen="127.0.0.1:0", upstream="127.0.0.1:9")
counter = [0]
def native_mutation():
    counter[0] += 1
    name = f"lag{counter[0]}"
    service.set_fault("bench", native.Fault.latency(
        name, direction="downstream", delay_ns=1000))
    service.remove_fault("bench", name)
results.append(bench("native_fault_mutation", 20, native_mutation))
from eggchaos_client import LatencyFault, ProxyCreate
with Client(base_url=ADMIN) as remote:
    remote.create_proxy(ProxyCreate(name="rbench", listen="127.0.0.1:0",
                                    upstream="127.0.0.1:9"))
    rcounter = [0]
    def remote_mutation():
        rcounter[0] += 1
        name = f"lag{rcounter[0]}"
        remote.add_fault("rbench", "downstream", name, LatencyFault(delay_ns=1000))
        remote.delete_fault("rbench", name)
    results.append(bench("remote_fault_mutation", 20, remote_mutation))
    remote.delete_proxy("rbench")
schedule = {"version": 2, "seed": 7, "execution_key": 11, "isolation": "strict",
            "cleanup": "restore-initial",
            "phases": [{"name": "p", "duration_ns": 1000000, "actions": [
                {"type": "remove-fault", "proxy": "bench",
                 "direction": "downstream", "id": "lag"}]}]}
results.append(bench("native_schedule_validate", 20,
                     lambda: service.schedule_validate(schedule)))
service.close()
with open("bench-native.json", "w", encoding="utf-8") as handle:
    json.dump(results, handle, indent=2)
print('{"python_native_qualify":"pass"}')
EOF
