use std::{
    io,
    num::{NonZeroU64, NonZeroUsize},
    sync::Arc,
    time::{Duration, Instant},
};

use eggchaos_core::{
    ChaosStream, Direction, FaultId, FaultKind, FaultPlan, FaultSpec, LatencyConfig, LivePolicy,
    Probability, StreamEvidence, StreamLossConfig,
};
use eggchaos_eggfetch::ChaosDialer;
use eggfetch_core::{DialTarget, Dialer};
use eggress_relay::{relay_with_options, RelayOptions};
use serde_json::{json, Value};
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const BUFFER_SIZE: usize = 64 * 1024;

/// What the case does to bytes accepted from the caller.
#[derive(Debug, Clone, Copy)]
enum StreamKind {
    /// Every accepted byte must be forwarded (preserving byte conservation).
    Preserving,
    /// Engine may discard some accepted bytes by design (stream-loss,
    /// blackhole-style composition).
    Destructive,
}

/// How the bytes are submitted to the chaos stream.
#[derive(Debug, Clone, Copy)]
enum StreamProfile {
    /// Existing 8 MiB single-shot workload.
    Large,
    /// Many small writes, every accepted byte owns a `poll_write`.
    Small,
    /// Vectored write of equal-size iovecs spanning the entire payload.
    Vectored,
}

/// Which public constructor built the stream.
#[derive(Debug, Clone, Copy)]
enum StreamConstructor {
    Static,
    Live,
}

#[derive(Debug, Clone, Copy)]
struct CaseSpec {
    kind: StreamKind,
    profile: StreamProfile,
    constructor: StreamConstructor,
}

impl CaseSpec {
    fn label(self) -> &'static str {
        match (self.constructor, self.profile) {
            (StreamConstructor::Static, StreamProfile::Large) => "static-large",
            (StreamConstructor::Static, StreamProfile::Small) => "static-small",
            (StreamConstructor::Static, StreamProfile::Vectored) => "static-vectored",
            (StreamConstructor::Live, StreamProfile::Large) => "live-large",
            (StreamConstructor::Live, StreamProfile::Small) => "live-small",
            (StreamConstructor::Live, StreamProfile::Vectored) => "live-vectored",
        }
    }
}

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
    let selected = std::env::var("EGGCHAOS_BENCH_CASE").ok();
    let provenance = bench_provenance();
    let provenance_json = serde_json::to_string(&provenance).expect("provenance serializes");

    println!(
        "{{\"bytes\":{bytes},\"rounds\":{rounds},\"platform\":\"{}\",\"arch\":\"{}\",\"rustc_version\":\"{}\",\"provenance\":{provenance_json},\"cases\":[",
        std::env::consts::OS,
        std::env::consts::ARCH,
        rustc_version_runtime(),
    );
    let mut first = true;
    for case in cases(bytes) {
        if selected.as_deref().is_some_and(|sel| sel != case.name) {
            continue;
        }
        if !first {
            println!(", ");
        }
        first = false;
        let payload = run_case(case, bytes)
            .await
            .expect("benchmark case completed");
        println!("{payload}");
    }
    if selected
        .as_deref()
        .is_none_or(|sel| sel == "eggfetch_adapter_empty_policy")
    {
        if !first {
            println!(", ");
        }
        let eggfetch = run_eggfetch_adapter(bytes)
            .await
            .expect("Eggfetch adapter benchmark completed");
        println!("{eggfetch}");
    }
    println!("]}}");
    if selected.is_none() {
        emit_probes(&provenance).await;
    }
}

/// Provenance embedded in stream case and probe reports (M047).
///
/// The canonical benchmark wrapper (`scripts/benchmark.sh`) collects one
/// provenance object via `scripts/bench_provenance.py` and supplies it
/// through `EGGCHAOS_BENCH_PROVENANCE_JSON` (either the bare object or the
/// collector's `{"provenance": {...}}` envelope) so both JSON documents
/// from one invocation carry identical provenance. A direct
/// `cargo run --manifest-path benchmarks/Cargo.toml` that bypasses the
/// wrapper emits an explicitly non-authoritative `unavailable` marker;
/// canonical qualification docs must use the wrapper.
fn bench_provenance() -> Value {
    let raw = std::env::var("EGGCHAOS_BENCH_PROVENANCE_JSON").unwrap_or_default();
    if !raw.trim().is_empty() {
        if let Ok(parsed) = serde_json::from_str::<Value>(&raw) {
            let inner = parsed.get("provenance").cloned().unwrap_or(parsed);
            if inner.get("schema").is_some()
                && inner.get("head_sha").is_some()
                && inner.get("worktree").is_some()
                && inner.get("authoritative").is_some()
            {
                return inner;
            }
        }
    }
    json!({
        "schema": 1,
        "head_sha": Value::Null,
        "worktree": "unknown",
        "authoritative": false,
        "source_fingerprint": Value::Null,
        "index_dirty": false,
        "tracked_dirty": false,
        "untracked_source": false,
        "git_describe": Value::Null,
        "collector": "unavailable",
    })
}

fn rustc_version_runtime() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

struct StreamCase {
    name: &'static str,
    plan: Option<FaultPlan>,
    spec: Option<CaseSpec>,
}

fn cases(_bytes: usize) -> Vec<StreamCase> {
    vec![
        StreamCase {
            name: "bare_eggress_relay",
            plan: None,
            spec: None,
        },
        StreamCase {
            name: "eggchaos_static_empty_plan",
            plan: Some(FaultPlan::empty()),
            spec: Some(CaseSpec {
                kind: StreamKind::Preserving,
                profile: StreamProfile::Large,
                constructor: StreamConstructor::Static,
            }),
        },
        StreamCase {
            name: "eggchaos_live_empty_plan",
            plan: Some(FaultPlan::empty()),
            spec: Some(CaseSpec {
                kind: StreamKind::Preserving,
                profile: StreamProfile::Large,
                constructor: StreamConstructor::Live,
            }),
        },
        StreamCase {
            name: "eggchaos_static_small_writes_empty_plan",
            plan: Some(FaultPlan::empty()),
            spec: Some(CaseSpec {
                kind: StreamKind::Preserving,
                profile: StreamProfile::Small,
                constructor: StreamConstructor::Static,
            }),
        },
        StreamCase {
            name: "eggchaos_live_small_writes_empty_plan",
            plan: Some(FaultPlan::empty()),
            spec: Some(CaseSpec {
                kind: StreamKind::Preserving,
                profile: StreamProfile::Small,
                constructor: StreamConstructor::Live,
            }),
        },
        StreamCase {
            name: "eggchaos_static_vectored_writes_empty_plan",
            plan: Some(FaultPlan::empty()),
            spec: Some(CaseSpec {
                kind: StreamKind::Preserving,
                profile: StreamProfile::Vectored,
                constructor: StreamConstructor::Static,
            }),
        },
        StreamCase {
            name: "eggchaos_live_vectored_writes_empty_plan",
            plan: Some(FaultPlan::empty()),
            spec: Some(CaseSpec {
                kind: StreamKind::Preserving,
                profile: StreamProfile::Vectored,
                constructor: StreamConstructor::Live,
            }),
        },
        StreamCase {
            name: "eggchaos_static_vectored_writes_preserving_plan",
            plan: Some(preserving_plan_4kb_max_buffer()),
            spec: Some(CaseSpec {
                kind: StreamKind::Preserving,
                profile: StreamProfile::Vectored,
                constructor: StreamConstructor::Static,
            }),
        },
        StreamCase {
            name: "eggchaos_static_latency_1ms",
            plan: Some(latency_plan(1)),
            spec: Some(CaseSpec {
                kind: StreamKind::Preserving,
                profile: StreamProfile::Large,
                constructor: StreamConstructor::Static,
            }),
        },
        StreamCase {
            name: "eggchaos_static_bandwidth_16mib_s",
            plan: Some(bandwidth_plan()),
            spec: Some(CaseSpec {
                kind: StreamKind::Preserving,
                profile: StreamProfile::Large,
                constructor: StreamConstructor::Static,
            }),
        },
        StreamCase {
            name: "eggchaos_static_slice_16k",
            plan: Some(slice_plan(Duration::ZERO)),
            spec: Some(CaseSpec {
                kind: StreamKind::Preserving,
                profile: StreamProfile::Large,
                constructor: StreamConstructor::Static,
            }),
        },
        StreamCase {
            name: "eggchaos_static_combined_latency_slice",
            plan: Some(combined_latency_slice_plan(Duration::from_millis(1))),
            spec: Some(CaseSpec {
                kind: StreamKind::Preserving,
                profile: StreamProfile::Large,
                constructor: StreamConstructor::Static,
            }),
        },
        StreamCase {
            name: "eggchaos_static_stream_loss_zero",
            plan: Some(stream_loss_plan(0.0, 0.0)),
            spec: Some(CaseSpec {
                kind: StreamKind::Preserving,
                profile: StreamProfile::Large,
                constructor: StreamConstructor::Static,
            }),
        },
        StreamCase {
            name: "eggchaos_static_stream_loss_mid",
            plan: Some(stream_loss_plan(0.3, 0.2)),
            spec: Some(CaseSpec {
                kind: StreamKind::Destructive,
                profile: StreamProfile::Large,
                constructor: StreamConstructor::Static,
            }),
        },
        StreamCase {
            name: "eggchaos_static_stream_loss_full",
            plan: Some(stream_loss_plan(1.0, 0.0)),
            spec: Some(CaseSpec {
                kind: StreamKind::Destructive,
                profile: StreamProfile::Large,
                constructor: StreamConstructor::Static,
            }),
        },
        StreamCase {
            name: "eggchaos_static_stream_loss_latency",
            plan: Some(stream_loss_latency_plan()),
            spec: Some(CaseSpec {
                kind: StreamKind::Destructive,
                profile: StreamProfile::Large,
                constructor: StreamConstructor::Static,
            }),
        },
    ]
}

fn latency_plan(delay_ms: u64) -> FaultPlan {
    FaultPlan::new(vec![FaultSpec {
        id: FaultId::new("latency").expect("static fault id"),
        probability: Probability::new(1.0).expect("static probability"),
        kind: FaultKind::Latency(LatencyConfig {
            delay: Duration::from_millis(delay_ms),
            jitter: Duration::ZERO,
            max_buffer_bytes: NonZeroU64::new(BUFFER_SIZE as u64).expect("non-zero buffer"),
        }),
    }])
    .expect("static benchmark plan")
}

fn bandwidth_plan() -> FaultPlan {
    FaultPlan::new(vec![FaultSpec {
        id: FaultId::new("bandwidth").expect("static fault id"),
        probability: Probability::new(1.0).expect("static probability"),
        kind: FaultKind::Bandwidth(eggchaos_core::BandwidthConfig {
            bytes_per_second: NonZeroU64::new(16 * 1024 * 1024).expect("non-zero rate"),
            burst_bytes: NonZeroU64::new(BUFFER_SIZE as u64).expect("non-zero burst"),
        }),
    }])
    .expect("static benchmark plan")
}

fn slice_plan(slice_delay: Duration) -> FaultPlan {
    FaultPlan::new(vec![FaultSpec {
        id: FaultId::new("slice").expect("static fault id"),
        probability: Probability::new(1.0).expect("static probability"),
        kind: FaultKind::Slice(eggchaos_core::SliceConfig {
            average_size: NonZeroU64::new(16 * 1024).expect("non-zero slice"),
            variation: 4 * 1024,
            delay: slice_delay,
        }),
    }])
    .expect("static benchmark plan")
}

fn combined_latency_slice_plan(latency_delay: Duration) -> FaultPlan {
    FaultPlan::new(vec![
        FaultSpec {
            id: FaultId::new("latency").expect("static fault id"),
            probability: Probability::new(1.0).expect("static probability"),
            kind: FaultKind::Latency(LatencyConfig {
                delay: latency_delay,
                jitter: Duration::ZERO,
                max_buffer_bytes: NonZeroU64::new(BUFFER_SIZE as u64).expect("non-zero buffer"),
            }),
        },
        FaultSpec {
            id: FaultId::new("slice").expect("static fault id"),
            probability: Probability::new(1.0).expect("static probability"),
            kind: FaultKind::Slice(eggchaos_core::SliceConfig {
                average_size: NonZeroU64::new(16 * 1024).expect("non-zero slice"),
                variation: 4 * 1024,
                delay: Duration::ZERO,
            }),
        },
    ])
    .expect("static benchmark plan")
}

fn stream_loss_plan(rate: f64, correlation: f64) -> FaultPlan {
    FaultPlan::new(vec![FaultSpec {
        id: FaultId::new("loss").expect("static fault id"),
        probability: Probability::new(1.0).expect("static probability"),
        kind: FaultKind::StreamLoss(StreamLossConfig {
            loss_rate: Probability::new(rate).expect("static rate"),
            correlation: Probability::new(correlation).expect("static correlation"),
        }),
    }])
    .expect("static benchmark plan")
}

fn stream_loss_latency_plan() -> FaultPlan {
    FaultPlan::new(vec![
        FaultSpec {
            id: FaultId::new("loss").expect("static fault id"),
            probability: Probability::new(1.0).expect("static probability"),
            kind: FaultKind::StreamLoss(StreamLossConfig {
                loss_rate: Probability::new(0.3).expect("static rate"),
                correlation: Probability::new(0.2).expect("static correlation"),
            }),
        },
        FaultSpec {
            id: FaultId::new("latency").expect("static fault id"),
            probability: Probability::new(1.0).expect("static probability"),
            kind: FaultKind::Latency(LatencyConfig {
                delay: Duration::from_millis(1),
                jitter: Duration::ZERO,
                max_buffer_bytes: NonZeroU64::new(BUFFER_SIZE as u64).expect("non-zero buffer"),
            }),
        },
    ])
    .expect("static benchmark plan")
}

/// A latency-only preserving plan whose `max_buffer_bytes` is intentionally
/// smaller than the iovec-prefix supplied by the vectored profile. Exercises
/// the bounded prefix optimization seam in `poll_write_vectored` without
/// changing fault semantics.
fn preserving_plan_4kb_max_buffer() -> FaultPlan {
    FaultPlan::new(vec![FaultSpec {
        id: FaultId::new("latency").expect("static fault id"),
        probability: Probability::new(1.0).expect("static probability"),
        kind: FaultKind::Latency(LatencyConfig {
            delay: Duration::ZERO,
            jitter: Duration::ZERO,
            max_buffer_bytes: NonZeroU64::new(4 * 1024).expect("non-zero buffer"),
        }),
    }])
    .expect("static benchmark plan")
}

async fn run_case(case: StreamCase, bytes: usize) -> io::Result<Value> {
    let rounds = std::env::var("EGGCHAOS_BENCH_ROUNDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3);
    let mut samples = Vec::with_capacity(rounds);
    let mut evidence_samples = Vec::with_capacity(rounds);
    let mut received_samples = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        let (elapsed, evidence, received) = match (case.plan.as_ref(), case.spec) {
            (None, _) => {
                let (elapsed, received) = run_bare(bytes).await?;
                (elapsed, None, received)
            }
            (Some(plan), Some(spec)) => {
                let (elapsed, evidence, received) =
                    run_chaos(bytes, plan, spec.kind, spec.profile, spec.constructor).await?;
                (elapsed, Some(evidence), received)
            }
            (Some(_), None) => {
                return Err(io::Error::other("chaos case must declare a StreamCaseSpec"));
            }
        };
        samples.push(elapsed.as_secs_f64());
        evidence_samples.push(evidence);
        received_samples.push(received);
    }
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    let throughput = bytes as f64 / mean / (1024.0 * 1024.0);
    let evidence = evidence_samples.last().cloned().flatten();
    let bytes_received_target = *received_samples.last().unwrap_or(&0);
    let mut payload = json!({
        "name": case.name,
        "mean_seconds": mean,
        "throughput_mib_s": throughput,
        "samples_seconds": samples,
        "input_bytes": bytes,
        "bytes_received_target": bytes_received_target,
        "stream_kind": case.spec.map(|s| match s.kind {
            StreamKind::Preserving => "preserving",
            StreamKind::Destructive => "destructive",
        }).unwrap_or("bare"),
        "profile": case.spec.map(|s| match s.profile {
            StreamProfile::Large => "large",
            StreamProfile::Small => "small",
            StreamProfile::Vectored => "vectored",
        }).unwrap_or("large"),
        "constructor": case.spec.map(|s| match s.constructor {
            StreamConstructor::Static => "static",
            StreamConstructor::Live => "live",
        }).unwrap_or("n/a"),
        "profile_label": case.spec.map(CaseSpec::label).unwrap_or("bare"),
    });
    if let Some(ev) = evidence {
        payload["bytes_accepted"] = json!(ev.bytes_accepted);
        payload["bytes_forwarded"] = json!(ev.bytes_forwarded);
        payload["bytes_discarded"] = json!(ev.bytes_discarded);
    }
    Ok(payload)
}

async fn run_bare(bytes: usize) -> io::Result<(Duration, usize)> {
    let (mut client, relay_client) = duplex(BUFFER_SIZE);
    let (relay_server, mut server) = duplex(BUFFER_SIZE);
    let server_task = tokio::spawn(async move {
        let mut received = 0usize;
        let mut buffer = vec![0u8; BUFFER_SIZE];
        loop {
            match server.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => received = received.saturating_add(read),
                Err(_) => break,
            }
        }
        received
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
    let received = server_task.await.expect("server task");
    relay_task
        .await
        .expect("relay task")
        .map_err(|failure| io::Error::other(failure.to_string()))?;
    Ok((start.elapsed(), received))
}

/// Run one chaos case.
///
/// Topology mirrors production: the `upstream` ChaosStream wraps the target
/// socket so the relay's "client" half (the app side) writes through the
/// engine and the relay forwards the engine's accepted bytes down to the
/// target. This is the only way the engine evidence is observable: with the
/// chaos stream on the read side the relay never calls its `poll_write` and
/// the `note_direct`/engine counters would stay at zero.
async fn run_chaos(
    bytes: usize,
    plan: &FaultPlan,
    kind: StreamKind,
    profile: StreamProfile,
    constructor: StreamConstructor,
) -> io::Result<(Duration, RunEvidence, usize)> {
    let plan = plan.clone();
    let (mut client, relay_client) = duplex(BUFFER_SIZE);
    let (relay_server, mut server) = duplex(BUFFER_SIZE);
    let upstream: ChaosStream<tokio::io::DuplexStream> = match constructor {
        StreamConstructor::Static => ChaosStream::new(
            relay_server,
            plan.clone(),
            7,
            "benchmark",
            1,
            Direction::Upstream,
        )
        .map_err(|error| io::Error::other(error.to_string()))?,
        StreamConstructor::Live => {
            let policy = LivePolicy::new(plan.clone(), 7);
            ChaosStream::new_live(relay_server, policy, "benchmark", 1, Direction::Upstream)
                .map_err(|error| io::Error::other(error.to_string()))?
        }
    };
    let upstream_evidence: Arc<StreamEvidence> = upstream.stream_evidence();
    let downstream = ChaosStream::passthrough(relay_client, Direction::Downstream);
    let server_task = tokio::spawn(async move {
        let mut received = 0usize;
        let mut buffer = vec![0u8; BUFFER_SIZE];
        loop {
            match server.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => received = received.saturating_add(read),
                Err(_) => break,
            }
        }
        received
    });
    let relay_task = tokio::spawn(relay_with_options(
        downstream,
        upstream,
        RelayOptions::bounded(
            NonZeroUsize::new(BUFFER_SIZE).expect("non-zero buffer"),
            Duration::from_secs(1),
        ),
    ));
    let start = Instant::now();
    match profile {
        StreamProfile::Large => {
            write_full_payload(&mut client, bytes).await?;
        }
        StreamProfile::Small => {
            write_small_writes(&mut client, bytes).await?;
        }
        StreamProfile::Vectored => {
            write_vectored(&mut client, bytes).await?;
        }
    }
    client.shutdown().await?;
    let received = server_task.await.expect("server task");
    relay_task
        .await
        .expect("relay task")
        .map_err(|failure| io::Error::other(failure.to_string()))?;

    let ev = upstream_evidence.byte_counts();
    let bytes_accepted = ev.0;
    let bytes_forwarded = ev.1;
    let bytes_discarded = ev.2;

    let inputs = RunEvidence {
        bytes_accepted,
        bytes_forwarded,
        bytes_discarded,
    };

    // Per WP1 invariants:
    //
    //   * Preserving cases must show every input byte accepted and forwarded,
    //     with zero discards and the target receives exactly `bytes`.
    //   * Destructive cases (stream loss with a positive loss rate) accept
    //     the full input and may discard any subset. After the relay drains
    //     upstream EOF the target receives exactly `bytes_forwarded`.
    match kind {
        StreamKind::Preserving => {
            assert_eq!(
                bytes_accepted, bytes as u64,
                "accepted bytes mismatch in {kind:?}",
            );
            assert_eq!(bytes_discarded, 0, "preserving case must not discard");
            assert_eq!(
                received as u64, bytes as u64,
                "preserving target byte conservation"
            );
        }
        StreamKind::Destructive => {
            assert_eq!(
                bytes_accepted, bytes as u64,
                "destructive case must accept input"
            );
            assert_eq!(
                bytes_forwarded + bytes_discarded,
                bytes_accepted,
                "forwarded + discarded must equal accepted"
            );
            assert_eq!(
                received as u64, bytes_forwarded,
                "target sees exactly what was forwarded to the inner stream"
            );
        }
    }

    Ok((start.elapsed(), inputs, received))
}

#[derive(Debug, Clone, Copy)]
struct RunEvidence {
    bytes_accepted: u64,
    bytes_forwarded: u64,
    bytes_discarded: u64,
}

async fn write_full_payload(client: &mut tokio::io::DuplexStream, bytes: usize) -> io::Result<()> {
    let payload = vec![0xA5u8; bytes];
    client.write_all(&payload).await?;
    Ok(())
}

async fn write_small_writes(client: &mut tokio::io::DuplexStream, bytes: usize) -> io::Result<()> {
    const CHUNK: usize = 1024;
    let chunk = vec![0xA5u8; CHUNK];
    let mut written = 0usize;
    while written < bytes {
        let remaining = bytes - written;
        let want = remaining.min(CHUNK);
        let mut submitted = 0usize;
        while submitted < want {
            let n = client.write(&chunk[..want - submitted]).await?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "small-write profile produced zero-length write",
                ));
            }
            submitted += n;
        }
        written += want;
    }
    Ok(())
}

async fn write_vectored(client: &mut tokio::io::DuplexStream, bytes: usize) -> io::Result<()> {
    const SEGMENT: usize = 4 * 1024;
    // Pre-fill a single contiguous buffer so every vectored slice points at
    // distinct, non-overlapping 4 KiB bytes. Distinct backing memory is the
    // property the chaos engine cares about; values are uniform anyway.
    let segment_block: Vec<u8> = vec![0xA5u8; bytes.max(SEGMENT)];
    if bytes == 0 {
        return Ok(());
    }
    let mut written = 0usize;
    while written < bytes {
        let remaining = bytes - written;
        let slices_in_batch = remaining.div_ceil(SEGMENT);
        let last_len = if remaining % SEGMENT == 0 {
            SEGMENT
        } else {
            remaining % SEGMENT
        };
        // Build slices with explicit (start, end) byte ranges inside the
        // pre-filled block. The actual buffers are kept alive in `ranges`
        // for the entire batch; short writes re-slice over the same memory.
        let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(slices_in_batch);
        let mut cursor = 0usize;
        for index in 0..slices_in_batch {
            let len = if index + 1 == slices_in_batch && last_len != SEGMENT {
                last_len
            } else {
                SEGMENT
            };
            ranges.push((cursor, cursor + len));
            cursor += len;
        }
        let batch_total: usize = ranges.iter().map(|(s, e)| e - s).sum();
        let mut submitted = 0usize;
        'short_write: loop {
            let mut bufs: Vec<std::io::IoSlice<'_>> = Vec::with_capacity(ranges.len());
            for (start, end) in &ranges {
                let consumed_in_range = submitted.saturating_sub(*start).min(*end - *start);
                let real_start = *start + consumed_in_range;
                if real_start < *end {
                    bufs.push(std::io::IoSlice::new(&segment_block[real_start..*end]));
                }
            }
            if bufs.is_empty() {
                break 'short_write;
            }
            let n = client.write_vectored(&bufs).await?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "vectored profile produced zero-length write",
                ));
            }
            submitted += n;
            if submitted >= batch_total {
                break;
            }
        }
        written += batch_total.min(remaining);
    }
    Ok(())
}

async fn run_eggfetch_adapter(bytes: usize) -> io::Result<Value> {
    let rounds = std::env::var("EGGCHAOS_BENCH_ROUNDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let bytes_per_round = bytes;
    let server_task = tokio::spawn(async move {
        let mut total_received = 0usize;
        for _ in 0..rounds {
            let (mut stream, _) = listener.accept().await?;
            let mut received = 0usize;
            let mut buffer = vec![0; BUFFER_SIZE];
            while received < bytes_per_round {
                let read = stream.read(&mut buffer).await?;
                if read == 0 {
                    break;
                }
                received = received.saturating_add(read);
            }
            total_received = total_received.saturating_add(received);
        }
        Ok::<usize, std::io::Error>(total_received)
    });
    let dialer = ChaosDialer::new(7, "benchmark");
    let mut samples = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        let mut stream = dialer
            .dial(DialTarget::new("127.0.0.1", address.port()))
            .await
            .map_err(|error| io::Error::other(error.to_string()))?;
        let payload = vec![0xA5; bytes];
        let start = Instant::now();
        stream.write_all(&payload).await?;
        stream.shutdown().await?;
        samples.push(start.elapsed().as_secs_f64());
    }
    let received = server_task.await.expect("server task")?;
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    let throughput = bytes as f64 / mean / (1024.0 * 1024.0);
    Ok(json!({
        "name": "eggfetch_adapter_empty_policy",
        "mean_seconds": mean,
        "throughput_mib_s": throughput,
        "samples_seconds": samples,
        "input_bytes": bytes,
        "bytes_received_target": received,
        "stream_kind": "preserving",
        "profile": "large",
        "constructor": "live",
        "profile_label": "live-large",
    }))
}

/// Emit benchmark-only probes that cannot be derived reliably from
/// end-to-end throughput numbers. Output goes to stderr as a separate JSON
/// document so the stdout `{"cases":[…]}` keeps its analyzer-friendly shape.
/// The `provenance` value must be the same object embedded in the stdout
/// case report from this invocation.
async fn emit_probes(provenance: &Value) {
    use std::fmt::Write as _;
    let provenance_json = serde_json::to_string(provenance).expect("provenance serializes");
    let mut buf = String::new();
    let _ = writeln!(
        buf,
        "{{\"format\":2,\"platform\":\"{}\",\"arch\":\"{}\",\"provenance\":{provenance_json},\"probes\":[",
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    let mut first = true;
    for probe in probes() {
        if !first {
            let _ = writeln!(buf, ",");
        }
        first = false;
        let _ = writeln!(buf, "{probe}");
    }
    let _ = writeln!(buf, "]}}");
    eprintln!("{buf}");
}

fn probes() -> Vec<serde_json::Value> {
    // Hoist the live policy out of the timing loop so the probe measures the
    // repeated-snapshot cost rather than the one-shot constructor.
    let live_empty = LivePolicy::new(FaultPlan::empty(), 7);
    let live_loss_zero = LivePolicy::new(stream_loss_plan(0.0, 0.0), 7);
    let live_4stage = LivePolicy::new(combined_latency_slice_plan(Duration::from_millis(1)), 7);
    vec![
        json!({
            "name": "live_policy_snapshot_load_full",
            "description": "Repeated LivePolicy::snapshot on a stable policy",
            "iterations": 200_000,
            "nanos_per_iter": measure_nanos_per_iter(200_000, || {
                let snap = live_empty.snapshot();
                std::hint::black_box(snap.generation);
            }),
        }),
        json!({
            "name": "live_policy_generation_only_path",
            "description": "Generation compare followed by full snapshot on a generation change",
            "iterations": 200_000,
            "nanos_per_iter": measure_nanos_per_iter(200_000, || {
                let snap = live_empty.snapshot();
                if snap.generation == 0 {
                    let _ = live_empty.snapshot();
                }
            }),
        }),
        json!({
            "name": "engine_build_stages_0",
            "iterations": 1000,
            "nanos_per_iter": measure_nanos_per_iter(1000, || {
                let _ = eggchaos_core::DirectionEngine::new(
                    FaultPlan::empty(),
                    1,
                    "probe",
                    1,
                    Direction::Upstream,
                );
            }),
        }),
        json!({
            "name": "engine_build_stages_4_latency",
            "iterations": 1000,
            "nanos_per_iter": measure_nanos_per_iter(1000, || {
                let _ = eggchaos_core::DirectionEngine::new(
                    combined_latency_slice_plan(Duration::from_millis(1)),
                    1,
                    "probe",
                    1,
                    Direction::Upstream,
                );
            }),
        }),
        json!({
            "name": "engine_build_preserving_8k",
            "iterations": 1000,
            "nanos_per_iter": measure_nanos_per_iter(1000, || {
                let _ = eggchaos_core::DirectionEngine::new(
                    preserving_plan_4kb_max_buffer(),
                    1,
                    "probe",
                    1,
                    Direction::Upstream,
                );
            }),
        }),
        json!({
            "name": "stream_loss_zero_engine_build",
            "iterations": 1024,
            "nanos_per_iter": measure_nanos_per_iter(1024, || {
                let snap = live_loss_zero.snapshot();
                let _ = eggchaos_core::DirectionEngine::new(
                    (*snap.plan).clone(),
                    snap.seed_namespace,
                    "probe",
                    1,
                    Direction::Upstream,
                );
            }),
        }),
        json!({
            "name": "stream_loss_full_engine_build",
            "iterations": 1024,
            "nanos_per_iter": measure_nanos_per_iter(1024, || {
                let snap = live_loss_zero.snapshot();
                let _ = eggchaos_core::DirectionEngine::new(
                    (*snap.plan).clone(),
                    snap.seed_namespace,
                    "probe",
                    1,
                    Direction::Upstream,
                );
            }),
        }),
        json!({
            "name": "live_4stage_engine_recompile",
            "description": "Snap a live 4-stage policy and recompile a DirectionEngine from it",
            "iterations": 1000,
            "nanos_per_iter": measure_nanos_per_iter(1000, || {
                let snap = live_4stage.snapshot();
                let _ = eggchaos_core::DirectionEngine::new(
                    (*snap.plan).clone(),
                    snap.seed_namespace,
                    "probe",
                    1,
                    Direction::Upstream,
                );
            }),
        }),
    ]
}

fn measure_nanos_per_iter(iterations: usize, mut op: impl FnMut()) -> f64 {
    let start = Instant::now();
    for _ in 0..iterations {
        op();
        std::hint::black_box(());
    }
    let elapsed_nanos = start.elapsed().as_nanos() as f64;
    elapsed_nanos / iterations as f64
}
