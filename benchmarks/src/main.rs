use std::{
    num::{NonZeroU64, NonZeroUsize},
    time::{Duration, Instant},
};

use eggchaos_core::{
    ChaosStream, Direction, FaultId, FaultKind, FaultPlan, FaultSpec, LatencyConfig, Probability,
};
use eggchaos_eggfetch::ChaosDialer;
use eggfetch_core::{DialTarget, Dialer};
use eggress_relay::{relay_with_options, RelayOptions};
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const BUFFER_SIZE: usize = 64 * 1024;

#[tokio::main]
async fn main() {
    let bytes = std::env::var("EGGCHAOS_BENCH_BYTES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8 * 1024 * 1024);
    let rounds = std::env::var("EGGCHAOS_BENCH_ROUNDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3);

    println!("{{\"bytes\":{bytes},\"rounds\":{rounds},\"cases\":[");
    let cases = [
        ("bare_eggress_relay", None),
        ("eggchaos_empty_plan", Some(FaultPlan::empty())),
        ("eggchaos_latency_1ms", Some(latency_plan())),
    ];
    for (index, (name, plan)) in cases.into_iter().enumerate() {
        let mut samples = Vec::with_capacity(rounds);
        for _ in 0..rounds {
            let elapsed = match plan.clone() {
                Some(plan) => run_chaos(bytes, plan).await,
                None => run_bare(bytes).await,
            };
            let elapsed = elapsed.expect("benchmark relay completed");
            samples.push(elapsed.as_secs_f64());
        }
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        let throughput = bytes as f64 / mean / (1024.0 * 1024.0);
        if index != 0 {
            println!(", ");
        }
        println!(
            "{{\"name\":\"{name}\",\"mean_seconds\":{mean:.6},\"throughput_mib_s\":{throughput:.3},\"samples_seconds\":{:?}}}",
            samples
        );
    }
    println!(", ");
    let mut samples = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        samples.push(
            run_eggfetch_adapter(bytes)
                .await
                .expect("Eggfetch adapter benchmark completed")
                .as_secs_f64(),
        );
    }
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    let throughput = bytes as f64 / mean / (1024.0 * 1024.0);
    println!(
        "{{\"name\":\"eggfetch_adapter_empty_policy\",\"mean_seconds\":{mean:.6},\"throughput_mib_s\":{throughput:.3},\"samples_seconds\":{:?}}}",
        samples
    );
    println!("]}} ");
}

fn latency_plan() -> FaultPlan {
    FaultPlan::new(vec![FaultSpec {
        id: FaultId::new("latency").expect("static fault id"),
        probability: Probability::new(1.0).expect("static probability"),
        kind: FaultKind::Latency(LatencyConfig {
            delay: Duration::from_millis(1),
            jitter: Duration::ZERO,
            max_buffer_bytes: NonZeroU64::new(BUFFER_SIZE as u64).expect("non-zero buffer"),
        }),
    }])
    .expect("static benchmark plan")
}

async fn run_bare(bytes: usize) -> std::io::Result<Duration> {
    let (mut client, relay_client) = duplex(BUFFER_SIZE);
    let (relay_server, mut server) = duplex(BUFFER_SIZE);
    let server_task = tokio::spawn(async move {
        let mut received = 0;
        let mut buffer = vec![0; BUFFER_SIZE];
        while received < bytes {
            let read = server.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            received += read;
        }
        server.shutdown().await?;
        Ok::<usize, std::io::Error>(received)
    });
    let relay_task = tokio::spawn(relay_with_options(
        relay_client,
        relay_server,
        RelayOptions::bounded(
            NonZeroUsize::new(BUFFER_SIZE).expect("non-zero buffer"),
            Duration::from_secs(1),
        ),
    ));
    let start = Instant::now();
    client.write_all(&vec![0xA5; bytes]).await?;
    client.shutdown().await?;
    assert_eq!(server_task.await.expect("server task")?, bytes);
    relay_task
        .await
        .expect("relay task")
        .map_err(|failure| std::io::Error::other(failure.to_string()))?;
    Ok(start.elapsed())
}

async fn run_chaos(bytes: usize, plan: FaultPlan) -> std::io::Result<Duration> {
    let (mut client, relay_client) = duplex(BUFFER_SIZE);
    let (relay_server, mut server) = duplex(BUFFER_SIZE);
    let upstream = ChaosStream::new(relay_client, plan, 7, "benchmark", 1, Direction::Upstream)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let downstream = ChaosStream::passthrough(relay_server, Direction::Downstream);
    let server_task = tokio::spawn(async move {
        let mut received = 0;
        let mut buffer = vec![0; BUFFER_SIZE];
        while received < bytes {
            let read = server.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            received += read;
        }
        server.shutdown().await?;
        Ok::<usize, std::io::Error>(received)
    });
    let relay_task = tokio::spawn(relay_with_options(
        upstream,
        downstream,
        RelayOptions::bounded(
            NonZeroUsize::new(BUFFER_SIZE).expect("non-zero buffer"),
            Duration::from_secs(1),
        ),
    ));
    let start = Instant::now();
    client.write_all(&vec![0xA5; bytes]).await?;
    client.shutdown().await?;
    assert_eq!(server_task.await.expect("server task")?, bytes);
    relay_task
        .await
        .expect("relay task")
        .map_err(|failure| std::io::Error::other(failure.to_string()))?;
    Ok(start.elapsed())
}

async fn run_eggfetch_adapter(bytes: usize) -> std::io::Result<Duration> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server_task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let mut received = 0;
        let mut buffer = vec![0; BUFFER_SIZE];
        while received < bytes {
            let read = stream.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            received += read;
        }
        Ok::<usize, std::io::Error>(received)
    });
    let dialer = ChaosDialer::new(7, "benchmark");
    let mut stream = dialer
        .dial(DialTarget::new("127.0.0.1", address.port()))
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let payload = vec![0xA5; bytes];
    let start = Instant::now();
    stream.write_all(&payload).await?;
    stream.shutdown().await?;
    assert_eq!(server_task.await.expect("server task")?, bytes);
    Ok(start.elapsed())
}
