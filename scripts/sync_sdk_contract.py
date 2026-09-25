#!/usr/bin/env python3
"""Derive SDK contract artifacts from the M032 OpenAPI authority.

Reads api/openapi/eggchaos-v1.yaml (the checked-in native contract,
drift-checked against eggchaos-protocol by contract_drift.rs) and emits:

- bindings/_contract/operations.json (shared snapshot for both SDKs)
- bindings/python-client/eggchaos_client/_generated.py (operation table)
- bindings/typescript-client/src/generated.ts (operation table + tag unions)

Output is deterministic (sorted keys, fixed separators). CI fails when
regeneration changes committed artifacts unexpectedly.
"""

import json
import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
OPENAPI = ROOT / "api" / "openapi" / "eggchaos-v1.yaml"
SNAPSHOT = ROOT / "bindings" / "_contract" / "operations.json"
PY_GENERATED = ROOT / "bindings" / "python-client" / "eggchaos_client" / "_generated.py"
TS_GENERATED = ROOT / "bindings" / "typescript-client" / "src" / "generated.ts"

METHODS = ("get", "post", "patch", "delete")

# operationId -> idiomatic SDK method name. This table is the reviewed
# hand-off between the OpenAPI authority and both facades; drift tests
# assert every entry resolves to a real client method.
OPERATION_METHODS = {
    "getHealth": "health",
    "getVersion": "version",
    "getMetrics": "metrics_text",
    "resetService": "reset",
    "listProxies": "list_proxies",
    "createProxy": "create_proxy",
    "getProxy": "get_proxy",
    "patchProxy": "patch_proxy",
    "deleteProxy": "delete_proxy",
    "listFaults": "list_faults",
    "addFault": "add_fault",
    "getFault": "get_fault",
    "patchFault": "patch_fault",
    "deleteFault": "delete_fault",
    "listConnections": "list_connections",
    "getConnection": "get_connection",
    "killConnection": "kill_connection",
    "getHistory": "history",
    "applyScenario": "apply_scenario",
    "validateSchedule": "validate_schedule",
    "compileSchedule": "compile_schedule",
    "getScenario": "get_scenario",
    "cancelScenario": "cancel_scenario",
    "listDatagramProxies": "list_datagram_proxies",
    "createDatagramProxy": "create_datagram_proxy",
    "getDatagramProxy": "get_datagram_proxy",
    "patchDatagramProxy": "patch_datagram_proxy",
    "deleteDatagramProxy": "delete_datagram_proxy",
    "listDatagramFaults": "list_datagram_faults",
    "addDatagramFault": "add_datagram_fault",
    "getDatagramFault": "get_datagram_fault",
    "patchDatagramFault": "patch_datagram_fault",
    "deleteDatagramFault": "delete_datagram_fault",
    "listDatagramAssociations": "list_datagram_associations",
    "getDatagramAssociation": "get_datagram_association",
    "killDatagramAssociation": "kill_datagram_association",
}

def _camel(name: str) -> str:
    head, *tail = name.split("_")
    return head + "".join(part.capitalize() for part in tail)


TS_OPERATION_METHODS = {
    operation_id: _camel(method) for operation_id, method in OPERATION_METHODS.items()
}


def main() -> None:
    with OPENAPI.open(encoding="utf-8") as handle:
        doc = yaml.safe_load(handle)
    operations = []
    for path in sorted(doc["paths"]):
        item = doc["paths"][path]
        for method in METHODS:
            if method in item:
                operations.append(
                    {
                        "method": method.upper(),
                        "path": path,
                        "operation_id": item[method]["operationId"],
                    }
                )
    operations.sort(key=lambda op: (op["path"], op["method"]))

    def tags(schema: str) -> list[str]:
        mapping = doc["components"]["schemas"][schema]["discriminator"]["mapping"]
        return sorted(mapping)

    snapshot = {
        "openapi_version": doc["info"]["version"],
        "operations": operations,
        "stream_fault_tags": tags("StreamFaultKind"),
        "datagram_fault_tags": tags("DatagramFaultKind"),
        "scenario_action_tags": tags("ScenarioActionV1"),
    }
    SNAPSHOT.parent.mkdir(parents=True, exist_ok=True)
    SNAPSHOT.write_text(json.dumps(snapshot, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    py_lines = [
        '"""Generated operation table. DO NOT EDIT: run scripts/sync_sdk_contract.py."""',
        "",
        "OPENAPI_VERSION = %r" % snapshot["openapi_version"],
        "",
        "OPERATIONS = (",
    ]
    for op in operations:
        py_lines.append(
            "    {\"method\": %r, \"path\": %r, \"operation_id\": %r},"
            % (op["method"], op["path"], op["operation_id"])
        )
    py_lines += [
        ")",
        "",
        "STREAM_FAULT_TAGS = %r" % (tuple(snapshot["stream_fault_tags"]),),
        "",
        "DATAGRAM_FAULT_TAGS = %r" % (tuple(snapshot["datagram_fault_tags"]),),
        "",
        "SCENARIO_ACTION_TAGS = %r" % (tuple(snapshot["scenario_action_tags"]),),
        "",
        "OPERATION_METHODS = {",
    ]
    for operation_id in sorted(OPERATION_METHODS):
        py_lines.append("    %r: %r," % (operation_id, OPERATION_METHODS[operation_id]))
    py_lines += ["}", ""]
    PY_GENERATED.parent.mkdir(parents=True, exist_ok=True)
    PY_GENERATED.write_text("\n".join(py_lines), encoding="utf-8")

    def ts_union(values: list[str]) -> str:
        return " | ".join('"%s"' % value for value in values)

    ts_lines = [
        "// Generated operation table. DO NOT EDIT: run scripts/sync_sdk_contract.py.",
        "",
        "export const OPENAPI_VERSION = %s;" % json.dumps(snapshot["openapi_version"]),
        "",
        "export interface NativeOperation {",
        "  method: string;",
        "  path: string;",
        "  operationId: string;",
        "}",
        "",
        "export const OPERATIONS: NativeOperation[] = [",
    ]
    for op in operations:
        ts_lines.append(
            "  { method: %s, path: %s, operationId: %s },"
            % (json.dumps(op["method"]), json.dumps(op["path"]), json.dumps(op["operation_id"]))
        )
    ts_lines += [
        "];",
        "",
        "export const OPERATION_METHODS: Record<string, string> = {",
    ]
    for operation_id in sorted(TS_OPERATION_METHODS):
        ts_lines.append(
            "  %s: %s,"
            % (json.dumps(operation_id), json.dumps(TS_OPERATION_METHODS[operation_id]))
        )
    ts_lines += [
        "};",
        "",
        "export type StreamFaultTag = %s;" % ts_union(snapshot["stream_fault_tags"]),
        "export const STREAM_FAULT_TAGS: StreamFaultTag[] = [%s];"
        % ", ".join(json.dumps(tag) for tag in snapshot["stream_fault_tags"]),
        "",
        "export type DatagramFaultTag = %s;" % ts_union(snapshot["datagram_fault_tags"]),
        "export const DATAGRAM_FAULT_TAGS: DatagramFaultTag[] = [%s];"
        % ", ".join(json.dumps(tag) for tag in snapshot["datagram_fault_tags"]),
        "",
        "export type ScenarioActionTag = %s;" % ts_union(snapshot["scenario_action_tags"]),
        "export const SCENARIO_ACTION_TAGS: ScenarioActionTag[] = [%s];"
        % ", ".join(json.dumps(tag) for tag in snapshot["scenario_action_tags"]),
        "",
    ]
    TS_GENERATED.parent.mkdir(parents=True, exist_ok=True)
    TS_GENERATED.write_text("\n".join(ts_lines), encoding="utf-8")
    print(
        json.dumps(
            {
                "sync": "pass",
                "operations": len(operations),
                "openapi_version": snapshot["openapi_version"],
            }
        )
    )


if __name__ == "__main__":
    sys.exit(main())
