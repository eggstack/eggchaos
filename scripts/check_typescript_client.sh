#!/usr/bin/env sh
# M033 TypeScript client gate: typecheck + build + contract/cross-language tests.
set -eu
python3 scripts/sync_sdk_contract.py
git diff --exit-code -- bindings/_contract/operations.json \
  bindings/python-client/eggchaos_client/_generated.py \
  bindings/typescript-client/src/generated.ts
if [ ! -d bindings/typescript-client/node_modules ]; then
  (cd bindings/typescript-client && npm install --no-audit --no-fund)
fi
(cd bindings/typescript-client && ./node_modules/.bin/tsc --noEmit)
(cd bindings/typescript-client && ./node_modules/.bin/tsc)
(cd bindings/typescript-client && node --test dist/tests/contract.test.js dist/tests/cross_language.test.js)
echo '{"typescript_client":"pass"}'
