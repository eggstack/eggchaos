//! Route inventory proof: every `NATIVE_OPERATIONS` entry resolves against
//! a live native admin listener, and every error body uses the shared
//! protocol error envelope.
//!
//! A missing route and a missing resource both answer 404, so this test
//! asserts the envelope `message` is never `"route not found"` and that
//! each operation returns its documented status family.

use eggchaos_server::{AdminConfig, ControlState, NativeAdmin};
use eggfetch_core::Client;

async fn send(
    client: &Client,
    method: &str,
    url: &str,
    body: Option<serde_json::Value>,
) -> (u16, serde_json::Value, String) {
    let mut builder = match method {
        "GET" => client.get(url).unwrap(),
        "POST" => client.post(url).unwrap(),
        "PATCH" => client.patch(url).unwrap(),
        "DELETE" => client.delete(url).unwrap(),
        other => panic!("unsupported method {other}"),
    };
    if let Some(body) = body {
        builder = builder.json(&body).unwrap();
    }
    let mut response = builder.send().await.unwrap();
    let status = response.status().as_u16();
    let bytes = response.bytes().await.unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    (status, json, text)
}

fn assert_routed(status: u16, json: &serde_json::Value, context: &str) {
    if status == 404 {
        let message = json
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        assert_ne!(message, "route not found", "unrouted operation: {context}");
        assert!(json
            .get("error")
            .and_then(|error| error.get("code"))
            .is_some());
    }
    if (400..500).contains(&status) {
        assert!(
            json.get("error")
                .and_then(|error| error.get("code"))
                .is_some(),
            "client errors must use the shared envelope: {context}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_native_operation_resolves_on_the_live_server() {
    let state = ControlState::default();
    let mut admin = NativeAdmin::start(
        AdminConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            ..AdminConfig::default()
        },
        state.clone(),
    )
    .await
    .unwrap();
    let base = format!("http://{}", admin.local_addr());
    let client = Client::builder().build();
    let mut covered = std::collections::HashSet::new();

    macro_rules! call {
        ($op:expr, $method:expr, $url:expr, $body:expr) => {{
            covered.insert($op.to_owned());
            let (status, json, _) = send(&client, $method, &$url, $body).await;
            assert_routed(status, &json, $op);
            (status, json)
        }};
    }

    let (status, _) = call!("getHealth", "GET", format!("{base}/v1/health"), None);
    assert_eq!(status, 200);
    let (status, _) = call!("getVersion", "GET", format!("{base}/v1/version"), None);
    assert_eq!(status, 200);

    let mut response = client
        .get(&format!("{base}/metrics"))
        .unwrap()
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let metrics = String::from_utf8(response.bytes().await.unwrap().to_vec()).unwrap();
    assert!(metrics.contains("eggchaos_"));
    covered.insert("getMetrics".to_owned());

    let (status, _) = call!(
        "createProxy",
        "POST",
        format!("{base}/v1/proxies"),
        Some(serde_json::json!({"name":"web","listen":"127.0.0.1:0","upstream":"127.0.0.1:9"}))
    );
    assert_eq!(status, 201);
    let (status, json) = call!("listProxies", "GET", format!("{base}/v1/proxies"), None);
    assert_eq!(status, 200);
    assert_eq!(json.as_array().unwrap().len(), 1);
    let (status, _) = call!("getProxy", "GET", format!("{base}/v1/proxies/web"), None);
    assert_eq!(status, 200);
    let (status, _) = call!(
        "patchProxy",
        "PATCH",
        format!("{base}/v1/proxies/web"),
        Some(serde_json::json!({"enabled":true}))
    );
    assert_eq!(status, 200);

    let (status, _) = call!(
        "addFault",
        "POST",
        format!("{base}/v1/proxies/web/faults"),
        Some(
            serde_json::json!({"direction":"downstream","id":"lag","probability":1.0,"kind":{"type":"latency","delay_ns":1000000,"jitter_ns":0,"max_buffer_bytes":1024}})
        )
    );
    assert_eq!(status, 201);
    let (status, _) = call!(
        "listFaults",
        "GET",
        format!("{base}/v1/proxies/web/faults"),
        None
    );
    assert_eq!(status, 200);
    let (status, _) = call!(
        "getFault",
        "GET",
        format!("{base}/v1/proxies/web/faults/lag"),
        None
    );
    assert_eq!(status, 200);
    let (status, _) = call!(
        "patchFault",
        "PATCH",
        format!("{base}/v1/proxies/web/faults/lag"),
        Some(serde_json::json!({"probability":0.5}))
    );
    assert_eq!(status, 200);
    let (status, json) = call!(
        "getFault-missing",
        "GET",
        format!("{base}/v1/proxies/web/faults/absent"),
        None
    );
    assert_eq!(status, 404);
    assert_ne!(json["error"]["message"], "route not found");

    let (status, json) = call!(
        "listConnections",
        "GET",
        format!("{base}/v1/connections"),
        None
    );
    assert_eq!(status, 200);
    assert!(json.as_array().is_some());
    let (status, _) = call!(
        "getConnection-bad-id",
        "GET",
        format!("{base}/v1/connections/nope"),
        None
    );
    assert_eq!(status, 400);
    covered.insert("getConnection".to_owned());
    let (status, json) = call!(
        "killConnection-missing",
        "DELETE",
        format!("{base}/v1/connections/999999"),
        None
    );
    assert_eq!(status, 404);
    assert_ne!(json["error"]["message"], "route not found");
    covered.insert("killConnection".to_owned());
    let (status, _) = call!("getHistory", "GET", format!("{base}/v1/history"), None);
    assert_eq!(status, 200);

    let v1 = serde_json::json!({"version":1,"seed":3,"events":[]});
    let (status, json) = call!(
        "applyScenario",
        "POST",
        format!("{base}/v1/scenarios/apply"),
        Some(v1)
    );
    assert_eq!(status, 202);
    let run_id = json["run_id"].as_u64().unwrap();

    let schedule = serde_json::json!({
        "version": 2, "seed": 7, "execution_key": 11,
        "isolation": "strict", "cleanup": "restore-initial",
        "phases": [{"name": "warmup", "duration_ns": 1000000, "actions": [
            {"type": "remove-fault", "proxy": "web", "direction": "downstream", "id": "lag"}
        ]}]
    });
    let (status, _) = call!(
        "validateSchedule",
        "POST",
        format!("{base}/v1/scenarios/validate"),
        Some(schedule.clone())
    );
    assert_eq!(status, 200);
    let (status, json) = call!(
        "compileSchedule",
        "POST",
        format!("{base}/v1/scenarios/compile"),
        Some(schedule)
    );
    assert_eq!(status, 200);
    assert_eq!(
        json["events"].as_array().map(|events| events.len()),
        Some(1)
    );

    let (status, _) = call!(
        "getScenario",
        "GET",
        format!("{base}/v1/scenarios/{run_id}"),
        None
    );
    assert_eq!(status, 200);
    // Allow the v1 run to finish before cancelling so the record exists.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let (status, _) = call!(
        "cancelScenario",
        "DELETE",
        format!("{base}/v1/scenarios/{run_id}"),
        None
    );
    assert!(status == 200 || status == 404);

    let (status, _) = call!(
        "createDatagramProxy",
        "POST",
        format!("{base}/v1/datagram-proxies"),
        Some(serde_json::json!({"name":"dns","listen":"127.0.0.1:0","upstream":"127.0.0.1:9"}))
    );
    assert_eq!(status, 201);
    let (status, _) = call!(
        "listDatagramProxies",
        "GET",
        format!("{base}/v1/datagram-proxies"),
        None
    );
    assert_eq!(status, 200);
    let (status, _) = call!(
        "getDatagramProxy",
        "GET",
        format!("{base}/v1/datagram-proxies/dns"),
        None
    );
    assert_eq!(status, 200);
    let (status, _) = call!(
        "patchDatagramProxy",
        "PATCH",
        format!("{base}/v1/datagram-proxies/dns"),
        Some(serde_json::json!({"enabled":true}))
    );
    assert_eq!(status, 200);
    let (status, _) = call!(
        "addDatagramFault",
        "POST",
        format!("{base}/v1/datagram-proxies/dns/faults"),
        Some(
            serde_json::json!({"direction":"upstream","id":"loss","probability":0.5,"kind":{"type":"loss"}})
        )
    );
    assert_eq!(status, 201);
    let (status, _) = call!(
        "listDatagramFaults",
        "GET",
        format!("{base}/v1/datagram-proxies/dns/faults"),
        None
    );
    assert_eq!(status, 200);
    let (status, _) = call!(
        "getDatagramFault",
        "GET",
        format!("{base}/v1/datagram-proxies/dns/faults/loss"),
        None
    );
    assert_eq!(status, 200);
    let (status, _) = call!(
        "patchDatagramFault",
        "PATCH",
        format!("{base}/v1/datagram-proxies/dns/faults/loss"),
        Some(serde_json::json!({"probability":0.25}))
    );
    assert_eq!(status, 200);
    let (status, _) = call!(
        "deleteDatagramFault",
        "DELETE",
        format!("{base}/v1/datagram-proxies/dns/faults/loss"),
        None
    );
    assert_eq!(status, 200);

    let (status, json) = call!(
        "listDatagramAssociations",
        "GET",
        format!("{base}/v1/datagram-associations"),
        None
    );
    assert_eq!(status, 200);
    assert!(json.as_array().is_some());
    let (status, _) = call!(
        "getDatagramAssociation-bad-id",
        "GET",
        format!("{base}/v1/datagram-associations/nope"),
        None
    );
    assert_eq!(status, 400);
    covered.insert("getDatagramAssociation".to_owned());
    let (status, json) = call!(
        "killDatagramAssociation-missing",
        "DELETE",
        format!("{base}/v1/datagram-associations/999999"),
        None
    );
    assert_eq!(status, 404);
    assert_ne!(json["error"]["message"], "route not found");
    covered.insert("killDatagramAssociation".to_owned());

    let (status, _) = call!(
        "deleteFault",
        "DELETE",
        format!("{base}/v1/proxies/web/faults/lag"),
        None
    );
    assert_eq!(status, 200);
    let (status, _) = call!("resetService", "POST", format!("{base}/v1/reset"), None);
    assert_eq!(status, 200);
    let (status, _) = call!(
        "deleteProxy",
        "DELETE",
        format!("{base}/v1/proxies/web"),
        None
    );
    assert_eq!(status, 200);
    let (status, _) = call!(
        "deleteDatagramProxy",
        "DELETE",
        format!("{base}/v1/datagram-proxies/dns"),
        None
    );
    assert_eq!(status, 200);

    let expected: std::collections::HashSet<String> = eggchaos_protocol::NATIVE_OPERATIONS
        .iter()
        .map(|operation| operation.operation_id.to_owned())
        .collect();
    let missing: Vec<_> = expected.difference(&covered).collect();
    assert!(
        missing.is_empty(),
        "operations never exercised: {missing:?}"
    );

    admin.shutdown();
    admin.wait().await;
    state.shutdown_and_join().await;
}
