//! Server adapters from protocol wire DTOs into the runtime model.
//!
//! The stable `/v1` JSON contract lives in `eggchaos-protocol`. This module
//! re-exports those DTOs so existing `eggchaos_server::*` imports keep
//! compiling, and adds the server-side assembly that the protocol crate
//! must not own: `ProxySpec` construction, live-policy wiring, runtime
//! view mapping, and limit assembly. There is exactly one DTO definition
//! per wire type (in the protocol crate) and exactly one runtime assembly
//! per DTO family (here).

use std::time::Duration;

pub use eggchaos_protocol::{
    DatagramFaultKindV1, DatagramFaultPatchV1, DatagramFaultSpecV1, DatagramFaultUpsertV1,
    DatagramRuntimeConfigV1, FaultKindV1, FaultPatchV1, FaultUpsertV1,
    NativeDatagramAssociationViewV1, NativeDatagramEvidenceV1, NativeDatagramProxyPatchV1,
    NativeDatagramProxyRequestV1, NativeDatagramProxyViewV1, NativeFaultViewV1, NativeProxyPatchV1,
    NativeProxyRequestV1, NativeProxyViewV1, RuntimeConfigV1, ScenarioActionV1,
    ScenarioEventResultV1, ScenarioEventV1, ScenarioFaultV1, ScenarioRunV1, ScenarioStatusV1,
    ScenarioV1, NATIVE_DEFAULT_BANDWIDTH_BURST_BYTES, NATIVE_DEFAULT_BANDWIDTH_BYTES_PER_SECOND,
    NATIVE_DEFAULT_BUFFER_BYTES, NATIVE_DEFAULT_LIMIT_BYTES, NATIVE_DEFAULT_PROXY_TIMEOUT_MS,
    NATIVE_DEFAULT_SLICE_AVERAGE_SIZE,
};

use crate::{
    AdmissionLimits, DatagramAssociationSnapshot, DatagramProxySpec, DatagramProxyView,
    DatagramRuntimeLimits, FaultPatch, FaultUpsert, ProxyPatch, ProxySpec, ProxyView,
};

/// Assemble a stream proxy definition from its wire request.
///
/// Wire bounds are checked by the protocol DTO first; runtime identity and
/// plan validation remain the `ProxySpec` authority.
pub fn proxy_request_into_spec(request: NativeProxyRequestV1) -> Result<ProxySpec, String> {
    request.validate()?;
    let mut proxy = ProxySpec::new(request.name, request.listen, request.upstream);
    proxy.enabled = request.enabled;
    proxy.max_connections = request.max_connections;
    proxy.connect_timeout = Duration::from_millis(request.connect_timeout_ms);
    proxy.seed = request.seed;
    proxy.validate().map_err(|error| error.to_string())?;
    Ok(proxy)
}

impl From<NativeProxyPatchV1> for ProxyPatch {
    fn from(value: NativeProxyPatchV1) -> Self {
        Self {
            listen: value.listen,
            upstream: value.upstream,
            enabled: value.enabled,
            max_connections: value.max_connections,
            connect_timeout_ms: value.connect_timeout_ms,
        }
    }
}

/// Assemble a runtime fault upsert from its wire request.
pub fn fault_upsert_into_runtime(upsert: FaultUpsertV1) -> Result<FaultUpsert, String> {
    let (direction, spec) = upsert.into_core()?;
    Ok(FaultUpsert {
        direction,
        id: spec.id.to_string(),
        probability: spec.probability.get(),
        kind: spec.kind,
    })
}

/// Assemble a runtime fault patch from its wire request.
pub fn fault_patch_into_runtime(patch: FaultPatchV1) -> Result<FaultPatch, String> {
    let (probability, kind) = patch.into_core()?;
    Ok(FaultPatch { probability, kind })
}

impl From<ProxyView> for NativeProxyViewV1 {
    fn from(value: ProxyView) -> Self {
        Self {
            name: value.name,
            listen: value.listen,
            upstream: value.upstream,
            bound_addr: value.bound_addr,
            running: value.running,
            enabled: value.enabled,
            upstream_faults: value
                .upstream_faults
                .faults()
                .iter()
                .map(NativeFaultViewV1::from)
                .collect(),
            downstream_faults: value
                .downstream_faults
                .faults()
                .iter()
                .map(NativeFaultViewV1::from)
                .collect(),
            upstream_generation: value.upstream_generation,
            downstream_generation: value.downstream_generation,
            upstream_seed_namespace: value.upstream_seed_namespace,
            downstream_seed_namespace: value.downstream_seed_namespace,
            max_connections: value.max_connections,
            connect_timeout_ms: value.connect_timeout_ms,
            seed: value.seed,
        }
    }
}

/// Assemble global admission limits from the wire runtime configuration.
pub fn runtime_admission_limits(config: RuntimeConfigV1) -> Result<AdmissionLimits, String> {
    config.validate()?;
    Ok(AdmissionLimits {
        global_connections: config.global_connections,
        history: config.history,
    })
}

/// Assemble datagram runtime limits from the wire runtime configuration.
pub fn runtime_datagram_limits(
    config: DatagramRuntimeConfigV1,
) -> Result<DatagramRuntimeLimits, String> {
    DatagramRuntimeLimits {
        max_proxies: config.max_proxies,
        max_associations: config.max_associations,
        history: config.history,
        ingress_per_association: config.ingress_per_association,
        max_ingress_queue_bytes: config.max_ingress_queue_bytes,
    }
    .validate()
    .map_err(|error| error.to_string())
}

/// Assemble a runtime Scenario V1 document from its wire DTO.
pub fn scenario_v1_into_runtime(scenario: ScenarioV1) -> Result<crate::Scenario, String> {
    let version = scenario.version;
    let seed = scenario.seed;
    let actions = scenario.into_core_actions()?;
    Ok(crate::Scenario {
        version,
        seed,
        events: actions
            .into_iter()
            .map(|(at_ms, action)| crate::ScenarioEvent { at_ms, action })
            .collect(),
    })
}

impl From<crate::ScenarioRunRecord> for ScenarioRunV1 {
    fn from(record: crate::ScenarioRunRecord) -> Self {
        let status = match record.status {
            crate::ScenarioRunStatus::Pending => ScenarioStatusV1::Pending,
            crate::ScenarioRunStatus::Running => ScenarioStatusV1::Running,
            crate::ScenarioRunStatus::Cancelling => ScenarioStatusV1::Cancelling,
            crate::ScenarioRunStatus::Cancelled => ScenarioStatusV1::Cancelled,
            crate::ScenarioRunStatus::Completed => ScenarioStatusV1::Completed,
            crate::ScenarioRunStatus::Failed => ScenarioStatusV1::Failed,
        };
        Self::from_parts(
            record.run_id,
            record.seed,
            status,
            record.applied,
            record.failure,
            record
                .trail
                .into_iter()
                .map(|event| ScenarioEventResultV1 {
                    index: event.index,
                    at_ms: event.at_ms,
                    action: event.action,
                    proxy: event.proxy,
                    direction: event.direction,
                    global_generation: event.global_generation,
                    upstream_generation: event.upstream_generation,
                    downstream_generation: event.downstream_generation,
                })
                .collect(),
        )
    }
}

/// Assemble a datagram proxy definition from its wire request.
///
/// Wire bounds, queue limits, fault conversion, and cross-direction ID
/// uniqueness are checked by the protocol DTO first; policy wiring and
/// global association validation remain the server authority.
pub fn datagram_proxy_request_into_spec(
    request: NativeDatagramProxyRequestV1,
) -> Result<DatagramProxySpec, String> {
    let parts = request.into_core_parts()?;
    let mut spec =
        DatagramProxySpec::new(parts.name, parts.listen, parts.upstream, parts.queue_limits)
            .map_err(|error| error.to_string())?;
    spec.max_associations = parts.max_associations;
    spec.association_idle_timeout = Duration::from_millis(parts.association_idle_timeout_ms);
    spec.seed = parts.seed;
    spec.upstream_policy = eggchaos_core::DatagramLivePolicy::new(
        eggchaos_core::DatagramPlan::new(parts.upstream_faults)
            .map_err(|error| error.to_string())?,
        parts.seed,
    )
    .map_err(|error| error.to_string())?;
    spec.downstream_policy = eggchaos_core::DatagramLivePolicy::new(
        eggchaos_core::DatagramPlan::new(parts.downstream_faults)
            .map_err(|error| error.to_string())?,
        parts.seed,
    )
    .map_err(|error| error.to_string())?;
    spec.validate(65_536).map_err(|error| error.to_string())?;
    Ok(spec)
}

/// Convert a wire datagram fault patch to validated probability/kind parts.
pub fn datagram_fault_patch_into_parts(
    patch: DatagramFaultPatchV1,
) -> Result<(Option<f64>, Option<eggchaos_core::DatagramFaultKind>), String> {
    patch.into_core_parts()
}

impl From<DatagramProxyView> for NativeDatagramProxyViewV1 {
    fn from(value: DatagramProxyView) -> Self {
        Self {
            name: value.name,
            listen: value.listen,
            bound_addr: value.bound_addr,
            upstream: value.upstream,
            running: value.running,
            max_associations: value.max_associations,
            active_associations: value.active_associations,
            association_idle_timeout_ms: value.association_idle_timeout_ms,
            max_datagram_size: value.max_datagram_size,
            max_queued_datagrams: value.max_queued_datagrams,
            max_queued_bytes: value.max_queued_bytes,
            seed: value.seed,
            upstream_generation: value.upstream_generation,
            downstream_generation: value.downstream_generation,
            oversize_datagrams: value.oversize_datagrams,
            association_capacity_rejections: value.association_capacity_rejections,
            ingress_queue_overflow: value.ingress_queue_overflow,
            association_setup_failures: value.association_setup_failures,
        }
    }
}

impl From<DatagramAssociationSnapshot> for NativeDatagramAssociationViewV1 {
    fn from(value: DatagramAssociationSnapshot) -> Self {
        Self {
            id: value.id,
            proxy: value.proxy,
            client: value.client,
            upstream: value.upstream,
            age_ms: value.age_ms,
            idle_ms: value.idle_ms,
            ingress_datagrams: value.ingress_datagrams,
            ingress_bytes: value.ingress_bytes,
            egress_datagrams: value.egress_datagrams,
            egress_bytes: value.egress_bytes,
            ingress_queue_overflow: value.ingress_queue_overflow,
            oversize_datagrams: value.oversize_datagrams,
            administrative_discards: value.administrative_discards,
            send_errors: value.send_errors,
            upstream_evidence: value.upstream_evidence.into(),
            downstream_evidence: value.downstream_evidence.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggchaos_protocol::validate_proxy_name;

    #[test]
    fn stream_proxy_adapter_builds_the_same_spec_as_wire_validation() {
        let request = NativeProxyRequestV1 {
            name: "cache".into(),
            listen: "127.0.0.1:0".parse().unwrap(),
            upstream: "127.0.0.1:6379".parse().unwrap(),
            enabled: true,
            max_connections: None,
            connect_timeout_ms: 2_500,
            seed: 4,
        };
        let spec = proxy_request_into_spec(request).unwrap();
        assert_eq!(spec.connect_timeout, Duration::from_millis(2_500));
        assert_eq!(spec.seed, 4);
        let invalid = NativeProxyRequestV1 {
            name: "bad name!".into(),
            listen: "127.0.0.1:0".parse().unwrap(),
            upstream: "127.0.0.1:6379".parse().unwrap(),
            enabled: true,
            max_connections: None,
            connect_timeout_ms: 2_500,
            seed: 0,
        };
        assert!(proxy_request_into_spec(invalid).is_err());
    }

    #[test]
    fn protocol_and_runtime_proxy_name_rules_agree() {
        let valid = ["a", "cache-1", "a.b_c-d", &"x".repeat(128)];
        for name in valid {
            assert!(validate_proxy_name(name).is_ok(), "{name}");
            assert!(
                ProxySpec::new(
                    name,
                    "127.0.0.1:0".parse().unwrap(),
                    "127.0.0.1:1".parse().unwrap()
                )
                .validate()
                .is_ok(),
                "{name}"
            );
        }
        let invalid: Vec<String> = vec![
            String::new(),
            "x".repeat(129),
            "bad name".into(),
            "a/b".into(),
            "café".into(),
        ];
        for name in invalid {
            let name = name.as_str();
            assert!(validate_proxy_name(name).is_err(), "{name}");
            assert!(
                ProxySpec::new(
                    name,
                    "127.0.0.1:0".parse().unwrap(),
                    "127.0.0.1:1".parse().unwrap()
                )
                .validate()
                .is_err(),
                "{name}"
            );
        }
    }

    #[test]
    fn datagram_proxy_adapter_preserves_defaults_and_queue_bounds() {
        let request: NativeDatagramProxyRequestV1 = serde_json::from_str(
            r#"{"name":"dns","listen":"127.0.0.1:0","upstream":"127.0.0.1:5353"}"#,
        )
        .unwrap();
        let spec = datagram_proxy_request_into_spec(request).unwrap();
        assert_eq!(spec.max_associations, 256);
        assert_eq!(spec.association_idle_timeout, Duration::from_secs(60));
        let invalid: NativeDatagramProxyRequestV1 = serde_json::from_str(
            r#"{"name":"dns","listen":"127.0.0.1:0","upstream":"127.0.0.1:5353","max_queued_bytes":0}"#,
        ).unwrap();
        assert!(datagram_proxy_request_into_spec(invalid).is_err());
    }

    #[test]
    fn runtime_limit_adapters_agree_with_protocol_bounds() {
        let config = RuntimeConfigV1::default();
        let limits = runtime_admission_limits(config).unwrap();
        assert_eq!(limits.global_connections, 1024);
        assert_eq!(limits.history, 256);
        assert!(runtime_admission_limits(RuntimeConfigV1 {
            global_connections: 1_000_001,
            ..config
        })
        .is_err());
        let datagram = runtime_datagram_limits(config.datagram).unwrap();
        assert_eq!(datagram.max_proxies, 128);
        assert!(runtime_datagram_limits(DatagramRuntimeConfigV1 {
            max_proxies: 0,
            ..config.datagram
        })
        .is_err());
    }

    #[test]
    fn scenario_adapter_assembles_runtime_events() {
        let dto: ScenarioV1 = serde_json::from_str(
            r#"{"version":1,"seed":7,"events":[{"at_ms":25,"action":{"type":"remove-fault","proxy":"cache","direction":"upstream","id":"delay"}}]}"#,
        )
        .unwrap();
        let runtime = scenario_v1_into_runtime(dto).unwrap();
        assert_eq!(runtime.events.len(), 1);
        assert_eq!(runtime.seed, 7);
    }
}
