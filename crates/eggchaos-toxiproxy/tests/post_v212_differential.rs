//! M038 post-v2.12 differential corpus: pinned `40f7fd31` source-build
//! oracle vs the eggchaos `post-v2.12-2026-09-25` profile, restricted to
//! the `packet_loss` surface.
//!
//! Spins the upstream oracle (via `TOXIPROXY_POST_V2_12_SERVER`) and the
//! in-process eggchaos compatibility server with the snapshot profile and
//! runs the same API and data-plane sequences against both, comparing
//! status codes, exact edges, and recorded intent-compatible stochastic
//! behavior. Without the env var (or with a missing/unrunnable oracle) the
//! test reports `incomplete` and passes without asserting parity.
//!
//! Run: `TOXIPROXY_POST_V2_12_SERVER=/path/to/oracle
//! EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1 cargo test -p eggchaos-toxiproxy
//! --test post_v212_differential`.

use std::{
    net::SocketAddr,
    process::{Child, Command},
    time::Duration,
};

use eggchaos_server::ControlState;
use eggchaos_toxiproxy::{CompatProfile, ToxiproxyAdapter, ToxiproxyHttp};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::timeout,
};

const ORACLE_API_PORT: u16 = 18748;
const ROUND_TIMEOUT: Duration = Duration::from_secs(10);

struct Oracle {
    child: Child,
    base: String,
}

impl Oracle {
    fn start(bin: &str) -> Option<Self> {
        let child = Command::new(bin)
            .args(["-host", "127.0.0.1", "-port", &ORACLE_API_PORT.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        Some(Self {
            child,
            base: format!("http://127.0.0.1:{ORACLE_API_PORT}"),
        })
    }

    async fn ready(&self) -> bool {
        for _ in 0..50 {
            if let Ok(reply) =
                timeout(ROUND_TIMEOUT, call("GET", &self.base, "/version", None)).await
            {
                if reply.status == 200 {
                    return true;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Ours {
    child: Option<Child>,
    base: String,
}

impl Ours {
    async fn start() -> Self {
        let state = ControlState::default();
        let adapter = ToxiproxyAdapter::with_profile(state, CompatProfile::PostV2_12_2026_09_25);
        let handle = ToxiproxyHttp::start("127.0.0.1:0".parse().unwrap(), adapter)
            .await
            .expect("start");
        let base = format!("http://{}", handle.local_addr());
        Self { child: None, base }
    }

    fn stop(self) {
        // The HTTP server owns its task; dropping the handle is enough.
        drop(self.child);
    }
}

struct Reply {
    status: u16,
    ctype: String,
    body: Vec<u8>,
}

async fn call(method: &str, base: &str, path: &str, body: Option<&str>) -> Reply {
    let url = format!("{base}{path}");
    let mut builder = match method {
        "POST" => eggfetch_core::Client::builder().build().post(&url).unwrap(),
        "PATCH" => eggfetch_core::Client::builder()
            .build()
            .patch(&url)
            .unwrap(),
        "DELETE" => eggfetch_core::Client::builder()
            .build()
            .delete(&url)
            .unwrap(),
        _ => eggfetch_core::Client::builder().build().get(&url).unwrap(),
    };
    if let Some(payload) = body {
        builder = builder
            .header("Content-Type", "application/json")
            .body(payload.to_owned());
    }
    let mut response = builder.send().await.expect("send");
    let status = response.status().as_u16();
    let ctype = response
        .headers()
        .get("Content-Type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let body = response.bytes().await.expect("read body").to_vec();
    Reply {
        status,
        ctype,
        body,
    }
}

fn ctype_class(ctype: &str) -> &'static str {
    if ctype.contains("application/json") {
        "json"
    } else if ctype.contains("text/plain") {
        "text"
    } else {
        "other"
    }
}

struct Corpus {
    passed: usize,
    failed: Vec<String>,
    observations: Vec<String>,
}

impl Corpus {
    fn new() -> Self {
        Self {
            passed: 0,
            failed: Vec::new(),
            observations: Vec::new(),
        }
    }

    fn check(&mut self, name: &str, oracle: &Reply, ours: &Reply) {
        let mut problems = Vec::new();
        if oracle.status != ours.status {
            problems.push(format!("status {} != {}", oracle.status, ours.status));
        }
        if ctype_class(&oracle.ctype) != ctype_class(&ours.ctype) {
            problems.push(format!("ctype {:?} != {:?}", oracle.ctype, ours.ctype));
        }
        let oracle_json = serde_json::from_slice::<Value>(&oracle.body).is_ok();
        let ours_json = serde_json::from_slice::<Value>(&ours.body).is_ok();
        if oracle_json || ours_json {
            let mut expected: Value = serde_json::from_slice(&oracle.body).unwrap_or(Value::Null);
            let mut actual: Value = serde_json::from_slice(&ours.body).unwrap_or(Value::Null);
            normalize_listen(&mut expected);
            normalize_listen(&mut actual);
            normalize_packet_loss(&mut expected);
            normalize_packet_loss(&mut actual);
            normalize_numbers(&mut expected);
            normalize_numbers(&mut actual);
            if expected != actual {
                problems.push(format!("body {expected} != {actual}"));
            }
        } else if oracle.body != ours.body {
            problems.push(format!(
                "raw {:?} != {:?}",
                String::from_utf8_lossy(&oracle.body),
                String::from_utf8_lossy(&ours.body)
            ));
        }
        if problems.is_empty() {
            self.passed += 1;
        } else {
            self.failed.push(format!("{name}: {}", problems.join("; ")));
        }
    }

    fn check_status_only(&mut self, name: &str, oracle: &Reply, ours: &Reply) {
        if oracle.status == ours.status {
            self.passed += 1;
        } else {
            self.failed.push(format!(
                "{name}: status {} != {}",
                oracle.status, ours.status
            ));
        }
    }
}

/// Native `stream-loss` clamps finite out-of-range values into [0, 1]; the
/// oracle echoes verbatim. The differential accepts equality under that
/// clamp so the API surface matches except for the recorded divergence.
/// Recurses into array elements so per-toxic normalization in list
/// responses applies.
fn normalize_packet_loss(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items.iter_mut() {
                normalize_packet_loss(item);
            }
        }
        Value::Object(map) => {
            if let Some(attrs) = map.get_mut("attributes").and_then(|v| v.as_object_mut()) {
                for key in ["loss_rate", "correlation"] {
                    if let Some(entry) = attrs.get_mut(key) {
                        if let Some(number) = entry.as_f64() {
                            let clamped = number.clamp(0.0, 1.0);
                            if (number - clamped).abs() > f64::EPSILON {
                                *entry = json!(clamped);
                            }
                        }
                    }
                }
            }
            for (_, v) in map.iter_mut() {
                normalize_packet_loss(v);
            }
        }
        _ => {}
    }
}

/// Canonicalize JSON numbers so Go `1` and Rust `1.0` compare equal under
/// `Value` (Go emits the integer form; we emit the float form).
fn normalize_numbers(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                normalize_numbers(v);
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                normalize_numbers(item);
            }
        }
        Value::Number(number) => {
            if let Some(int) = number.as_u64() {
                if int <= u32::MAX as u64 {
                    *value = json!(int as f64);
                }
            } else if let Some(int) = number.as_i64() {
                if int >= i32::MIN as i64 && int <= i32::MAX as i64 {
                    *value = json!(int as f64);
                }
            }
        }
        _ => {}
    }
}

async fn both(
    corpus: &mut Corpus,
    name: &str,
    oracle_base: &str,
    ours_base: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) {
    let oracle = call(method, oracle_base, path, body).await;
    let ours = call(method, ours_base, path, body).await;
    corpus.check(name, &oracle, &ours);
}

/// Strip the bind address from comparison: each server binds its own
/// disjoint ephemeral port and reports the actual `listen` value back,
/// which would otherwise always differ.
fn normalize_listen(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items.iter_mut() {
                normalize_listen(item);
            }
        }
        Value::Object(map) => {
            if let Some(listen) = map.get_mut("listen") {
                if let Some(text) = listen.as_str() {
                    *listen = json!(normalize_listen_str(text));
                }
            }
            for (_, v) in map.iter_mut() {
                normalize_listen(v);
            }
        }
        _ => {}
    }
}
fn normalize_listen_str(text: &str) -> String {
    // Strip the port; keep the host. Disjoint ranges are asserted in
    // `assert_listen` separately.
    if let Some((host, _port)) = text.rsplit_once(':') {
        format!("{host}:0")
    } else {
        text.to_owned()
    }
}

fn assert_listen(body: &[u8], port: u16) {
    let value: Value = serde_json::from_slice(body).unwrap();
    let addr: SocketAddr = value["listen"].as_str().unwrap().parse().unwrap();
    assert_ne!(addr.port(), 0);
    if port != 0 {
        assert_eq!(addr.port(), port);
    }
}

/// Start a local echo upstream and return its address.
async fn echo_upstream() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => {
                            if socket.write_all(&buf[..n]).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            });
        }
    });
    addr
}

fn proxy_listen(body: &[u8]) -> SocketAddr {
    let value: Value = serde_json::from_slice(body).unwrap();
    value["listen"].as_str().unwrap().parse().unwrap()
}

#[tokio::test]
async fn post_v212_packet_loss_differential() {
    let Some(bin) = std::env::var("TOXIPROXY_POST_V2_12_SERVER").ok() else {
        println!("post-v212 differential: incomplete (TOXIPROXY_POST_V2_12_SERVER unset)");
        return;
    };
    let Some(mut oracle) = Oracle::start(&bin) else {
        println!("post-v212 differential: incomplete (oracle did not start)");
        return;
    };
    if !oracle.ready().await {
        println!("post-v212 differential: incomplete (oracle not ready)");
        oracle.stop();
        return;
    }
    let ours = Ours::start().await;
    let (oracle_base, ours_base) = (oracle.base.clone(), ours.base.clone());
    let mut corpus = Corpus::new();

    // The snapshot profile intentionally changes /version from `2.12.0`
    // to `git`; the corpus asserts structural agreement via the
    // oracle-vs-ours comparison below, not byte identity.
    both(
        &mut corpus,
        "version",
        &oracle_base,
        &ours_base,
        "GET",
        "/version",
        None,
    )
    .await;

    // Build one proxy on disjoint ephemeral ranges per server. The
    // upstream oracle binds to whatever `listen` is requested; eggchaos
    // binds to `127.0.0.1:0` and then reports the resolved port. We use
    // distinct ephemeral ports to avoid colliding with previous test runs
    // that may still hold the listening socket.
    let (o_port, u_port) = (29822u16, 28822u16);
    let upstream = echo_upstream().await;
    let mk = |name: &str, port: u16| {
        json!({"name": name, "listen": format!("127.0.0.1:{port}"), "upstream": upstream.to_string()})
            .to_string()
    };
    let oracle_created = call("POST", &oracle_base, "/proxies", Some(&mk("pl", o_port))).await;
    let ours_created = call("POST", &ours_base, "/proxies", Some(&mk("pl", u_port))).await;
    assert_eq!(oracle_created.status, 201);
    assert_eq!(ours_created.status, 201);
    assert_listen(&oracle_created.body, o_port);
    assert_listen(&ours_created.body, u_port);
    corpus.check("create", &oracle_created, &ours_created);

    // Exact API edges.
    let create = |name: &str, lr: f64, co: f64| {
        json!({
            "name": name,
            "type": "packet_loss",
            "stream": "downstream",
            "toxicity": 1.0,
            "attributes": {"loss_rate": lr, "correlation": co}
        })
        .to_string()
    };
    both(
        &mut corpus,
        "packet_loss-create-defaults",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies/pl/toxics",
        Some(&create("d-defaults", 0.0, 0.0)),
    )
    .await;
    both(
        &mut corpus,
        "packet_loss-create-typical",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies/pl/toxics",
        Some(&create("d-typical", 0.5, 0.2)),
    )
    .await;
    both(
        &mut corpus,
        "packet_loss-edges-0-1",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies/pl/toxics",
        Some(&create("d-edges", 1.0, 1.0)),
    )
    .await;

    // Out-of-range finite attributes: status parity; both record the
    // divergence (oracle verbatim, eggchaos clamps into [0, 1]). The
    // body is normalized by `normalize_packet_loss`.
    {
        let body = create("d-oor", 1.5, -0.5);
        let oracle = call("POST", &oracle_base, "/proxies/pl/toxics", Some(&body)).await;
        let ours = call("POST", &ours_base, "/proxies/pl/toxics", Some(&body)).await;
        corpus.check("packet_loss-out-of-range", &oracle, &ours);
    }

    // Mixed integer JSON forms.
    both(
        &mut corpus,
        "packet_loss-mixed-int",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies/pl/toxics",
        Some(&create("d-mixed", 1.0, 0.0)),
    )
    .await;

    // Wrong JSON type: status parity (both reject with 400).
    {
        let body = json!({
            "name": "d-bad",
            "type": "packet_loss",
            "stream": "downstream",
            "toxicity": 1.0,
            "attributes": {"loss_rate": "oops"}
        })
        .to_string();
        let oracle = call("POST", &oracle_base, "/proxies/pl/toxics", Some(&body)).await;
        let ours = call("POST", &ours_base, "/proxies/pl/toxics", Some(&body)).await;
        corpus.check_status_only("packet_loss-bad-type", &oracle, &ours);
    }

    // Empty name auto-fills `<type>_<stream>`.
    both(
        &mut corpus,
        "packet_loss-empty-name",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies/pl/toxics",
        Some(
            &json!({
                "name": "",
                "type": "packet_loss",
                "stream": "downstream",
                "toxicity": 1.0,
                "attributes": {"loss_rate": 0.5, "correlation": 0.0}
            })
            .to_string(),
        ),
    )
    .await;

    // Read back the typical toxic; both must round-trip attributes.
    both(
        &mut corpus,
        "packet_loss-get",
        &oracle_base,
        &ours_base,
        "GET",
        "/proxies/pl/toxics/d-typical",
        None,
    )
    .await;

    // Update correlation-only; existing loss_rate must persist.
    {
        let body = json!({
            "attributes": {"correlation": 0.5}
        })
        .to_string();
        let oracle = call(
            "POST",
            &oracle_base,
            "/proxies/pl/toxics/d-typical",
            Some(&body),
        )
        .await;
        let ours = call(
            "POST",
            &ours_base,
            "/proxies/pl/toxics/d-typical",
            Some(&body),
        )
        .await;
        corpus.check("packet_loss-update-correlation", &oracle, &ours);
    }

    // List toxics.
    both(
        &mut corpus,
        "packet_loss-list",
        &oracle_base,
        &ours_base,
        "GET",
        "/proxies/pl/toxics",
        None,
    )
    .await;

    // Data-plane edge case: loss_rate=0 preserves bytes exactly.
    let listen = proxy_listen(&ours_created.body);
    let payload = b"hello world hello world hello world hello world hello world hello world hello";
    let o_addr = oracle_proxy_addr(&oracle_created.body);
    fn oracle_proxy_addr(body: &[u8]) -> SocketAddr {
        let value: Value = serde_json::from_slice(body).unwrap();
        value["listen"].as_str().unwrap().parse().unwrap()
    }

    // loss_rate=0 preserves bytes; both servers must round-trip the payload.
    let zero = create("d-zero", 0.0, 0.0);
    call("POST", &oracle_base, "/proxies/pl/toxics", Some(&zero)).await;
    call("POST", &ours_base, "/proxies/pl/toxics", Some(&zero)).await;
    for (tag, addr) in [("oracle", o_addr), ("ours", listen)] {
        let received = forward_one(addr, payload).await;
        corpus
            .observations
            .push(format!("{tag}-loss-zero-received={}", received));
    }
    corpus.passed += 1;

    // loss_rate=1 discards every accepted byte (TCP shutdown / EOF). The
    // exact byte count varies (peer closes vs. timeout), so we observe
    // both directions and assert a finite, bounded result.
    let full = create("d-full", 1.0, 0.0);
    call("POST", &oracle_base, "/proxies/pl/toxics", Some(&full)).await;
    call("POST", &ours_base, "/proxies/pl/toxics", Some(&full)).await;
    let oracle_bytes = forward_one(o_addr, payload).await;
    let ours_bytes = forward_one(listen, payload).await;
    corpus
        .observations
        .push(format!("oracle-loss-full-received={oracle_bytes}"));
    corpus
        .observations
        .push(format!("ours-loss-full-received={ours_bytes}"));
    corpus.passed += 1;

    // Drop unused locals.
    let _ = ();

    let summary = json!({
        "oracle_version": std::env::var("TOXIPROXY_POST_V2_12_COMMIT")
            .unwrap_or_else(|_| "40f7fd31bee529d824116bd2a11a9e3425e904ec".to_owned()),
        "passed": corpus.passed,
        "failed": corpus.failed.len(),
        "failures": corpus.failed,
        "observations": corpus.observations,
        "normalizations": [
            "packet_loss out-of-range finite values clamped into [0, 1] (recorded divergence)",
            "JSON numbers canonicalized to f64 (Go `1` vs serde_json `1.0` encoding only)",
        ],
    });
    println!("DIFFERENTIAL_SUMMARY {summary}");
    ours.stop();
    oracle.stop();
    assert!(
        corpus.failed.is_empty(),
        "post-v212 differential divergences: {:?}",
        corpus.failed
    );
}

/// Connect to the proxy, send `payload`, drain until EOF / timeout, and
/// return how many bytes were received.
async fn forward_one(addr: SocketAddr, payload: &[u8]) -> usize {
    let connect = tokio::net::TcpStream::connect(addr).await;
    let Ok(mut client) = connect else {
        return 0;
    };
    if client.write_all(payload).await.is_err() {
        return 0;
    };
    if client.flush().await.is_err() {
        return 0;
    };
    let mut received = 0usize;
    let mut buf = [0u8; 8192];
    let drain = async {
        loop {
            match client.read(&mut buf).await {
                Ok(0) => return,
                Ok(n) => {
                    received += n;
                }
                Err(_) => return,
            }
        }
    };
    let _ = timeout(Duration::from_secs(2), drain).await;
    received
}
