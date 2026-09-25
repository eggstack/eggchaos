#!/usr/bin/env sh
# M033 Python client gate: regeneration drift + unit tests (no server).
set -eu
python3 scripts/sync_sdk_contract.py
git diff --exit-code -- bindings/_contract/operations.json \
  bindings/python-client/eggchaos_client/_generated.py \
  bindings/typescript-client/src/generated.ts
python3 -m pytest bindings/python-client/tests/test_models.py \
  bindings/python-client/tests/test_contract.py \
  bindings/python-client/tests/test_cross_language.py -q
python3 - <<'EOF'
import sys
sys.path.insert(0, "bindings/python-client")
import eggchaos_client
assert eggchaos_client.Client and eggchaos_client.AsyncClient
print('{"python_client":"pass"}')
EOF
