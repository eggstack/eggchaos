//! Facade lifecycle/control tests plus protocol-equivalence conformance
//! against the HTTP admin path.

use eggchaos_embed::{EmbedOptions, EmbeddedService};
use eggchaos_protocol::{
    DatagramFaultPatchV1, DatagramFaultUpsertV1, FaultUpsertV1, NativeDatagramProxyRequestV1,
    NativeProxyRequestV1, ScenarioScheduleV2Dto, ScenarioV1,
};

fn proxy(name: &str) -> NativeProxyRequestV1 {
    serde_json::from_value(serde_json::json!({
        "name": name, "listen": "127.0.0.1:0", "upstream": "127.0.0.1:9"
    }))
    .unwrap()
}

#[test]
fn start_health_version_shutdown_and_double_close() {
    let service = EmbeddedService::start(EmbedOptions::default()).unwrap();
    assert!(!service.is_closed());
    assert!(service.health().unwrap().running);
    assert_eq!(service.version().unwrap().api, "v1");
    assert!(service.metrics_text().unwrap().contains("eggchaos_"));
    service.shutdown();
    assert!(service.is_closed());
    service.shutdown();
    assert!(service.list_proxies().is_err());
}

#[test]
fn drop_without_close_still_shuts_down() {
    let bound = {
        let service = EmbeddedService::start(EmbedOptions::default()).unwrap();
        let (view, _) = service.create_proxy(proxy("dropme")).unwrap();
        assert!(view.running);
        view.bound_addr.unwrap()
    };
    let _ = bound;
    // A fresh service must be able to start again; the dropped service's
    // runtime was released with it (no detached listener ownership).
    let service = EmbeddedService::start(EmbedOptions::default()).unwrap();
    service.shutdown();
}

#[test]
fn repeated_construction_has_independent_state() {
    let first = EmbeddedService::start(EmbedOptions::default()).unwrap();
    first.create_proxy(proxy("solo")).unwrap();
    assert_eq!(first.list_proxies().unwrap().len(), 1);
    let second = EmbeddedService::start(EmbedOptions::default()).unwrap();
    assert!(second.list_proxies().unwrap().is_empty());
    assert!(second.get_proxy("solo").is_err());
    first.shutdown();
    second.shutdown();
}

#[test]
fn concurrent_calls_from_multiple_threads_are_safe() {
    let service = EmbeddedService::start(EmbedOptions::default()).unwrap();
    service.create_proxy(proxy("shared")).unwrap();
    std::thread::scope(|scope| {
        for index in 0..8 {
            let service = &service;
            scope.spawn(move || {
                for _ in 0..10 {
                    let _ = service.health().unwrap();
                    let _ = service.list_proxies().unwrap();
                    let _ = service.generation();
                    let _ = index;
                }
            });
        }
    });
    service.shutdown();
}

#[test]
fn stream_proxy_and_fault_crud_round_trip() {
    let service = EmbeddedService::start(EmbedOptions::default()).unwrap();
    let (view, _) = service.create_proxy(proxy("web")).unwrap();
    assert!(view.running);
    let patched = service
        .patch_proxy(
            "web",
            serde_json::from_value(serde_json::json!({"enabled": true})).unwrap(),
        )
        .unwrap();
    assert!(patched.0.enabled);
    let upsert: FaultUpsertV1 = serde_json::from_value(serde_json::json!({
        "direction": "downstream", "id": "lag", "probability": 0.5,
        "kind": {"type": "latency", "delay_ns": 1000, "jitter_ns": 0, "max_buffer_bytes": 1024}
    }))
    .unwrap();
    let (direction, fault, _) = service.add_fault("web", upsert).unwrap();
    assert_eq!(direction, eggchaos_core::Direction::Downstream);
    assert_eq!(fault.id, "lag");
    let (upstream, downstream) = service.list_faults("web").unwrap();
    assert!(upstream.is_empty() && downstream.len() == 1);
    let (_, got) = service.get_fault("web", "lag").unwrap();
    assert_eq!(got.probability, 0.5);
    let (_, updated, _) = service
        .patch_fault(
            "web",
            "lag",
            serde_json::from_value(serde_json::json!({"probability": 0.25})).unwrap(),
        )
        .unwrap();
    assert_eq!(updated.probability, 0.25);
    // Empty patch is rejected like the HTTP path.
    assert!(service
        .patch_fault(
            "web",
            "lag",
            serde_json::from_value(serde_json::json!({})).unwrap()
        )
        .is_err());
    service.remove_fault("web", "lag").unwrap();
    assert!(service.get_fault("web", "lag").is_err());
    assert!(service.reset().unwrap().reset);
    service.delete_proxy("web").unwrap();
    assert!(service.get_proxy("web").is_err());
    service.shutdown();
}

#[test]
fn invalid_config_and_missing_resources_map_to_categories() {
    let service = EmbeddedService::start(EmbedOptions::default()).unwrap();
    let bad: NativeProxyRequestV1 = serde_json::from_value(serde_json::json!({
        "name": "bad name!", "listen": "127.0.0.1:0", "upstream": "127.0.0.1:9"
    }))
    .unwrap();
    assert!(matches!(
        service.create_proxy(bad),
        Err(eggchaos_embed::EmbedError::Validation(_))
    ));
    assert!(matches!(
        service.get_proxy("absent"),
        Err(eggchaos_embed::EmbedError::NotFound(_))
    ));
    assert!(matches!(
        service.get_scenario(999),
        Err(eggchaos_embed::EmbedError::NotFound(_))
    ));
    service.shutdown();
}

#[test]
fn datagram_proxy_and_fault_crud_round_trip() {
    let service = EmbeddedService::start(EmbedOptions::default()).unwrap();
    let request: NativeDatagramProxyRequestV1 = serde_json::from_value(serde_json::json!({
        "name": "dns", "listen": "127.0.0.1:0", "upstream": "127.0.0.1:9"
    }))
    .unwrap();
    let (view, _) = service.create_datagram_proxy(request).unwrap();
    assert!(view.running);
    let upsert: DatagramFaultUpsertV1 = serde_json::from_value(serde_json::json!({
        "direction": "upstream", "id": "loss", "probability": 0.5, "kind": {"type": "loss"}
    }))
    .unwrap();
    let (direction, fault, _) = service.add_datagram_fault("dns", upsert).unwrap();
    assert_eq!(direction, eggchaos_core::Direction::Upstream);
    assert_eq!(fault.id, "loss");
    // Duplicate across directions is a conflict, matching HTTP semantics.
    let dupe: DatagramFaultUpsertV1 = serde_json::from_value(serde_json::json!({
        "direction": "downstream", "id": "loss", "probability": 1.0, "kind": {"type": "loss"}
    }))
    .unwrap();
    assert!(service.add_datagram_fault("dns", dupe).is_err());
    let (_, got) = service.get_datagram_fault("dns", "loss").unwrap();
    assert_eq!(got.probability, 0.5);
    let (_, updated, _) = service
        .patch_datagram_fault(
            "dns",
            "loss",
            serde_json::from_value(serde_json::json!({"probability": 0.25})).unwrap(),
        )
        .unwrap();
    assert_eq!(updated.probability, 0.25);
    service.remove_datagram_fault("dns", "loss").unwrap();
    assert!(service.get_datagram_fault("dns", "loss").is_err());
    assert!(service.datagram_associations().unwrap().is_empty());
    assert!(!service.kill_datagram_association(999).unwrap());
    service.delete_datagram_proxy("dns").unwrap();
    service.shutdown();
}

#[test]
fn scenario_v1_and_v2_lifecycle() {
    let service = EmbeddedService::start(EmbedOptions::default()).unwrap();
    service.create_proxy(proxy("web")).unwrap();
    let v1: ScenarioV1 = serde_json::from_str(r#"{"version":1,"seed":3,"events":[]}"#).unwrap();
    let run = service.apply_scenario_v1(v1).unwrap();
    assert_eq!(run.seed, 3);
    let schedule: ScenarioScheduleV2Dto = serde_json::from_value(serde_json::json!({
        "version": 2, "seed": 7, "execution_key": 11,
        "isolation": "strict", "cleanup": "restore-initial",
        "phases": [{"name": "probe", "duration_ns": 1000000, "actions": [
            {"type": "remove-fault", "proxy": "web", "direction": "downstream", "id": "lag"}
        ]}]
    }))
    .unwrap();
    let validated = service.validate_schedule_v2(schedule.clone()).unwrap();
    assert_eq!(validated.event_count, 1);
    let compiled = service.compile_schedule_v2(schedule.clone()).unwrap();
    assert_eq!(compiled.events.len(), 1);
    let run = service.apply_schedule_v2(schedule).unwrap();
    let status = service.get_scenario(run.run_id).unwrap();
    assert!(matches!(status, eggchaos_embed::ScenarioRunView::V2(_)));
    let cancelled = service.cancel_scenario(run.run_id).unwrap();
    assert!(matches!(cancelled, eggchaos_embed::ScenarioRunView::V2(_)));
    assert!(service.connections().unwrap().is_empty());
    assert!(service.history().unwrap().is_empty());
    assert!(!service.kill_connection(999).unwrap());
    service.shutdown();
}

#[test]
fn facade_and_http_admin_agree_on_protocol_views() {
    let service = EmbeddedService::start(EmbedOptions::default()).unwrap();
    service.create_proxy(proxy("web")).unwrap();
    let upsert: FaultUpsertV1 = serde_json::from_value(serde_json::json!({
        "direction": "downstream", "id": "lag", "probability": 1.0,
        "kind": {"type": "latency", "delay_ns": 1000, "jitter_ns": 0, "max_buffer_bytes": 1024}
    }))
    .unwrap();
    service.add_fault("web", upsert).unwrap();
    let facade_proxy = serde_json::to_value(service.get_proxy("web").unwrap()).unwrap();
    let (_, facade_fault) = service.get_fault("web", "lag").unwrap();
    let facade_fault = serde_json::to_value(facade_fault).unwrap();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let state = service.control_state();
        let mut admin = eggchaos_server::NativeAdmin::start(
            eggchaos_server::AdminConfig {
                bind: "127.0.0.1:0".parse().unwrap(),
                ..eggchaos_server::AdminConfig::default()
            },
            state,
        )
        .await
        .unwrap();
        let client = eggfetch_core::Client::builder().build();
        let base = format!("http://{}", admin.local_addr());
        let mut response = client
            .get(&format!("{base}/v1/proxies/web"))
            .unwrap()
            .send()
            .await
            .unwrap();
        let http_proxy: serde_json::Value =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        // Bound addresses differ (facade and HTTP share one service here,
        // so they must be identical).
        assert_eq!(http_proxy, facade_proxy);
        let mut response = client
            .get(&format!("{base}/v1/proxies/web/faults/lag"))
            .unwrap()
            .send()
            .await
            .unwrap();
        let http_fault: serde_json::Value =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert_eq!(http_fault["fault"], facade_fault);
        admin.shutdown();
        admin.wait().await;
    });
    service.shutdown();
}

fn datagram_upsert(direction: &str, id: &str, probability: f64) -> DatagramFaultUpsertV1 {
    serde_json::from_value(serde_json::json!({
        "direction": direction, "id": id, "probability": probability,
        "kind": {"type": "loss"}
    }))
    .unwrap()
}

/// Datagram HTTP/embed conformance for the shared fault mutation
/// authority: create/get/list/patch/delete plus same-direction,
/// cross-direction, empty-patch, and post-delete not-found behavior.
/// Both surfaces delegate to one `ControlState` authority, so any
/// reintroduced semantic split fails here.
#[test]
fn datagram_facade_and_http_admin_agree_on_mutation_and_conflicts() {
    let service = EmbeddedService::start(EmbedOptions::default()).unwrap();
    let request: NativeDatagramProxyRequestV1 = serde_json::from_value(serde_json::json!({
        "name": "dconf", "listen": "127.0.0.1:0", "upstream": "127.0.0.1:9"
    }))
    .unwrap();
    let (embed_view, _) = service.create_datagram_proxy(request).unwrap();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (mut admin, base) = runtime.block_on(async {
        let state = service.control_state();
        let admin = eggchaos_server::NativeAdmin::start(
            eggchaos_server::AdminConfig {
                bind: "127.0.0.1:0".parse().unwrap(),
                ..eggchaos_server::AdminConfig::default()
            },
            state,
        )
        .await
        .unwrap();
        let base = format!("http://{}", admin.local_addr());
        (admin, base)
    });

    // 1. Datagram proxy creation view agrees.
    runtime.block_on(async {
        let client = eggfetch_core::Client::builder().build();
        let mut response = client
            .get(&format!("{base}/v1/datagram-proxies/dconf"))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let http_view: serde_json::Value =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert_eq!(http_view, serde_json::to_value(&embed_view).unwrap());
    });

    // 2. Add an upstream fault through embed.
    let (direction, fault, _) = service
        .add_datagram_fault("dconf", datagram_upsert("upstream", "lag", 1.0))
        .unwrap();
    assert_eq!(direction, eggchaos_core::Direction::Upstream);
    let facade_fault = serde_json::to_value(&fault).unwrap();

    // 3-4. Get-by-path and directional list agree across surfaces.
    // NOTE: embed calls must stay outside `block_on` (the facade owns a
    // private runtime and cannot block from inside another runtime).
    let http_list = runtime.block_on(async {
        let client = eggfetch_core::Client::builder().build();
        let mut response = client
            .get(&format!("{base}/v1/datagram-proxies/dconf/faults/lag"))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let http_got: serde_json::Value =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert_eq!(http_got["direction"], "upstream");
        assert_eq!(http_got["fault"], facade_fault);
        let mut response = client
            .get(&format!("{base}/v1/datagram-proxies/dconf/faults"))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let http_list: serde_json::Value =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert_eq!(http_list["upstream"]["faults"].as_array().unwrap().len(), 1);
        assert_eq!(http_list["upstream"]["faults"][0], facade_fault);
        assert!(http_list["downstream"]["faults"]
            .as_array()
            .unwrap()
            .is_empty());
        http_list
    });
    let (embed_upstream, embed_downstream) = service.list_datagram_faults("dconf").unwrap();
    assert_eq!(
        http_list["upstream"]["faults"],
        serde_json::to_value(&embed_upstream).unwrap()
    );
    assert_eq!(
        http_list["downstream"]["faults"],
        serde_json::to_value(&embed_downstream).unwrap()
    );

    // 5-6. Duplicate conflicts agree: same-direction and cross-direction
    // adds are conflicts on both surfaces.
    assert!(matches!(
        service.add_datagram_fault("dconf", datagram_upsert("upstream", "lag", 0.5)),
        Err(eggchaos_embed::EmbedError::Conflict(_))
    ));
    assert!(matches!(
        service.add_datagram_fault("dconf", datagram_upsert("downstream", "lag", 0.5)),
        Err(eggchaos_embed::EmbedError::Conflict(_))
    ));
    runtime.block_on(async {
        let client = eggfetch_core::Client::builder().build();
        for direction in ["upstream", "downstream"] {
            let response = client
                .post(&format!("{base}/v1/datagram-proxies/dconf/faults"))
                .unwrap()
                .json(&serde_json::json!({
                    "direction": direction, "id": "lag",
                    "probability": 0.5, "kind": {"type": "loss"}
                }))
                .unwrap()
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 409, "duplicate via {direction}");
        }
    });

    // 7. Patch through HTTP is visible through embed.
    runtime.block_on(async {
        let client = eggfetch_core::Client::builder().build();
        let mut response = client
            .patch(&format!("{base}/v1/datagram-proxies/dconf/faults/lag"))
            .unwrap()
            .json(&serde_json::json!({"probability": 0.5}))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let patched: serde_json::Value =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert_eq!(patched["fault"]["probability"], 0.5);
    });
    let (_, got) = service.get_datagram_fault("dconf", "lag").unwrap();
    assert_eq!(got.probability, 0.5);

    // 8. Patch through embed is visible through HTTP.
    let (_, updated, _) = service
        .patch_datagram_fault(
            "dconf",
            "lag",
            serde_json::from_value(serde_json::json!({"probability": 0.25})).unwrap(),
        )
        .unwrap();
    assert_eq!(updated.probability, 0.25);
    runtime.block_on(async {
        let client = eggfetch_core::Client::builder().build();
        let mut response = client
            .get(&format!("{base}/v1/datagram-proxies/dconf/faults/lag"))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let http_got: serde_json::Value =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert_eq!(http_got["fault"]["probability"], 0.25);
    });

    // 9. Empty patches are rejected on both surfaces.
    let empty: DatagramFaultPatchV1 = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(matches!(
        service.patch_datagram_fault("dconf", "lag", empty),
        Err(eggchaos_embed::EmbedError::Validation(_))
    ));
    runtime.block_on(async {
        let client = eggfetch_core::Client::builder().build();
        let response = client
            .patch(&format!("{base}/v1/datagram-proxies/dconf/faults/lag"))
            .unwrap()
            .json(&serde_json::json!({}))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
    });

    // 10. Delete through HTTP is a not-found on both surfaces after.
    runtime.block_on(async {
        let client = eggfetch_core::Client::builder().build();
        let mut response = client
            .delete(&format!("{base}/v1/datagram-proxies/dconf/faults/lag"))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let deleted: serde_json::Value =
            serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert_eq!(deleted["deleted"], true);
        let response = client
            .get(&format!("{base}/v1/datagram-proxies/dconf/faults/lag"))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 404);
    });
    assert!(matches!(
        service.get_datagram_fault("dconf", "lag"),
        Err(eggchaos_embed::EmbedError::NotFound(_))
    ));
    assert!(matches!(
        service.remove_datagram_fault("dconf", "lag"),
        Err(eggchaos_embed::EmbedError::NotFound(_))
    ));

    // 11. Proxy delete cleans up on both surfaces.
    service.delete_datagram_proxy("dconf").unwrap();
    runtime.block_on(async {
        let client = eggfetch_core::Client::builder().build();
        let response = client
            .get(&format!("{base}/v1/datagram-proxies/dconf"))
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 404);
        admin.shutdown();
        admin.wait().await;
    });
    service.shutdown();
}
