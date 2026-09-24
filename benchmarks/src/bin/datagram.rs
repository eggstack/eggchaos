use std::{
    net::SocketAddr,
    num::NonZeroU64,
    time::{Duration, Instant},
};

use eggchaos_core::{
    DatagramFaultKind, DatagramFaultSpec, DatagramLivePolicy, DatagramPlan, DatagramQueueLimits,
    FaultId, Probability,
};
use eggchaos_server::{DatagramProxySpec, DatagramRuntime, DatagramRuntimeLimits};
use tokio::{net::UdpSocket, task::JoinSet, time::timeout};

const PAYLOAD_BYTES: usize = 1200;
const CLIENTS: usize = 8;

#[derive(Debug)]
struct Sample {
    datagrams_per_second: f64,
    mib_per_second: f64,
    p50_micros: u64,
    p95_micros: u64,
    attempts: u64,
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
    let mut output = Vec::new();
    for round in 0..rounds {
        for (name, fault) in cases() {
            if selected_case
                .as_deref()
                .is_some_and(|selected| selected != name)
            {
                continue;
            }
            let sample = if name == "direct_udp_echo" {
                run_direct(count).await?
            } else if name == "multi_client_empty_plan" {
                run_multi_client(count).await?
            } else {
                run_proxy(count, fault).await?
            };
            output.push(serde_json::json!({
                "round": round,
                "name": name,
                "payload_bytes": PAYLOAD_BYTES,
                "datagrams": if name == "multi_client_empty_plan" { count.max(CLIENTS) / CLIENTS * CLIENTS } else { count },
                "clients": if name == "multi_client_empty_plan" { CLIENTS } else { 1 },
                "datagrams_per_second": sample.datagrams_per_second,
                "mib_per_second": sample.mib_per_second,
                "p50_micros": sample.p50_micros,
                "p95_micros": sample.p95_micros,
                "attempts": sample.attempts,
                "queue_high_water_datagrams": sample.high_water_datagrams,
                "queue_high_water_bytes": sample.high_water_bytes,
            }));
        }
    }
    println!(
        "{}",
        serde_json::json!({
            "format": 1,
            "platform": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "rounds": rounds,
            "samples": output,
        })
    );
    Ok(())
}

fn cases() -> Vec<(&'static str, Option<Vec<DatagramFaultSpec>>)> {
    vec![
        ("direct_udp_echo", None),
        ("fixed_target_empty_plan", Some(vec![])),
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

fn sample(start: Instant, mut latency_us: Vec<u64>, attempts: u64, emitted: usize) -> Sample {
    latency_us.sort_unstable();
    let elapsed = start.elapsed().as_secs_f64().max(f64::MIN_POSITIVE);
    let p = |q: usize| latency_us[((latency_us.len().saturating_sub(1)) * q) / 100];
    Sample {
        datagrams_per_second: emitted as f64 / elapsed,
        mib_per_second: emitted as f64 * PAYLOAD_BYTES as f64 / elapsed / (1024.0 * 1024.0),
        p50_micros: p(50),
        p95_micros: p(95),
        attempts,
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

async fn run_direct(count: usize) -> Result<Sample, Box<dyn std::error::Error>> {
    let (target, echo) = start_echo().await?;
    let client = UdpSocket::bind("127.0.0.1:0").await?;
    client.connect(target).await?;
    let (sample, _) = exchange(&client, count, 1, false).await?;
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
    let mut result = sample(start, std::mem::take(&mut latencies), attempts, emitted);
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
    Ok((sample(start, latencies, attempts, count), attempts))
}
