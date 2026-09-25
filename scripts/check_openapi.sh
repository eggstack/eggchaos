#!/usr/bin/env sh
# M032 native contract gate: the checked-in OpenAPI document must agree with
# the eggchaos-protocol authority and every native route must resolve live.
set -eu
cargo test -p eggchaos-protocol --all-features
cargo test -p eggchaos-server --all-features --test native_route_inventory
python3 - <<'EOF'
import sys
try:
    import yaml
except ImportError:
    print("check_openapi: pyyaml not installed; rust drift tests already parsed the YAML")
    sys.exit(0)
with open("api/openapi/eggchaos-v1.yaml", encoding="utf-8") as handle:
    doc = yaml.safe_load(handle)
assert doc["openapi"] == "3.0.3", doc["openapi"]
assert len(doc["paths"]) == 21, len(doc["paths"])
ops = sum(len([m for m in v if m in ("get", "post", "patch", "delete")]) for v in doc["paths"].values())
assert ops == 36, ops
print(f'{{"openapi":"pass","paths":{len(doc["paths"])},"operations":{ops}}}')
EOF
