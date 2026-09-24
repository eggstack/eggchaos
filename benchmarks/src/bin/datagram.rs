use std::{
    collections::{HashSet, VecDeque},
    net::SocketAddr,
    num::NonZeroU64,
    time::{Duration, Instant},
};

use bytes::Bytes;
use eggchaos_core::{
    DatagramDirectionEngine, DatagramFaultKind, DatagramFaultSpec, DatagramLivePolicy,
    DatagramPlan, DatagramQueueLimits, Direction, FaultId, Probability, PublishedDatagramPolicy,
    RngVersion,
};
use eggchaos_server::{DatagramProxySpec, DatagramRuntime, DatagramRuntimeLimits};
use std::sync::Arc;
use tokio::{net::UdpSocket, task::JoinSet, time::timeout};

const PAYLOAD_BYTES: usize = 1200;
const CLIENTS: usize = 8;
const WINDOW_SIZE: usize = 32;

#[derive(Debug)]
struct Sample {
    datagrams_per_second: f64,
    mib_per_second: f64,
    p50_micros: u64,
    p95_micros: u64,
    attempts: u64,
    duplicates: u64,
    high_water_datagrams: u64,
    high_water_bytes: u64,
    latency_samples: Vec<u64>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count = std::env::var("EGGCHAOS_DATAGRAM_BENCH_DATAGRAMS")
        .ok()
        .and_then(|x| x.parse::<usize>().ok())
        .filter(|x| *x > 0)
        .unwrap_or(2_000);
    let rounds = std::env::var("EGGCHAOS_DATAGRAM_BENCH_ROUNDS")
        .ok()
        .and_then(|x| x.parse::<usize>().ok())
        .filter(|x| *x > 0)
        .unwrap_or(3);
    let selected_case = std::env::var("EGGCHAOS_DATAGRAM_BENCH_CASE").ok();
    let selected = |name: &str| {
        selected_case
            .as_deref()
            .is_none_or(|selected| selected == name)
    };
    let mut output = Vec::new();
    for round in 0..rounds {
        for (name, fault) in cases() {
            if !selected(name) {
                continue;
            }
            let (sample, mode, extra_datagrams) = if name == "direct_udp_echo" {
                (run_direct(count).await?, "sequential", count)
            } else if name == "bare_fixed_target_relay" {
                (run_bare(count).await?, "sequential", count)
            } else if name == "multi_client_empty_plan" {
                (
                    run_multi_client(count).await?,
                    "sequential",
                    count.max(CLIENTS) / CLIENTS * CLIENTS,
                )
            } else if name == "direct_windowed" {
                (run_direct_windowed(count).await?, "windowed", count)
            } else if name == "bare_windowed" {
                (run_bare_windowed(count).await?, "windowed", count)
            } else if name == "empty_plan_windowed" {
                (run_proxy_windowed(count).await?, "windowed", count)
            } else {
                (run_proxy(count, fault).await?, "sequential", count)
            };
            output.push(serde_json::json!({
                "round": round,
                "name": name,
                "mode": mode,
                "window": if mode == "windowed" { WINDOW_SIZE } else { 1 },
                "payload_bytes": PAYLOAD_BYTES,
                "datagrams": extra_datagrams,
                "clients": if name == "multi_client_empty_plan" { CLIENTS } else { 1 },
                "datagrams_per_second": sample.datagrams_per_second,
                "mib_per_second": sample.mib_per_second,
                "p50_micros": sample.p50_micros,
                "p95_micros": sample.p95_micros,
                "attempts": sample.attempts,
                "duplicates_observed": sample.duplicates,
                "queue_high_water_datagrams": sample.high_water_datagrams,
                "queue_high_water_bytes": sample.high_water_bytes,
            }));
        }
    }
    let scheduler = if selected_case
        .as_deref()
        .is_none_or(|s| s.starts_with("scheduler"))
    {
        scheduler_probes()
    } else {
        Vec::new()
    };
    println!(
        "{}",
        serde_json::json!({
            "format": 2,
            "platform": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "rounds": rounds,
            "payload_bytes": PAYLOAD_BYTES,
            "window_size": WINDOW_SIZE,
            "samples": output,
            "scheduler": scheduler,
        })
    );
    Ok(())
}

fn cases() -> Vec<(&'static str, Option<Vec<DatagramFaultSpec>>)> {
    vec![
        ("direct_udp_echo", None),
        ("bare_fixed_target_relay", None),
        ("fixed_target_empty_plan", Some(vec![])),
        ("direct_windowed", None),
        ("bare_windowed", None),
        ("empty_plan_windowed", Some(vec![])),
        (
            "delay_20us",
            Some(vec![fault(
                "delay",
                DatagramFaultKind::Delay {
                    delay: Duration::from_micros(20),
                    jitter: Duration::ZERO,
                },
            )]),
        ),
        (
            "loss_1pct",
            Some(vec![with_probability(
                fault("loss", DatagramFaultKind::Loss),
                0.01,
            )]),
        ),
        (
            "duplicate_one_copy",
            Some(vec![fault(
                "duplicate",
                DatagramFaultKind::Duplicate {
                    additional_copies: 1,
                },
            )]),
        ),
        (
            "reorder_20us",
            Some(vec![fault(
                "reorder",
                DatagramFaultKind::Reorder {
                    hold: Duration::from_micros(20),
                },
            )]),
        ),
        (
            "corrupt_one_byte",
            Some(vec![fault(
                "corrupt",
                DatagramFaultKind::PayloadCorrupt {
                    bytes: NonZeroU64::new(1).unwrap(),
                },
            )]),
        ),
        (
            "bandwidth_100mib_s",
            Some(vec![fault(
                "bandwidth",
                DatagramFaultKind::Bandwidth {
                    bytes_per_second: NonZeroU64::new(100 * 1024 * 1024).unwrap(),
                    burst_bytes: NonZeroU64::new(64 * 1024).unwrap(),
                },
            )]),
        ),
        (
            "combined_delay_corrupt_bandwidth",
            Some(vec![
                fault(
                    "delay",
                    DatagramFaultKind::Delay {
                        delay: Duration::from_micros(20),
                        jitter: Duration::ZERO,
                    },
                ),
                fault(
                    "corrupt",
                    DatagramFaultKind::PayloadCorrupt {
                        bytes: NonZeroU64::new(1).unwrap(),
                    },
                ),
                fault(
                    "bandwidth",
                    DatagramFaultKind::Bandwidth {
                        bytes_per_second: NonZeroU64::new(100 * 1024 * 1024).unwrap(),
                        burst_bytes: NonZeroU64::new(64 * 1024).unwrap(),
                    },
                ),
            ]),
        ),
        ("multi_client_empty_plan", Some(vec![])),
    ]
}

fn fault(id: &str, kind: DatagramFaultKind) -> DatagramFaultSpec {
    DatagramFaultSpec {
        id: FaultId::new(id).expect("static benchmark ID"),
        probability: Probability::new(1.0).expect("valid probability"),
        kind,
    }
}

fn with_probability(mut fault: DatagramFaultSpec, probability: f64) -> DatagramFaultSpec {
    fault.probability = Probability::new(probability).expect("valid probability");
    fault
}

fn queue_limits() -> DatagramQueueLimits {
    DatagramQueueLimits {
        max_queued_datagrams: NonZeroU64::new(4096).unwrap(),
        max_queued_bytes: NonZeroU64::new(8 * 1024 * 1024).unwrap(),
        max_datagram_bytes: NonZeroU64::new(65_507).unwrap(),
    }
}

fn sample(
    start: Instant,
    mut latency_us: Vec<u64>,
    attempts: u64,
    duplicates: u64,
    emitted: usize,
) -> Sample {
    latency_us.sort_unstable();
    let elapsed = start.elapsed().as_secs_f64().max(f64::MIN_POSITIVE);
    let p = |q: usize| {
        if latency_us.is_empty() {
            0
        } else {
            latency_us[((latency_us.len().saturating_sub(1)) * q) / 100]
        }
    };
    Sample {
        datagrams_per_second: emitted as f64 / elapsed,
        mib_per_second: emitted as f64 * PAYLOAD_BYTES as f64 / elapsed / (1024.0 * 1024.0),
        p50_micros: p(50),
        p95_micros: p(95),
        attempts,
        duplicates,
        high_water_datagrams: 0,
        high_water_bytes: 0,
        latency_samples: latency_us,
    }
}

async fn start_echo() -> Result<(SocketAddr, tokio::task::JoinHandle<()>), std::io::Error> {
    let socket = UdpSocket::bind("127.0.0.1:0").await?;
    let address = socket.local_addr()?;
    let task = tokio::spawn(async move {
        let mut buffer = vec![0u8; 65_536];
        while let Ok((size, peer)) = socket.recv_from(&mut buffer).await {
            let _ = socket.send_to(&buffer[..size], peer).await;
        }
    });
    Ok((address, task))
}

/// Benchmark-local bare fixed-target relay with the same client -> proxy ->
/// fixed target -> proxy -> client socket topology as the eggchaos UDP
/// runtime but no [`DatagramDirectionEngine`]. This isolates the unavoidable
/// extra socket-hop cost from chaos-engine overhead. It is benchmark-only and
/// must not become a second production runtime.
async fn start_bare_relay(
    target: SocketAddr,
) -> Result<(SocketAddr, tokio::task::JoinHandle<()>), std::io::Error> {
    let listener = UdpSocket::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = UdpSocket::bind("127.0.0.1:0").await?;
    upstream.connect(target).await?;
    let task = tokio::spawn(async move {
        let mut downstream_buffer = vec![0u8; 65_536];
        let mut upstream_buffer = vec![0u8; 65_536];
        let mut client: Option<SocketAddr> = None;
        loop {
            tokio::select! {
                received = listener.recv_from(&mut downstream_buffer) => {
                    if let Ok((size, peer)) = received {
                        client = Some(peer);
                        let _ = upstream.send(&downstream_buffer[..size]).await;
                    } else {
                        break;
                    }
                }
                received = upstream.recv(&mut upstream_buffer) => {
                    match received {
                        Ok(size) => {
                            if let Some(peer) = client {
                                let _ = listener.send_to(&upstream_buffer[..size], peer).await;
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });
    Ok((address, task))
}

async fn run_direct(count: usize) -> Result<Sample, Box<dyn std::error::Error>> {
    let (target, echo) = start_echo().await?;
    let client = UdpSocket::bind("127.0.0.1:0").await?;
    client.connect(target).await?;
    let (sample, _) = exchange(&client, count, 1, false).await?;
    echo.abort();
    Ok(sample)
}

async fn run_bare(count: usize) -> Result<Sample, Box<dyn std::error::Error>> {
    let (target, echo) = start_echo().await?;
    let (relay, relay_task) = start_bare_relay(target).await?;
    let client = UdpSocket::bind("127.0.0.1:0").await?;
    client.connect(relay).await?;
    let (sample, _) = exchange(&client, count, 1, false).await?;
    relay_task.abort();
    echo.abort();
    Ok(sample)
}

async fn run_direct_windowed(count: usize) -> Result<Sample, Box<dyn std::error::Error>> {
    let (target, echo) = start_echo().await?;
    let client = UdpSocket::bind("127.0.0.1:0").await?;
    client.connect(target).await?;
    let sample = exchange_windowed(&client, count).await?;
    echo.abort();
    Ok(sample)
}

async fn run_bare_windowed(count: usize) -> Result<Sample, Box<dyn std::error::Error>> {
    let (target, echo) = start_echo().await?;
    let (relay, relay_task) = start_bare_relay(target).await?;
    let client = UdpSocket::bind("127.0.0.1:0").await?;
    client.connect(relay).await?;
    let sample = exchange_windowed(&client, count).await?;
    relay_task.abort();
    echo.abort();
    Ok(sample)
}

async fn run_proxy_windowed(count: usize) -> Result<Sample, Box<dyn std::error::Error>> {
    let (target, echo) = start_echo().await?;
    // The windowed workload keeps WINDOW_SIZE datagrams outstanding, which
    // exceeds the default 16-deep per-association ingress queue. Use a deeper
    // benchmark-only ingress bound (production defaults are unchanged) so the
    // windowed budget measures engine/runtime forwarding cost rather than the
    // documented bounded-ingress drop/retry path.
    let runtime = DatagramRuntime::new(DatagramRuntimeLimits {
        ingress_per_association: 64,
        ..DatagramRuntimeLimits::default()
    })?;
    let mut proxy = DatagramProxySpec::new(
        "bench-windowed",
        "127.0.0.1:0".parse().unwrap(),
        target,
        queue_limits(),
    )?;
    proxy.seed = 0x23_0b_1;
    proxy.upstream_policy = DatagramLivePolicy::new(DatagramPlan::empty(), proxy.seed)?;
    proxy.downstream_policy = DatagramLivePolicy::new(DatagramPlan::empty(), proxy.seed)?;
    let view = runtime.create_proxy(proxy).await?;
    let client = UdpSocket::bind("127.0.0.1:0").await?;
    client.connect(view.bound_addr.unwrap()).await?;
    let mut sample = exchange_windowed(&client, count).await?;
    if let Some(snapshot) = runtime.associations().await.into_iter().next() {
        sample.high_water_datagrams = snapshot
            .upstream_evidence
            .high_water_datagrams
            .max(snapshot.downstream_evidence.high_water_datagrams);
        sample.high_water_bytes = snapshot
            .upstream_evidence
            .high_water_bytes
            .max(snapshot.downstream_evidence.high_water_bytes);
    }
    runtime.shutdown().await;
    echo.abort();
    Ok(sample)
}

async fn run_proxy(
    count: usize,
    faults: Option<Vec<DatagramFaultSpec>>,
) -> Result<Sample, Box<dyn std::error::Error>> {
    let (target, echo) = start_echo().await?;
    let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default())?;
    let mut proxy = DatagramProxySpec::new(
        "bench",
        "127.0.0.1:0".parse().unwrap(),
        target,
        queue_limits(),
    )?;
    proxy.seed = 0x23_0b_1;
    let plan = DatagramPlan::new(faults.unwrap_or_default())?;
    let duplicate_replies = plan
        .faults()
        .iter()
        .find_map(|fault| match &fault.kind {
            DatagramFaultKind::Duplicate { additional_copies } => {
                Some(usize::from(*additional_copies) + 1)
            }
            _ => None,
        })
        .unwrap_or(1);
    proxy.upstream_policy = DatagramLivePolicy::new(plan, proxy.seed)?;
    proxy.downstream_policy = DatagramLivePolicy::new(DatagramPlan::empty(), proxy.seed)?;
    let view = runtime.create_proxy(proxy).await?;
    let client = UdpSocket::bind("127.0.0.1:0").await?;
    client.connect(view.bound_addr.unwrap()).await?;
    let (mut sample, attempts) = exchange(&client, count, duplicate_replies, true).await?;
    sample.attempts = attempts;
    if let Some(snapshot) = runtime.associations().await.into_iter().next() {
        sample.high_water_datagrams = snapshot
            .upstream_evidence
            .high_water_datagrams
            .max(snapshot.downstream_evidence.high_water_datagrams);
        sample.high_water_bytes = snapshot
            .upstream_evidence
            .high_water_bytes
            .max(snapshot.downstream_evidence.high_water_bytes);
    }
    runtime.shutdown().await;
    echo.abort();
    Ok(sample)
}

async fn run_multi_client(count: usize) -> Result<Sample, Box<dyn std::error::Error>> {
    let (target, echo) = start_echo().await?;
    let runtime = DatagramRuntime::new(DatagramRuntimeLimits::default())?;
    let proxy = DatagramProxySpec::new(
        "bench-multi",
        "127.0.0.1:0".parse().unwrap(),
        target,
        queue_limits(),
    )?;
    let view = runtime.create_proxy(proxy).await?;
    let address = view.bound_addr.unwrap();
    let each = (count / CLIENTS).max(1);
    let start = Instant::now();
    let mut tasks = JoinSet::new();
    for _ in 0..CLIENTS {
        tasks.spawn(async move {
            let client = UdpSocket::bind("127.0.0.1:0").await?;
            client.connect(address).await?;
            let (sample, attempts) = exchange(&client, each, 1, true).await?;
            Ok::<_, std::io::Error>((sample, attempts))
        });
    }
    let mut latencies = Vec::with_capacity(each * CLIENTS);
    let mut emitted = 0;
    let mut attempts = 0;
    while let Some(joined) = tasks.join_next().await {
        let (sample, sent) = joined??;
        latencies.extend(sample.latency_samples);
        emitted += each;
        attempts += sent;
    }
    let elapsed = start.elapsed().as_secs_f64().max(f64::MIN_POSITIVE);
    let mut result = sample(start, std::mem::take(&mut latencies), attempts, 0, emitted);
    result.datagrams_per_second = emitted as f64 / elapsed;
    result.mib_per_second = emitted as f64 * PAYLOAD_BYTES as f64 / elapsed / (1024.0 * 1024.0);
    if let Some(snapshot) = runtime.associations().await.into_iter().next() {
        result.high_water_datagrams = snapshot
            .upstream_evidence
            .high_water_datagrams
            .max(snapshot.downstream_evidence.high_water_datagrams);
        result.high_water_bytes = snapshot
            .upstream_evidence
            .high_water_bytes
            .max(snapshot.downstream_evidence.high_water_bytes);
    }
    runtime.shutdown().await;
    echo.abort();
    Ok(result)
}

async fn exchange(
    client: &UdpSocket,
    count: usize,
    replies_per_datagram: usize,
    retry_loss: bool,
) -> Result<(Sample, u64), std::io::Error> {
    let mut buffer = vec![0u8; 65_536];
    let mut latencies = Vec::with_capacity(count);
    let mut attempts = 0;
    let start = Instant::now();
    for sequence in 0..count {
        let mut payload = vec![0x5a; PAYLOAD_BYTES];
        payload[..8].copy_from_slice(&(sequence as u64).to_be_bytes());
        let expected_replies = replies_per_datagram;
        let request_started = Instant::now();
        let mut replies = 0;
        let max_attempts = if retry_loss { 20 } else { 1 };
        for _ in 0..max_attempts {
            attempts += 1;
            client.send(&payload).await?;
            let until = Instant::now() + Duration::from_millis(100);
            while replies < expected_replies {
                match timeout(
                    until.saturating_duration_since(Instant::now()),
                    client.recv(&mut buffer),
                )
                .await
                {
                    Ok(Ok(size)) if size == PAYLOAD_BYTES => replies += 1,
                    _ => break,
                }
            }
            if replies >= expected_replies {
                break;
            }
            if !retry_loss {
                return Err(std::io::Error::other(
                    "datagram benchmark request timed out",
                ));
            }
        }
        if replies == 0 {
            return Err(std::io::Error::other(format!(
                "loss benchmark exceeded retry bound for datagram {sequence}"
            )));
        }
        latencies.push(request_started.elapsed().as_micros().min(u64::MAX as u128) as u64);
    }
    Ok((sample(start, latencies, attempts, 0, count), attempts))
}

/// Windowed/saturated throughput: keep up to [`WINDOW_SIZE`] datagrams
/// outstanding, each carrying a deterministic sequence ID in its first eight
/// bytes. Replies are matched by sequence ID; already-completed IDs count as
/// duplicates (reported explicitly) rather than completions. The run is
/// bounded: unacknowledged datagrams are resent a bounded number of times and
/// the whole run fails after an overall deadline instead of hanging.
async fn exchange_windowed(client: &UdpSocket, count: usize) -> Result<Sample, std::io::Error> {
    const RESEND_TIMEOUT: Duration = Duration::from_millis(200);
    const MAX_RESENDS: u64 = 20;
    const RUN_DEADLINE: Duration = Duration::from_secs(120);
    let mut buffer = vec![0u8; 65_536];
    let mut pending: VecDeque<usize> = VecDeque::new();
    let mut sent = vec![false; count];
    let mut resends = vec![0u64; count];
    let mut completed = HashSet::with_capacity(count);
    let mut latencies = Vec::with_capacity(count);
    let mut sent_at = vec![Instant::now(); count];
    let mut attempts: u64 = 0;
    let mut duplicates: u64 = 0;
    let mut next_sequence = 0usize;
    let start = Instant::now();
    let deadline = start + RUN_DEADLINE;
    while completed.len() < count {
        if Instant::now() > deadline {
            return Err(std::io::Error::other(format!(
                "windowed run exceeded deadline with {}/{} completed",
                completed.len(),
                count
            )));
        }
        while pending.len() < WINDOW_SIZE && next_sequence < count {
            send_sequence(client, next_sequence).await?;
            sent[next_sequence] = true;
            sent_at[next_sequence] = Instant::now();
            pending.push_back(next_sequence);
            next_sequence += 1;
            attempts += 1;
        }
        let oldest_wait = pending
            .front()
            .map(|sequence| sent_at[*sequence] + RESEND_TIMEOUT)
            .unwrap_or_else(|| Instant::now() + RESEND_TIMEOUT);
        match timeout(
            oldest_wait.saturating_duration_since(Instant::now()),
            client.recv(&mut buffer),
        )
        .await
        {
            Ok(Ok(size)) if size >= 8 => {
                let sequence =
                    u64::from_be_bytes(buffer[..8].try_into().expect("eight bytes")) as usize;
                if sequence < count {
                    if completed.insert(sequence) {
                        latencies.push(
                            sent_at[sequence]
                                .elapsed()
                                .as_micros()
                                .min(u64::MAX as u128) as u64,
                        );
                        pending.retain(|pending| *pending != sequence);
                    } else {
                        duplicates += 1;
                    }
                } else {
                    duplicates += 1;
                }
            }
            _ => {
                let mut resent = false;
                if let Some(sequence) = pending.pop_front() {
                    resends[sequence] += 1;
                    if resends[sequence] > MAX_RESENDS {
                        return Err(std::io::Error::other(format!(
                            "windowed run exceeded resend bound for datagram {sequence}"
                        )));
                    }
                    send_sequence(client, sequence).await?;
                    sent_at[sequence] = Instant::now();
                    pending.push_back(sequence);
                    attempts += 1;
                    resent = true;
                }
                if !resent && next_sequence >= count && pending.is_empty() {
                    break;
                }
            }
        }
    }
    debug_assert!(sent.iter().all(|flag| *flag));
    Ok(sample(start, latencies, attempts, duplicates, count))
}

async fn send_sequence(client: &UdpSocket, sequence: usize) -> Result<(), std::io::Error> {
    let mut payload = vec![0x5a; PAYLOAD_BYTES];
    payload[..8].copy_from_slice(&(sequence as u64).to_be_bytes());
    client.send(&payload).await?;
    Ok(())
}

/// Core-only scheduler scaling probes at representative ready-queue depths.
/// No sockets are involved: a long-delay plan holds `depth` candidates while
/// `next_deadline` peeks are timed, then a zero-delay plan measures drain
/// behavior at the same depths. This separates engine scheduling cost from
/// socket and runtime overhead.
fn scheduler_probes() -> Vec<serde_json::Value> {
    const DEPTHS: [usize; 4] = [1, 32, 256, 1024];
    const PEEK_ITERS: usize = 2_000;
    const DRAIN_ROUNDS: usize = 5;
    let mut probes = Vec::new();
    for depth in DEPTHS {
        let limits = DatagramQueueLimits {
            max_queued_datagrams: NonZeroU64::new(4096).unwrap(),
            max_queued_bytes: NonZeroU64::new(64 * 1024 * 1024).unwrap(),
            max_datagram_bytes: NonZeroU64::new(65_535).unwrap(),
        };
        let hold = DatagramPlan::new(vec![DatagramFaultSpec {
            id: FaultId::new("hold").expect("static ID"),
            probability: Probability::new(1.0).expect("valid probability"),
            kind: DatagramFaultKind::Delay {
                delay: Duration::from_secs(60),
                jitter: Duration::ZERO,
            },
        }])
        .expect("valid hold plan");
        let hold_policy = Arc::new(PublishedDatagramPolicy {
            generation: 1,
            plan: Arc::new(hold),
            seed_namespace: 0x5eed,
        });
        let mut engine = DatagramDirectionEngine::new(
            limits,
            "scheduler-probe",
            1,
            Direction::Upstream,
            RngVersion::V1,
        )
        .expect("valid engine");
        let now = tokio::time::Instant::now();
        let admit_started = Instant::now();
        for _ in 0..depth {
            engine.admit(now, Bytes::from_static(b"probe-payload"), &hold_policy);
        }
        let admit_nanos = admit_started.elapsed().as_nanos() as f64 / depth as f64;
        assert_eq!(engine.evidence().queued_datagrams, depth as u64);
        let peek_started = Instant::now();
        for _ in 0..PEEK_ITERS {
            std::hint::black_box(engine.next_deadline());
        }
        let peek_nanos = peek_started.elapsed().as_nanos() as f64 / PEEK_ITERS as f64;
        let not_ready_started = Instant::now();
        for _ in 0..PEEK_ITERS {
            std::hint::black_box(engine.take_ready(now));
        }
        let not_ready_nanos = not_ready_started.elapsed().as_nanos() as f64 / PEEK_ITERS as f64;
        assert_eq!(engine.evidence().queued_datagrams, depth as u64);
        let mut drains = Vec::with_capacity(DRAIN_ROUNDS);
        for _ in 0..DRAIN_ROUNDS {
            let mut drain_engine = DatagramDirectionEngine::new(
                limits,
                "scheduler-probe",
                1,
                Direction::Upstream,
                RngVersion::V1,
            )
            .expect("valid engine");
            let ready_at = tokio::time::Instant::now();
            for _ in 0..depth {
                drain_engine.admit(ready_at, Bytes::from_static(b"probe-payload"), &hold_policy);
            }
            let target = ready_at + tokio::time::Duration::from_secs(61);
            let drain_started = Instant::now();
            let ready = drain_engine.take_ready(target);
            let elapsed = drain_started.elapsed();
            assert_eq!(ready.len(), depth);
            drains.push(elapsed.as_nanos() as f64 / depth as f64);
        }
        drains.sort_by(f64::total_cmp);
        probes.push(serde_json::json!({
            "depth": depth,
            "admit_ns_per_datagram": admit_nanos,
            "next_deadline_peek_ns": peek_nanos,
            "take_ready_not_ready_ns": not_ready_nanos,
            "take_ready_drain_ns_per_datagram": drains[DRAIN_ROUNDS / 2],
        }));
    }
    probes
}
