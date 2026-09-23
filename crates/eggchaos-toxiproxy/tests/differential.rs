//! Oracle differential corpus (M012 WP5).
//!
//! Spins the pinned `toxiproxy-server` v2.12.0 oracle (via `TOXIPROXY_SERVER`)
//! next to the in-process eggchaos compatibility server and runs the same
//! API and data-plane sequences against both, comparing status codes and
//! parsed bodies. Without the env var (or with a version mismatch) the test
//! reports `incomplete` and passes without asserting parity.
//!
//! Run: `TOXIPROXY_SERVER=/tmp/oracle/toxiproxy-server cargo test -p
//! eggchaos-toxiproxy --test differential`.

use std::{
    net::SocketAddr,
    process::{Child, Command},
    time::Duration,
};

use eggchaos_server::ControlState;
use eggchaos_toxiproxy::{ToxiproxyAdapter, ToxiproxyHttp};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::timeout,
};

const ORACLE_API_PORT: u16 = 18747;
const ROUND_TIMEOUT: Duration = Duration::from_secs(10);

struct Oracle {
    child: Child,
    base: String,
}

impl Oracle {
    fn start(bin: &str) -> Option<Self> {
        let version = Command::new(bin).arg("-version").output().ok()?;
        let text = String::from_utf8_lossy(&version.stdout).into_owned()
            + &String::from_utf8_lossy(&version.stderr);
        if !text.contains("2.12.0") {
            println!("differential: oracle version mismatch, want 2.12.0, got {text:?}");
            return None;
        }
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
        let client = eggfetch_core::Client::builder().build();
        for _ in 0..100 {
            if let Ok(mut response) = client
                .get(&format!("{}/version", self.base))
                .unwrap()
                .send()
                .await
            {
                if response.status().as_u16() == 200 {
                    let _ = response.bytes().await;
                    return true;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Ours {
    handle: eggchaos_toxiproxy::ToxiproxyHttpHandle,
    base: String,
}

impl Ours {
    async fn start() -> Self {
        let adapter = ToxiproxyAdapter::new(ControlState::default());
        let handle = ToxiproxyHttp::start("127.0.0.1:0".parse().unwrap(), adapter)
            .await
            .unwrap();
        let base = format!("http://{}", handle.local_addr());
        Self { handle, base }
    }

    fn stop(self) {
        self.handle.shutdown();
    }
}

struct Reply {
    status: u16,
    body: Vec<u8>,
    ctype: String,
}

async fn call(method: &str, base: &str, path: &str, body: Option<&str>) -> Reply {
    let client = eggfetch_core::Client::builder().build();
    let url = format!("{base}{path}");
    let mut request = match method {
        "GET" => client.get(&url).unwrap(),
        "POST" => client.post(&url).unwrap(),
        "PATCH" => client.request(eggfetch_core::Method::PATCH, &url).unwrap(),
        "DELETE" => client.delete(&url).unwrap(),
        other => panic!("unsupported method {other}"),
    };
    if let Some(text) = body {
        request = request.body(text);
    }
    let mut response = timeout(ROUND_TIMEOUT, request.send())
        .await
        .expect("compat round timed out")
        .unwrap();
    let status = response.status().as_u16();
    let ctype = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let body = response.bytes().await.unwrap().to_vec();
    Reply {
        status,
        body,
        ctype,
    }
}

/// Replace concrete listener addresses with a placeholder: the two servers
/// must bind disjoint ports, so exact listen values cannot match. Concrete
/// bind behavior is asserted per server (see `assert_listen`).
fn normalize(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.contains_key("listen") {
                map.insert("listen".to_owned(), Value::String("LISTEN".into()));
            }
            for (_, child) in map.iter_mut() {
                normalize(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize(item);
            }
        }
        _ => {}
    }
}

/// Canonicalize every JSON number to its f64 value so encoding-only
/// differences (`1` from Go vs `1.0` from serde_json) do not fail
/// comparison. Numeric magnitudes still compare exactly.
fn normalize_numbers(value: &mut Value) {
    match value {
        Value::Number(number) => {
            if let Some(float) = number.as_f64() {
                *number = serde_json::Number::from_f64(float).unwrap_or_else(|| number.clone());
            }
        }
        Value::Object(map) => {
            for (_, child) in map.iter_mut() {
                normalize_numbers(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_numbers(item);
            }
        }
        _ => {}
    }
}

/// Declared normalizations for recorded intent-compatible divergences (see
/// `plans/reference/toxiproxy-parity.md`):
/// - `toxicity` is clamped into [0, 1] on both sides (the oracle stores
///   out-of-range values verbatim; exact clamped values are asserted in the
///   dedicated toxicity cases);
/// - degenerate zero numerics that native `NonZero` bounds cannot represent
///   coalesce to 1 on both sides (bandwidth `rate`, slicer `average_size`,
///   limit_data `bytes`).
fn normalize_recorded(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Number(toxicity)) = map.get("toxicity") {
                if let Some(float) = toxicity.as_f64() {
                    map.insert(
                        "toxicity".to_owned(),
                        serde_json::Number::from_f64(float.clamp(0.0, 1.0))
                            .map(Value::Number)
                            .unwrap_or(Value::Null),
                    );
                }
            }
            let kind = map
                .get("type")
                .and_then(|kind| kind.as_str())
                .unwrap_or("")
                .to_owned();
            if let Some(Value::Object(attrs)) = map.get_mut("attributes") {
                let coalesce = |attrs: &mut serde_json::Map<String, Value>, key: &str| {
                    if attrs.get(key).and_then(|v| v.as_f64()) == Some(0.0) {
                        attrs.insert(key.to_owned(), json!(1.0));
                    }
                };
                match kind.as_str() {
                    "bandwidth" => coalesce(attrs, "rate"),
                    "slicer" => coalesce(attrs, "average_size"),
                    "limit_data" => coalesce(attrs, "bytes"),
                    _ => {}
                }
            }
            for (_, child) in map.iter_mut() {
                normalize_recorded(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_recorded(item);
            }
        }
        _ => {}
    }
}

/// Assert a proxy body carries a usable bound listener on the expected port
/// (`0` means any nonzero ephemeral port).
fn assert_listen(body: &[u8], port: u16) {
    let value: Value = serde_json::from_slice(body).unwrap();
    let addr: SocketAddr = value["listen"].as_str().unwrap().parse().unwrap();
    if port == 0 {
        assert_ne!(addr.port(), 0);
    } else {
        assert_eq!(addr.port(), port);
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
            normalize(&mut expected);
            normalize(&mut actual);
            normalize_numbers(&mut expected);
            normalize_numbers(&mut actual);
            normalize_recorded(&mut expected);
            normalize_recorded(&mut actual);
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

async fn data_plane(corpus: &mut Corpus, oracle_base: &str, ours_base: &str) {
    // One echo upstream and one ephemeral proxy per server.
    let mut listens = Vec::new();
    for (tag, base) in [("oracle", oracle_base), ("ours", ours_base)] {
        let upstream = echo_upstream().await;
        let created = call(
            "POST",
            base,
            "/proxies",
            Some(
                &json!({"name": format!("dp-{tag}"), "upstream": upstream.to_string()}).to_string(),
            ),
        )
        .await;
        assert_eq!(created.status, 201, "{tag} data-plane proxy create");
        let listen = proxy_listen(&created.body);
        assert_ne!(listen.port(), 0, "{tag} port-0 bind resolves");
        listens.push((tag, base.to_owned(), listen));
    }

    // Latency preserves bytes and delays on both sides.
    for (tag, base, listen) in &listens {
        call(
            "POST",
            base,
            &format!("/proxies/dp-{tag}/toxics"),
            Some(
                &json!({"name":"lat","type":"latency","stream":"downstream","attributes":{"latency":200}})
                    .to_string(),
            ),
        )
        .await;
        let payload = vec![0xABu8; 4096];
        let mut socket = timeout(ROUND_TIMEOUT, tokio::net::TcpStream::connect(*listen))
            .await
            .unwrap()
            .unwrap();
        let start = std::time::Instant::now();
        socket.write_all(&payload).await.unwrap();
        let mut received = vec![0u8; 4096];
        timeout(ROUND_TIMEOUT, socket.read_exact(&mut received))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, payload, "{tag} latency byte preservation");
        assert!(
            start.elapsed() >= Duration::from_millis(150),
            "{tag} latency delay observed"
        );
    }
    corpus.passed += 1;
    for (tag, base, _) in &listens {
        call(
            "DELETE",
            base,
            &format!("/proxies/dp-{tag}/toxics/lat"),
            None,
        )
        .await;
    }

    // Sustained bandwidth pacing: compare byte preservation and a broad
    // throughput window, not scheduler-exact completion times. The oracle
    // and native output pipelines have different chunk schedulers, so their
    // measured times may differ by up to a factor of three.
    let mut bandwidth_seconds = Vec::new();
    for (tag, base, listen) in &listens {
        call(
            "POST",
            base,
            &format!("/proxies/dp-{tag}/toxics"),
            Some(
                &json!({"name":"bw","type":"bandwidth","stream":"downstream","attributes":{"rate":256}})
                    .to_string(),
            ),
        )
        .await;
        let payload = vec![0xB7u8; 1024 * 1024];
        let mut socket = timeout(ROUND_TIMEOUT, tokio::net::TcpStream::connect(*listen))
            .await
            .unwrap()
            .unwrap();
        let start = std::time::Instant::now();
        socket.write_all(&payload).await.unwrap();
        let mut received = vec![0u8; payload.len()];
        timeout(ROUND_TIMEOUT, socket.read_exact(&mut received))
            .await
            .unwrap()
            .unwrap();
        let elapsed = start.elapsed();
        assert_eq!(received, payload, "{tag} bandwidth byte preservation");
        assert!(
            (Duration::from_secs(1)..Duration::from_secs(10)).contains(&elapsed),
            "{tag} bandwidth sustained-rate window: {elapsed:?}"
        );
        bandwidth_seconds.push(elapsed.as_secs_f64());
        corpus
            .observations
            .push(format!("bandwidth-{tag}-1MiB={elapsed:?}"));
    }
    let bandwidth_ratio = bandwidth_seconds.iter().copied().fold(0.0, f64::max)
        / bandwidth_seconds
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
    assert!(
        bandwidth_ratio <= 3.0,
        "bandwidth timing ratio {bandwidth_ratio}"
    );
    corpus.passed += 1;
    for (tag, base, _) in &listens {
        call(
            "DELETE",
            base,
            &format!("/proxies/dp-{tag}/toxics/bw"),
            None,
        )
        .await;
    }

    // Slow-close delays graceful close after the echo bytes drain. Allow
    // 100 ms tolerance while checking both exact bytes and the close window.
    for (tag, base, listen) in &listens {
        call(
            "POST",
            base,
            &format!("/proxies/dp-{tag}/toxics"),
            Some(
                &json!({"name":"sc","type":"slow_close","stream":"downstream","attributes":{"delay":300}})
                    .to_string(),
            ),
        )
        .await;
        let payload = b"slow-close-evidence";
        let mut socket = timeout(ROUND_TIMEOUT, tokio::net::TcpStream::connect(*listen))
            .await
            .unwrap()
            .unwrap();
        socket.write_all(payload).await.unwrap();
        let mut echoed = vec![0u8; payload.len()];
        timeout(ROUND_TIMEOUT, socket.read_exact(&mut echoed))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(echoed, payload, "{tag} slow_close echo bytes");
        let start = std::time::Instant::now();
        socket.shutdown().await.unwrap();
        let mut received = Vec::new();
        timeout(ROUND_TIMEOUT, socket.read_to_end(&mut received))
            .await
            .unwrap()
            .unwrap();
        let elapsed = start.elapsed();
        assert!(received.is_empty(), "{tag} slow_close clean close tail");
        assert!(
            elapsed >= Duration::from_millis(200),
            "{tag} slow_close close delay: {elapsed:?}"
        );
        corpus
            .observations
            .push(format!("slow-close-{tag}={elapsed:?}"));
    }
    corpus.passed += 1;
    for (tag, base, _) in &listens {
        call(
            "DELETE",
            base,
            &format!("/proxies/dp-{tag}/toxics/sc"),
            None,
        )
        .await;
    }

    // Slicer compares preserved stream bytes and a tolerant observable
    // pacing floor; exact Go random chunk sequences are intentionally not
    // asserted against the deterministic native slicer.
    let mut slicer_seconds = Vec::new();
    for (tag, base, listen) in &listens {
        call(
            "POST",
            base,
            &format!("/proxies/dp-{tag}/toxics"),
            Some(
                &json!({"name":"sl","type":"slicer","stream":"downstream","attributes":{"average_size":256,"size_variation":0,"delay":1000}})
                    .to_string(),
            ),
        )
        .await;
        let payload = vec![0x5Cu8; 64 * 1024];
        let mut socket = timeout(ROUND_TIMEOUT, tokio::net::TcpStream::connect(*listen))
            .await
            .unwrap()
            .unwrap();
        let start = std::time::Instant::now();
        socket.write_all(&payload).await.unwrap();
        let mut received = vec![0u8; payload.len()];
        timeout(ROUND_TIMEOUT, socket.read_exact(&mut received))
            .await
            .unwrap()
            .unwrap();
        let elapsed = start.elapsed();
        assert_eq!(received, payload, "{tag} slicer byte preservation");
        assert!(
            elapsed >= Duration::from_millis(20),
            "{tag} slicer pacing floor: {elapsed:?}"
        );
        slicer_seconds.push(elapsed.as_secs_f64());
        corpus
            .observations
            .push(format!("slicer-{tag}-64KiB={elapsed:?}"));
    }
    let slicer_ratio = slicer_seconds.iter().copied().fold(0.0, f64::max)
        / slicer_seconds.iter().copied().fold(f64::INFINITY, f64::min);
    assert!(slicer_ratio <= 3.0, "slicer timing ratio {slicer_ratio}");
    corpus.passed += 1;
    for (tag, base, _) in &listens {
        call(
            "DELETE",
            base,
            &format!("/proxies/dp-{tag}/toxics/sl"),
            None,
        )
        .await;
    }

    // limit_data exact boundary on both sides.
    for (tag, base, listen) in &listens {
        call(
            "POST",
            base,
            &format!("/proxies/dp-{tag}/toxics"),
            Some(
                &json!({"name":"lim","type":"limit_data","stream":"downstream","attributes":{"bytes":100}})
                    .to_string(),
            ),
        )
        .await;
        let mut socket = timeout(ROUND_TIMEOUT, tokio::net::TcpStream::connect(*listen))
            .await
            .unwrap()
            .unwrap();
        socket.write_all(&vec![0xCDu8; 1000]).await.unwrap();
        let mut received = Vec::new();
        let _ = timeout(ROUND_TIMEOUT, socket.read_to_end(&mut received)).await;
        assert_eq!(received.len(), 100, "{tag} limit_data boundary");
        assert!(
            received.iter().all(|byte| *byte == 0xCD),
            "{tag} limit_data bytes"
        );
    }
    corpus.passed += 1;
    for (tag, base, _) in &listens {
        call(
            "DELETE",
            base,
            &format!("/proxies/dp-{tag}/toxics/lim"),
            None,
        )
        .await;
    }

    // timeout=0 blocks until the toxic is removed (fresh connection after).
    for (tag, base, listen) in &listens {
        call(
            "POST",
            base,
            &format!("/proxies/dp-{tag}/toxics"),
            Some(
                &json!({"name":"blk","type":"timeout","stream":"downstream","attributes":{"timeout":0}})
                    .to_string(),
            ),
        )
        .await;
        let mut socket = timeout(ROUND_TIMEOUT, tokio::net::TcpStream::connect(*listen))
            .await
            .unwrap()
            .unwrap();
        socket.write_all(b"hello").await.unwrap();
        let mut one = [0u8; 1];
        let blocked = timeout(Duration::from_millis(600), socket.read(&mut one)).await;
        assert!(blocked.is_err(), "{tag} timeout=0 blocks");
        call(
            "DELETE",
            base,
            &format!("/proxies/dp-{tag}/toxics/blk"),
            None,
        )
        .await;
        let mut fresh = timeout(ROUND_TIMEOUT, tokio::net::TcpStream::connect(*listen))
            .await
            .unwrap()
            .unwrap();
        fresh.write_all(b"hello").await.unwrap();
        let mut echo = [0u8; 5];
        timeout(ROUND_TIMEOUT, fresh.read_exact(&mut echo))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&echo, b"hello", "{tag} flows after toxic removal");
    }
    corpus.passed += 1;

    // reset_peer terminates the connection on both sides.
    for (tag, base, listen) in &listens {
        call(
            "POST",
            base,
            &format!("/proxies/dp-{tag}/toxics"),
            Some(
                &json!({"name":"rst","type":"reset_peer","stream":"downstream","attributes":{"timeout":0}})
                    .to_string(),
            ),
        )
        .await;
        let mut socket = timeout(ROUND_TIMEOUT, tokio::net::TcpStream::connect(*listen))
            .await
            .unwrap()
            .unwrap();
        socket.write_all(b"hello").await.unwrap();
        let mut received = Vec::new();
        let outcome = timeout(Duration::from_secs(5), socket.read_to_end(&mut received)).await;
        let terminated = match outcome {
            Err(_) => false,
            Ok(Err(_)) => true,
            Ok(Ok(_)) => true,
        };
        assert!(terminated, "{tag} reset_peer eventually terminates");
    }
    corpus.passed += 1;
}

#[tokio::test]
async fn toxiproxy_v212_differential() {
    let Some(bin) = std::env::var("TOXIPROXY_SERVER").ok() else {
        println!("differential: incomplete (TOXIPROXY_SERVER unset)");
        return;
    };
    let Some(oracle) = Oracle::start(&bin) else {
        println!("differential: incomplete (oracle did not start)");
        return;
    };
    if !oracle.ready().await {
        println!("differential: incomplete (oracle not ready)");
        oracle.stop();
        return;
    }
    let ours = Ours::start().await;
    let (oracle_base, ours_base) = (oracle.base.clone(), ours.base.clone());
    let mut corpus = Corpus::new();

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
    both(
        &mut corpus,
        "list-empty",
        &oracle_base,
        &ours_base,
        "GET",
        "/proxies",
        None,
    )
    .await;
    both(
        &mut corpus,
        "get-missing",
        &oracle_base,
        &ours_base,
        "GET",
        "/proxies/nope",
        None,
    )
    .await;
    both(
        &mut corpus,
        "delete-missing",
        &oracle_base,
        &ours_base,
        "DELETE",
        "/proxies/nope",
        None,
    )
    .await;
    // Malformed bodies differ in Go-specific text; status parity only.
    {
        let oracle = call("POST", &oracle_base, "/proxies", Some("{bad")).await;
        let ours = call("POST", &ours_base, "/proxies", Some("{bad")).await;
        corpus.check_status_only("malformed-status", &oracle, &ours);
    }
    both(
        &mut corpus,
        "create-missing-upstream",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies",
        Some(&json!({"name": "m"}).to_string()),
    )
    .await;

    // Fixed-port proxies on disjoint ranges per server; listen values are
    // normalized, with concrete binds asserted per server below.
    let (o_port, u_port) = (19801u16, 18801u16);
    let mk = |name: &str, port: u16| {
        json!({"name": name, "listen": format!("127.0.0.1:{port}"), "upstream": "127.0.0.1:1"})
            .to_string()
    };
    let oracle_created = call("POST", &oracle_base, "/proxies", Some(&mk("p1", o_port))).await;
    let ours_created = call("POST", &ours_base, "/proxies", Some(&mk("p1", u_port))).await;
    assert_eq!(oracle_created.status, 201);
    assert_eq!(ours_created.status, 201);
    assert_listen(&oracle_created.body, o_port);
    assert_listen(&ours_created.body, u_port);
    corpus.check("create", &oracle_created, &ours_created);
    both(
        &mut corpus,
        "create-dup",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies",
        Some(&mk("p1", o_port)),
    )
    .await;
    both(
        &mut corpus,
        "get",
        &oracle_base,
        &ours_base,
        "GET",
        "/proxies/p1",
        None,
    )
    .await;
    both(
        &mut corpus,
        "update-disable",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies/p1",
        Some(&json!({"enabled": false}).to_string()),
    )
    .await;
    both(
        &mut corpus,
        "update-enable",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies/p1",
        Some(&json!({"enabled": true}).to_string()),
    )
    .await;

    // Toxic defaults for all seven types, full attribute objects.
    for toxic in [
        "latency",
        "bandwidth",
        "slow_close",
        "timeout",
        "slicer",
        "limit_data",
        "reset_peer",
    ] {
        let body = json!({"name": format!("d-{toxic}"), "type": toxic, "stream": "downstream"})
            .to_string();
        both(
            &mut corpus,
            &format!("toxic-default-{toxic}"),
            &oracle_base,
            &ours_base,
            "POST",
            "/proxies/p1/toxics",
            Some(&body),
        )
        .await;
    }
    both(
        &mut corpus,
        "toxic-auto-name",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies/p1/toxics",
        Some(&json!({"type": "latency", "stream": "downstream"}).to_string()),
    )
    .await;
    both(
        &mut corpus,
        "toxic-list",
        &oracle_base,
        &ours_base,
        "GET",
        "/proxies/p1/toxics",
        None,
    )
    .await;
    both(
        &mut corpus,
        "toxic-get",
        &oracle_base,
        &ours_base,
        "GET",
        "/proxies/p1/toxics/d-latency",
        None,
    )
    .await;
    both(
        &mut corpus,
        "toxic-get-missing",
        &oracle_base,
        &ours_base,
        "GET",
        "/proxies/p1/toxics/nope",
        None,
    )
    .await;
    both(
        &mut corpus,
        "toxic-update-ignores-type-stream",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies/p1/toxics/d-latency",
        Some(
            &json!({"type": "bandwidth", "stream": "upstream", "attributes": {"rate": 5}})
                .to_string(),
        ),
    )
    .await;
    // PATCH update paths (the pinned Go client uses PATCH for toxic updates).
    both(
        &mut corpus,
        "patch-proxy-update",
        &oracle_base,
        &ours_base,
        "PATCH",
        "/proxies/p1",
        Some(&json!({"enabled": true}).to_string()),
    )
    .await;
    both(
        &mut corpus,
        "patch-toxic-update",
        &oracle_base,
        &ours_base,
        "PATCH",
        "/proxies/p1/toxics/d-latency",
        Some(&json!({"toxicity": 0.75}).to_string()),
    )
    .await;
    both(
        &mut corpus,
        "toxic-update-ignores-type-stream",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies/p1/toxics/d-latency",
        Some(
            &json!({"type": "bandwidth", "stream": "upstream", "attributes": {"rate": 5}})
                .to_string(),
        ),
    )
    .await;
    both(
        &mut corpus,
        "toxic-dup",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies/p1/toxics",
        Some(&json!({"name": "d-latency", "type": "latency", "stream": "downstream"}).to_string()),
    )
    .await;
    both(
        &mut corpus,
        "toxic-bad-type",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies/p1/toxics",
        Some(&json!({"name": "x", "type": "nope", "stream": "downstream"}).to_string()),
    )
    .await;
    both(
        &mut corpus,
        "toxic-bad-stream",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies/p1/toxics",
        Some(&json!({"name": "y", "type": "latency", "stream": "sideways"}).to_string()),
    )
    .await;
    both(
        &mut corpus,
        "toxic-missing-proxy",
        &oracle_base,
        &ours_base,
        "POST",
        "/proxies/nope/toxics",
        Some(&json!({"name": "z", "type": "latency", "stream": "downstream"}).to_string()),
    )
    .await;
    // Out-of-range toxicity: oracle echoes verbatim, we clamp; status parity
    // plus explicit clamp assertion.
    for toxicity in [2.5f64, -1.0] {
        let body = json!({"name": format!("t{toxicity}"), "type": "latency", "stream": "downstream", "toxicity": toxicity}).to_string();
        let oracle = call("POST", &oracle_base, "/proxies/p1/toxics", Some(&body)).await;
        let ours = call("POST", &ours_base, "/proxies/p1/toxics", Some(&body)).await;
        corpus.check_status_only(&format!("toxicity-{toxicity}-status"), &oracle, &ours);
        assert_eq!(ours.status, 200);
        let rendered: Value = serde_json::from_slice(&ours.body).unwrap();
        assert_eq!(rendered["toxicity"], json!(toxicity.clamp(0.0, 1.0)));
    }
    both(
        &mut corpus,
        "toxic-delete",
        &oracle_base,
        &ours_base,
        "DELETE",
        "/proxies/p1/toxics/d-bandwidth",
        None,
    )
    .await;
    both(
        &mut corpus,
        "toxic-delete-missing",
        &oracle_base,
        &ours_base,
        "DELETE",
        "/proxies/p1/toxics/nope",
        None,
    )
    .await;

    // Populate matrix.
    both(
        &mut corpus,
        "populate-empty",
        &oracle_base,
        &ours_base,
        "POST",
        "/populate",
        Some("[]"),
    )
    .await;
    both(
        &mut corpus,
        "populate-new",
        &oracle_base,
        &ours_base,
        "POST",
        "/populate",
        Some(
            &json!([{"name":"q1","listen":"127.0.0.1:0","upstream":"127.0.0.1:1","enabled":true}])
                .to_string(),
        ),
    )
    .await;
    both(
        &mut corpus,
        "populate-missing-name",
        &oracle_base,
        &ours_base,
        "POST",
        "/populate",
        Some(&json!([{"listen":"127.0.0.1:1","upstream":"127.0.0.1:2"}]).to_string()),
    )
    .await;
    // Keep-path uses per-server listen ports; listen values normalized.
    {
        let mk_keep = |port: u16| {
            json!([{"name":"p1","listen":format!("127.0.0.1:{port}"),"upstream":"127.0.0.1:1","enabled":false}])
                .to_string()
        };
        let oracle = call("POST", &oracle_base, "/populate", Some(&mk_keep(o_port))).await;
        let ours = call("POST", &ours_base, "/populate", Some(&mk_keep(u_port))).await;
        corpus.check("populate-keep", &oracle, &ours);
    }

    // Reset: disable, then reset and compare.
    call(
        "POST",
        &oracle_base,
        "/proxies/p1",
        Some(&json!({"enabled": false}).to_string()),
    )
    .await;
    call(
        "POST",
        &ours_base,
        "/proxies/p1",
        Some(&json!({"enabled": false}).to_string()),
    )
    .await;
    both(
        &mut corpus,
        "reset",
        &oracle_base,
        &ours_base,
        "POST",
        "/reset",
        None,
    )
    .await;
    both(
        &mut corpus,
        "get-after-reset",
        &oracle_base,
        &ours_base,
        "GET",
        "/proxies/p1",
        None,
    )
    .await;

    both(
        &mut corpus,
        "metrics-404",
        &oracle_base,
        &ours_base,
        "GET",
        "/metrics",
        None,
    )
    .await;
    both(
        &mut corpus,
        "unknown-route",
        &oracle_base,
        &ours_base,
        "GET",
        "/bogus",
        None,
    )
    .await;

    // Data-plane byte/timing behavior.
    data_plane(&mut corpus, &oracle_base, &ours_base).await;

    // Cleanup parity then final report.
    both(
        &mut corpus,
        "delete",
        &oracle_base,
        &ours_base,
        "DELETE",
        "/proxies/p1",
        None,
    )
    .await;

    let summary = json!({
        "oracle": "toxiproxy-server 2.12.0",
        "passed": corpus.passed,
        "failed": corpus.failed.len(),
        "failures": corpus.failed,
        "data_plane_observations": corpus.observations,
        "normalizations": [
            "listen addresses replaced (disjoint binds; concrete ports asserted per server)",
            "JSON numbers canonicalized to f64 (Go `1` vs serde_json `1.0` encoding only)",
            "toxicity clamped into [0,1] (recorded divergence; exact values asserted separately)",
            "degenerate zero rate/average_size/bytes coalesced to 1 (recorded divergence)",
        ],
    });
    println!("DIFFERENTIAL_SUMMARY {summary}");
    ours.stop();
    oracle.stop();
    assert!(
        corpus.failed.is_empty(),
        "differential divergences: {:?}",
        corpus.failed
    );
}
