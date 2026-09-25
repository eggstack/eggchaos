//! Native operation inventory: the single machine-readable route authority.
//!
//! Every implemented native route appears exactly once in
//! [`NATIVE_OPERATIONS`]. The checked-in OpenAPI document and the
//! drift tests compare against this table, so adding a route without
//! updating the table fails CI by construction.
//!
//! Path templates use `{name}` / `{id}` placeholders matching the
//! OpenAPI document. Authentication is uniform: when the admin listener
//! is configured with a bearer token, every operation below requires
//! `Authorization: Bearer <token>`; loopback listeners without a token
//! accept unauthenticated requests.

use serde::{Deserialize, Serialize};

/// OpenAPI contract document version served alongside the DTO authority.
///
/// Bump only with an explicit versioned contract decision; DTO moves alone
/// (M032) do not bump the native contract.
pub const OPENAPI_CONTRACT_VERSION: &str = "1.0.0";

/// One native control operation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct NativeOperation {
    /// HTTP method (`GET`, `POST`, `PATCH`, `DELETE`).
    pub method: &'static str,
    /// Path template (`/v1/proxies/{name}/faults/{id}`, `/metrics`, ...).
    pub path: &'static str,
    /// Stable operation identifier used by the OpenAPI document.
    pub operation_id: &'static str,
    /// Short human summary.
    pub summary: &'static str,
}

/// Complete implemented native operation inventory (36 operations).
///
/// Order is grouped by resource family and is part of the reviewed
/// contract; the drift test compares as a set, not as a sequence.
pub const NATIVE_OPERATIONS: &[NativeOperation] = &[
    NativeOperation {
        method: "GET",
        path: "/v1/health",
        operation_id: "getHealth",
        summary: "Service liveness and global generation",
    },
    NativeOperation {
        method: "GET",
        path: "/v1/version",
        operation_id: "getVersion",
        summary: "Service and native API version",
    },
    NativeOperation {
        method: "GET",
        path: "/metrics",
        operation_id: "getMetrics",
        summary: "Prometheus text metrics (no /v1 prefix)",
    },
    NativeOperation {
        method: "POST",
        path: "/v1/reset",
        operation_id: "resetService",
        summary: "Reset stream and datagram state to configured definitions",
    },
    NativeOperation {
        method: "GET",
        path: "/v1/proxies",
        operation_id: "listProxies",
        summary: "List stream proxies",
    },
    NativeOperation {
        method: "POST",
        path: "/v1/proxies",
        operation_id: "createProxy",
        summary: "Create a stream proxy",
    },
    NativeOperation {
        method: "GET",
        path: "/v1/proxies/{name}",
        operation_id: "getProxy",
        summary: "Get a stream proxy",
    },
    NativeOperation {
        method: "PATCH",
        path: "/v1/proxies/{name}",
        operation_id: "patchProxy",
        summary: "Update a stream proxy",
    },
    NativeOperation {
        method: "DELETE",
        path: "/v1/proxies/{name}",
        operation_id: "deleteProxy",
        summary: "Delete a stream proxy",
    },
    NativeOperation {
        method: "GET",
        path: "/v1/proxies/{name}/faults",
        operation_id: "listFaults",
        summary: "List stream faults by direction",
    },
    NativeOperation {
        method: "POST",
        path: "/v1/proxies/{name}/faults",
        operation_id: "addFault",
        summary: "Add a stream fault",
    },
    NativeOperation {
        method: "GET",
        path: "/v1/proxies/{name}/faults/{id}",
        operation_id: "getFault",
        summary: "Get a stream fault",
    },
    NativeOperation {
        method: "PATCH",
        path: "/v1/proxies/{name}/faults/{id}",
        operation_id: "patchFault",
        summary: "Update a stream fault",
    },
    NativeOperation {
        method: "DELETE",
        path: "/v1/proxies/{name}/faults/{id}",
        operation_id: "deleteFault",
        summary: "Remove a stream fault",
    },
    NativeOperation {
        method: "GET",
        path: "/v1/connections",
        operation_id: "listConnections",
        summary: "List active stream connections",
    },
    NativeOperation {
        method: "GET",
        path: "/v1/connections/{id}",
        operation_id: "getConnection",
        summary: "Get an active stream connection",
    },
    NativeOperation {
        method: "DELETE",
        path: "/v1/connections/{id}",
        operation_id: "killConnection",
        summary: "Terminate an active stream connection",
    },
    NativeOperation {
        method: "GET",
        path: "/v1/history",
        operation_id: "getHistory",
        summary: "List retained closed-connection evidence",
    },
    NativeOperation {
        method: "POST",
        path: "/v1/scenarios/apply",
        operation_id: "applyScenario",
        summary: "Apply a Scenario V1 document or Scenario V2 schedule",
    },
    NativeOperation {
        method: "POST",
        path: "/v1/scenarios/validate",
        operation_id: "validateSchedule",
        summary: "Validate a Scenario V2 schedule without creating a run",
    },
    NativeOperation {
        method: "POST",
        path: "/v1/scenarios/compile",
        operation_id: "compileSchedule",
        summary: "Compile a Scenario V2 schedule without creating a run",
    },
    NativeOperation {
        method: "GET",
        path: "/v1/scenarios/{id}",
        operation_id: "getScenario",
        summary: "Get a Scenario V1 or V2 run record",
    },
    NativeOperation {
        method: "DELETE",
        path: "/v1/scenarios/{id}",
        operation_id: "cancelScenario",
        summary: "Cancel a Scenario V1 or V2 run",
    },
    NativeOperation {
        method: "GET",
        path: "/v1/datagram-proxies",
        operation_id: "listDatagramProxies",
        summary: "List datagram proxies",
    },
    NativeOperation {
        method: "POST",
        path: "/v1/datagram-proxies",
        operation_id: "createDatagramProxy",
        summary: "Create a datagram proxy",
    },
    NativeOperation {
        method: "GET",
        path: "/v1/datagram-proxies/{name}",
        operation_id: "getDatagramProxy",
        summary: "Get a datagram proxy",
    },
    NativeOperation {
        method: "PATCH",
        path: "/v1/datagram-proxies/{name}",
        operation_id: "patchDatagramProxy",
        summary: "Update a datagram proxy",
    },
    NativeOperation {
        method: "DELETE",
        path: "/v1/datagram-proxies/{name}",
        operation_id: "deleteDatagramProxy",
        summary: "Delete a datagram proxy",
    },
    NativeOperation {
        method: "GET",
        path: "/v1/datagram-proxies/{name}/faults",
        operation_id: "listDatagramFaults",
        summary: "List datagram faults by direction",
    },
    NativeOperation {
        method: "POST",
        path: "/v1/datagram-proxies/{name}/faults",
        operation_id: "addDatagramFault",
        summary: "Add a datagram fault",
    },
    NativeOperation {
        method: "GET",
        path: "/v1/datagram-proxies/{name}/faults/{id}",
        operation_id: "getDatagramFault",
        summary: "Get a datagram fault",
    },
    NativeOperation {
        method: "PATCH",
        path: "/v1/datagram-proxies/{name}/faults/{id}",
        operation_id: "patchDatagramFault",
        summary: "Update a datagram fault",
    },
    NativeOperation {
        method: "DELETE",
        path: "/v1/datagram-proxies/{name}/faults/{id}",
        operation_id: "deleteDatagramFault",
        summary: "Remove a datagram fault",
    },
    NativeOperation {
        method: "GET",
        path: "/v1/datagram-associations",
        operation_id: "listDatagramAssociations",
        summary: "List datagram associations",
    },
    NativeOperation {
        method: "GET",
        path: "/v1/datagram-associations/{id}",
        operation_id: "getDatagramAssociation",
        summary: "Get a datagram association",
    },
    NativeOperation {
        method: "DELETE",
        path: "/v1/datagram-associations/{id}",
        operation_id: "killDatagramAssociation",
        summary: "Terminate a datagram association",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn operation_inventory_has_unique_operation_ids_and_paths() {
        assert_eq!(NATIVE_OPERATIONS.len(), 36);
        let mut ids = HashSet::new();
        let mut routes = HashSet::new();
        for operation in NATIVE_OPERATIONS {
            assert!(ids.insert(operation.operation_id), "duplicate operation id");
            assert!(
                routes.insert((operation.method, operation.path)),
                "duplicate method+path"
            );
        }
    }
}
