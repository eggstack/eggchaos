#!/usr/bin/env sh
# M033 TypeScript client gate: typecheck + build + contract/cross-language tests.
set -eu
python3 scripts/sync_sdk_contract.py
git diff --exit-code -- bindings/_contract/operations.json \
  bindings/python-client/eggchaos_client/_generated.py \
  bindings/typescript-client/src/generated.ts
# M059: lockfile-enforcing install on every gate run. The committed
# package-lock.json owns the graph; lifecycle scripts never run
# implicitly at install. Unconditional so a stale local node_modules
# can never shadow the locked graph under test.
(cd bindings/typescript-client && npm ci --ignore-scripts --no-audit --no-fund)
(cd bindings/typescript-client && ./node_modules/.bin/tsc --noEmit)
(cd bindings/typescript-client && ./node_modules/.bin/tsc)
(cd bindings/typescript-client && node --test dist/tests/contract.test.js dist/tests/cross_language.test.js)
echo '{"typescript_client":"pass"}'
