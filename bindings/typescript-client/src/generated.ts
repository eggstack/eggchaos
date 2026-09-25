// Generated operation table. DO NOT EDIT: run scripts/sync_sdk_contract.py.

export const OPENAPI_VERSION = "1.0.0";

export interface NativeOperation {
  method: string;
  path: string;
  operationId: string;
}

export const OPERATIONS: NativeOperation[] = [
  { method: "GET", path: "/metrics", operationId: "getMetrics" },
  { method: "GET", path: "/v1/connections", operationId: "listConnections" },
  { method: "DELETE", path: "/v1/connections/{id}", operationId: "killConnection" },
  { method: "GET", path: "/v1/connections/{id}", operationId: "getConnection" },
  { method: "GET", path: "/v1/datagram-associations", operationId: "listDatagramAssociations" },
  { method: "DELETE", path: "/v1/datagram-associations/{id}", operationId: "killDatagramAssociation" },
  { method: "GET", path: "/v1/datagram-associations/{id}", operationId: "getDatagramAssociation" },
  { method: "GET", path: "/v1/datagram-proxies", operationId: "listDatagramProxies" },
  { method: "POST", path: "/v1/datagram-proxies", operationId: "createDatagramProxy" },
  { method: "DELETE", path: "/v1/datagram-proxies/{name}", operationId: "deleteDatagramProxy" },
  { method: "GET", path: "/v1/datagram-proxies/{name}", operationId: "getDatagramProxy" },
  { method: "PATCH", path: "/v1/datagram-proxies/{name}", operationId: "patchDatagramProxy" },
  { method: "GET", path: "/v1/datagram-proxies/{name}/faults", operationId: "listDatagramFaults" },
  { method: "POST", path: "/v1/datagram-proxies/{name}/faults", operationId: "addDatagramFault" },
  { method: "DELETE", path: "/v1/datagram-proxies/{name}/faults/{id}", operationId: "deleteDatagramFault" },
  { method: "GET", path: "/v1/datagram-proxies/{name}/faults/{id}", operationId: "getDatagramFault" },
  { method: "PATCH", path: "/v1/datagram-proxies/{name}/faults/{id}", operationId: "patchDatagramFault" },
  { method: "GET", path: "/v1/health", operationId: "getHealth" },
  { method: "GET", path: "/v1/history", operationId: "getHistory" },
  { method: "GET", path: "/v1/proxies", operationId: "listProxies" },
  { method: "POST", path: "/v1/proxies", operationId: "createProxy" },
  { method: "DELETE", path: "/v1/proxies/{name}", operationId: "deleteProxy" },
  { method: "GET", path: "/v1/proxies/{name}", operationId: "getProxy" },
  { method: "PATCH", path: "/v1/proxies/{name}", operationId: "patchProxy" },
  { method: "GET", path: "/v1/proxies/{name}/faults", operationId: "listFaults" },
  { method: "POST", path: "/v1/proxies/{name}/faults", operationId: "addFault" },
  { method: "DELETE", path: "/v1/proxies/{name}/faults/{id}", operationId: "deleteFault" },
  { method: "GET", path: "/v1/proxies/{name}/faults/{id}", operationId: "getFault" },
  { method: "PATCH", path: "/v1/proxies/{name}/faults/{id}", operationId: "patchFault" },
  { method: "POST", path: "/v1/reset", operationId: "resetService" },
  { method: "POST", path: "/v1/scenarios/apply", operationId: "applyScenario" },
  { method: "POST", path: "/v1/scenarios/compile", operationId: "compileSchedule" },
  { method: "POST", path: "/v1/scenarios/validate", operationId: "validateSchedule" },
  { method: "DELETE", path: "/v1/scenarios/{id}", operationId: "cancelScenario" },
  { method: "GET", path: "/v1/scenarios/{id}", operationId: "getScenario" },
  { method: "GET", path: "/v1/version", operationId: "getVersion" },
];

export const OPERATION_METHODS: Record<string, string> = {
  "addDatagramFault": "addDatagramFault",
  "addFault": "addFault",
  "applyScenario": "applyScenario",
  "cancelScenario": "cancelScenario",
  "compileSchedule": "compileSchedule",
  "createDatagramProxy": "createDatagramProxy",
  "createProxy": "createProxy",
  "deleteDatagramFault": "deleteDatagramFault",
  "deleteDatagramProxy": "deleteDatagramProxy",
  "deleteFault": "deleteFault",
  "deleteProxy": "deleteProxy",
  "getConnection": "getConnection",
  "getDatagramAssociation": "getDatagramAssociation",
  "getDatagramFault": "getDatagramFault",
  "getDatagramProxy": "getDatagramProxy",
  "getFault": "getFault",
  "getHealth": "health",
  "getHistory": "history",
  "getMetrics": "metricsText",
  "getProxy": "getProxy",
  "getScenario": "getScenario",
  "getVersion": "version",
  "killConnection": "killConnection",
  "killDatagramAssociation": "killDatagramAssociation",
  "listConnections": "listConnections",
  "listDatagramAssociations": "listDatagramAssociations",
  "listDatagramFaults": "listDatagramFaults",
  "listDatagramProxies": "listDatagramProxies",
  "listFaults": "listFaults",
  "listProxies": "listProxies",
  "patchDatagramFault": "patchDatagramFault",
  "patchDatagramProxy": "patchDatagramProxy",
  "patchFault": "patchFault",
  "patchProxy": "patchProxy",
  "resetService": "reset",
  "validateSchedule": "validateSchedule",
};

export type StreamFaultTag = "bandwidth" | "blackhole" | "disconnect" | "latency" | "limit-data" | "slice" | "slow-close";
export const STREAM_FAULT_TAGS: StreamFaultTag[] = ["bandwidth", "blackhole", "disconnect", "latency", "limit-data", "slice", "slow-close"];

export type DatagramFaultTag = "bandwidth" | "delay" | "duplicate" | "loss" | "payload-corrupt" | "reorder";
export const DATAGRAM_FAULT_TAGS: DatagramFaultTag[] = ["bandwidth", "delay", "duplicate", "loss", "payload-corrupt", "reorder"];

export type ScenarioActionTag = "remove-datagram-fault" | "remove-fault" | "set-datagram-plan" | "set-plan";
export const SCENARIO_ACTION_TAGS: ScenarioActionTag[] = ["remove-datagram-fault", "remove-fault", "set-datagram-plan", "set-plan"];
