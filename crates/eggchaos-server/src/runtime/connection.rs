use super::*;

/// Remove exactly one connection record plus its cancellation token and
/// live evidence. Returns the snapshot, token, and evidence when this
/// caller won the removal; counters and history update only on `Some`,
/// so concurrent finish/purge paths cannot double-count or underflow.
pub(super) async fn take_connection(
    runtime: &RuntimeInner,
    id: u64,
) -> Option<(
    ConnectionSnapshot,
    Option<CancellationToken>,
    Option<ConnectionEvidence>,
)> {
    let snapshot = runtime.connections.write().await.remove(&id);
    let token = runtime.cancellations.write().await.remove(&id);
    let evidence = runtime.evidence.write().await.remove(&id);
    snapshot.map(|snapshot| (snapshot, token, evidence))
}

pub(super) async fn record_close(
    runtime: &RuntimeInner,
    id: u64,
    proxy_active: &Arc<AtomicUsize>,
    outcome: ConnectionOutcome,
    upstream_termination: Option<TerminationInfo>,
    downstream_termination: Option<TerminationInfo>,
    detail: Option<String>,
) {
    let Some((snapshot, _, evidence)) = take_connection(runtime, id).await else {
        return;
    };
    runtime.active.fetch_sub(1, Ordering::AcqRel);
    proxy_active.fetch_sub(1, Ordering::AcqRel);
    // Final evidence merges before metrics and history so both report the
    // streams' real closing state, including any drain-pending transition.
    let snapshot = merge_evidence(&snapshot, evidence.as_ref());
    aggregate_close_metrics(
        runtime,
        &snapshot,
        evidence.as_ref(),
        &outcome,
        upstream_termination
            .as_ref()
            .or(downstream_termination.as_ref()),
    );
    runtime.metrics.completed.fetch_add(1, Ordering::Relaxed);
    let mut snapshot = snapshot;
    snapshot.state = ConnectionState::Closed;
    if runtime.params.limits.history == 0 {
        return;
    }
    let mut history = runtime.history.write().await;
    while history.len() >= runtime.params.limits.history {
        history.pop_front();
    }
    history.push_back(ClosedConnection {
        snapshot,
        outcome,
        upstream_termination,
        downstream_termination,
        detail,
    });
}

/// Fold one closed connection into reconciling metrics. Totals are
/// derived from final stream evidence, so counters always agree with the
/// history records they summarize.
pub(super) fn aggregate_close_metrics(
    runtime: &RuntimeInner,
    snapshot: &ConnectionSnapshot,
    evidence: Option<&ConnectionEvidence>,
    outcome: &ConnectionOutcome,
    termination: Option<&TerminationInfo>,
) {
    runtime.metrics.outcomes[outcome_class(outcome)].fetch_add(1, Ordering::Relaxed);
    match termination.map(|info| info.request) {
        Some(TerminationRequest::Graceful) => {
            runtime
                .metrics
                .graceful_requests
                .fetch_add(1, Ordering::Relaxed);
        }
        Some(TerminationRequest::HardReset) => {
            runtime
                .metrics
                .hard_reset_requests
                .fetch_add(1, Ordering::Relaxed);
        }
        None => {}
    }
    if let ConnectionOutcome::HardReset { client, upstream } = outcome {
        for result in [client, upstream] {
            match result {
                ResetResult::Applied => {
                    runtime
                        .metrics
                        .reset_applied
                        .fetch_add(1, Ordering::Relaxed);
                }
                ResetResult::Unsupported(_) => {
                    runtime
                        .metrics
                        .reset_unsupported
                        .fetch_add(1, Ordering::Relaxed);
                }
                ResetResult::Failed(_) => {
                    runtime.metrics.reset_failed.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
    let mut up = [0; 7];
    let mut down = [0; 7];
    if let Some(record) = evidence {
        up = record.upstream.activations();
        down = record.downstream.activations();
        runtime.metrics.transitions.fetch_add(
            record.upstream.transitions() + record.downstream.transitions(),
            Ordering::Relaxed,
        );
    }
    runtime.metrics.bytes_accepted.fetch_add(
        snapshot.upstream_bytes.accepted + snapshot.downstream_bytes.accepted,
        Ordering::Relaxed,
    );
    runtime.metrics.bytes_forwarded.fetch_add(
        snapshot.upstream_bytes.forwarded + snapshot.downstream_bytes.forwarded,
        Ordering::Relaxed,
    );
    runtime.metrics.bytes_discarded.fetch_add(
        snapshot.upstream_bytes.discarded + snapshot.downstream_bytes.discarded,
        Ordering::Relaxed,
    );
    {
        let mut tables = runtime.metrics.tables.lock().expect("metrics lock");
        tables.record_complete(
            &snapshot.proxy,
            [
                snapshot.upstream_bytes.accepted,
                snapshot.upstream_bytes.forwarded,
                snapshot.upstream_bytes.discarded,
            ],
            [
                snapshot.downstream_bytes.accepted,
                snapshot.downstream_bytes.forwarded,
                snapshot.downstream_bytes.discarded,
            ],
        );
        tables.record_activations(&snapshot.proxy, "upstream", up);
        tables.record_activations(&snapshot.proxy, "downstream", down);
        tables.record_stream_loss(
            &snapshot.proxy,
            [
                snapshot.upstream_stream_loss_chunks_evaluated,
                snapshot.upstream_stream_loss_chunks_dropped,
                snapshot.upstream_stream_loss_bytes_discarded,
            ],
            [
                snapshot.downstream_stream_loss_chunks_evaluated,
                snapshot.downstream_stream_loss_chunks_dropped,
                snapshot.downstream_stream_loss_bytes_discarded,
            ],
        );
    }
}

/// Purge every remaining record for a proxy after its supervisor exits.
/// Aborted tasks never run their finalizer, so the purge owns their
/// accounting exactly once via `take_connection`, including bounded
/// history with the purge outcome.
pub(super) async fn purge_proxy_connections(
    runtime: &RuntimeInner,
    proxy_name: &str,
    outcome: ConnectionOutcome,
) {
    let ids: Vec<u64> = runtime
        .connections
        .read()
        .await
        .iter()
        .filter(|(_, snapshot)| snapshot.proxy == proxy_name)
        .map(|(id, _)| *id)
        .collect();
    if ids.is_empty() {
        return;
    }
    let proxy_active = runtime
        .proxies
        .read()
        .await
        .get(proxy_name)
        .map(|entry| entry.active.clone());
    for id in ids {
        let Some((snapshot, _, evidence)) = take_connection(runtime, id).await else {
            continue;
        };
        runtime.active.fetch_sub(1, Ordering::AcqRel);
        if let Some(active) = &proxy_active {
            active.fetch_sub(1, Ordering::AcqRel);
        }
        let snapshot = merge_evidence(&snapshot, evidence.as_ref());
        aggregate_close_metrics(runtime, &snapshot, evidence.as_ref(), &outcome, None);
        runtime.metrics.completed.fetch_add(1, Ordering::Relaxed);
        let mut snapshot = snapshot;
        snapshot.state = ConnectionState::Closed;
        if runtime.params.limits.history == 0 {
            continue;
        }
        let mut history = runtime.history.write().await;
        while history.len() >= runtime.params.limits.history {
            history.pop_front();
        }
        history.push_back(ClosedConnection {
            snapshot,
            outcome: outcome.clone(),
            upstream_termination: None,
            downstream_termination: None,
            detail: Some("supervisor purge after task abort".into()),
        });
    }
}

pub(super) fn is_eggchaos_termination(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::ConnectionAborted
        && error.to_string().starts_with("eggchaos stream terminated")
}

/// Human-readable stale-base conflict detail for scenario fail-fast.
pub(super) fn conflict_message(proxy: &str, conflict: PolicyConflict) -> String {
    format!(
        "proxy {proxy} policy moved from generation {} to {} during the operation; retry from current state",
        conflict.expected, conflict.found,
    )
}

/// Merge live stream evidence into a snapshot copy. Accept-time fields
/// are preserved; observed/pending generations, seed namespaces,
/// transitions, counters, and active fault lists come from the streams.
pub(super) fn merge_evidence(
    snapshot: &ConnectionSnapshot,
    evidence: Option<&ConnectionEvidence>,
) -> ConnectionSnapshot {
    let mut merged = snapshot.clone();
    let Some(record) = evidence else {
        return merged;
    };
    for (
        handle,
        observed,
        pending,
        namespace,
        transitions,
        bytes,
        faults,
        truncated,
        chunks_evaluated,
        chunks_dropped,
        bytes_discarded,
    ) in [
        (
            &record.upstream,
            &mut merged.observed_upstream_generation,
            &mut merged.pending_upstream_generation,
            &mut merged.upstream_seed_namespace,
            &mut merged.upstream_transitions,
            &mut merged.upstream_bytes,
            &mut merged.upstream_faults,
            &mut merged.upstream_faults_truncated,
            &mut merged.upstream_stream_loss_chunks_evaluated,
            &mut merged.upstream_stream_loss_chunks_dropped,
            &mut merged.upstream_stream_loss_bytes_discarded,
        ),
        (
            &record.downstream,
            &mut merged.observed_downstream_generation,
            &mut merged.pending_downstream_generation,
            &mut merged.downstream_seed_namespace,
            &mut merged.downstream_transitions,
            &mut merged.downstream_bytes,
            &mut merged.downstream_faults,
            &mut merged.downstream_faults_truncated,
            &mut merged.downstream_stream_loss_chunks_evaluated,
            &mut merged.downstream_stream_loss_chunks_dropped,
            &mut merged.downstream_stream_loss_bytes_discarded,
        ),
    ] {
        *observed = handle.observed_generation();
        let pending_value = handle.pending_generation();
        *pending = if pending_value == 0 {
            None
        } else {
            Some(pending_value)
        };
        *namespace = handle.seed_namespace();
        *transitions = handle.transitions();
        let (accepted, forwarded, discarded) = handle.byte_counts();
        *bytes = DirectionBytes {
            accepted,
            forwarded,
            discarded,
        };
        let (faults_live, truncated_live) = handle.active_faults();
        *faults = faults_live;
        *truncated = truncated_live;
        let snapshot = handle.snapshot(eggchaos_core::RngVersion::V1);
        *chunks_evaluated = snapshot.stream_loss_chunks_evaluated;
        *chunks_dropped = snapshot.stream_loss_chunks_dropped;
        *bytes_discarded = snapshot.stream_loss_bytes_discarded;
    }
    merged
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_connection(
    client: TcpStream,
    id: u64,
    key: u64,
    params: ProxyConnParams,
    runtime: Arc<RuntimeInner>,
    conn_token: CancellationToken,
    proxy_cancel: CancellationToken,
    proxy_active: Arc<AtomicUsize>,
    half_close: HalfClosePolicy,
    relay_buffer: NonZeroUsize,
    term_grace: Duration,
) {
    let classify_cancel = || {
        if runtime.shutdown_initiated.load(Ordering::Acquire) {
            ConnectionOutcome::ServiceShutdown
        } else if proxy_cancel.is_cancelled() {
            ConnectionOutcome::ProxyRemoved
        } else {
            ConnectionOutcome::KilledByOperator
        }
    };
    // Upstream dial with durable cancellation: a kill issued before the
    // relay begins waiting is still observed.
    let dial = TcpStream::connect(params.upstream);
    tokio::pin!(dial);
    let upstream = tokio::select! {
        () = conn_token.cancelled() => {
            let outcome = classify_cancel();
            record_close(&runtime, id, &proxy_active, outcome, None, None, Some("cancelled during upstream connect".into())).await;
            return;
        }
        result = timeout(params.connect_timeout, &mut dial) => {
            match result {
                Ok(Ok(stream)) => stream,
                Ok(Err(error)) => {
                    record_close(&runtime, id, &proxy_active, ConnectionOutcome::ConnectFailed(format!("dial: {error}")), None, None, None).await;
                    return;
                }
                Err(_) => {
                    record_close(&runtime, id, &proxy_active, ConnectionOutcome::ConnectFailed(format!("dial timed out after {:?}", params.connect_timeout)), None, None, None).await;
                    return;
                }
            }
        }
    };
    if let Some(snapshot) = runtime.connections.write().await.get_mut(&id) {
        snapshot.state = ConnectionState::Relaying;
    }
    let (client_wrapped, client_reset) = ResettableTcpStream::new(client);
    let (upstream_wrapped, upstream_reset) = ResettableTcpStream::new(upstream);
    // Engines compile from the accepted atomic snapshots, so the seed
    // namespaces and plans always agree with the accepted generation.
    let client_chaos = match ChaosStream::new_live(
        client_wrapped,
        params.downstream_policy.clone(),
        &params.name,
        key,
        Direction::Downstream,
    ) {
        Ok(stream) => stream,
        Err(error) => {
            record_close(
                &runtime,
                id,
                &proxy_active,
                ConnectionOutcome::RelayError(format!("downstream engine: {error}")),
                None,
                None,
                None,
            )
            .await;
            return;
        }
    };
    let upstream_chaos = match ChaosStream::new_live(
        upstream_wrapped,
        params.upstream_policy.clone(),
        &params.name,
        key,
        Direction::Upstream,
    ) {
        Ok(stream) => stream,
        Err(error) => {
            record_close(
                &runtime,
                id,
                &proxy_active,
                ConnectionOutcome::RelayError(format!("upstream engine: {error}")),
                None,
                None,
                None,
            )
            .await;
            return;
        }
    };
    let term_upstream = upstream_chaos.termination_handle();
    let term_downstream = client_chaos.termination_handle();
    // Register live evidence before relaying so snapshots report
    // observed generations, counters, and active faults from the start.
    runtime.evidence.write().await.insert(
        id,
        ConnectionEvidence {
            upstream: upstream_chaos.stream_evidence(),
            downstream: client_chaos.stream_evidence(),
        },
    );
    let options = RelayOptions {
        buffer_size: relay_buffer,
        half_close,
    };
    let mut relay_box = Box::pin(egress_relay::relay_with_options(
        client_chaos,
        upstream_chaos,
        options,
    ));
    let terms = || (term_upstream.get(), term_downstream.get());
    tokio::select! {
        biased;
        () = conn_token.cancelled() => {
            drop(relay_box);
            let outcome = classify_cancel();
            let (up_term, down_term) = terms();
            record_close(&runtime, id, &proxy_active, outcome, up_term, down_term, None).await;
        }
        relay_outcome = &mut relay_box => {
            let (up_term, down_term) = terms();
            match relay_outcome {
                Ok(report) => {
                    record_close(&runtime, id, &proxy_active, ConnectionOutcome::RelayCompleted, up_term, down_term, Some(format!("relay {} (+{}/{})", report.termination, report.bytes_upstream, report.bytes_downstream))).await;
                }
                Err(failure) => {
                    if is_eggchaos_termination(&failure.source) {
                        // The engine terminated the stream after its accepted
                        // prefix resolved; the durable handle names the mode.
                        let requested = up_term.as_ref().or(down_term.as_ref()).map(|info| info.request);
                        match requested {
                            Some(TerminationRequest::HardReset) => {
                                // Relay failed concurrently with a hard
                                // request but the sockets are already gone:
                                // no abortive close could be applied.
                                let unsupported = ResetResult::Unsupported("relay failed as the hard-reset request resolved; sockets already released".into());
                                record_close(&runtime, id, &proxy_active, ConnectionOutcome::HardReset { client: unsupported.clone(), upstream: unsupported }, up_term, down_term, Some(failure.to_string())).await;
                            }
                            _ => {
                                record_close(&runtime, id, &proxy_active, ConnectionOutcome::GracefulTermination { drained: true }, up_term, down_term, Some(failure.to_string())).await;
                            }
                        }
                    } else {
                        record_close(&runtime, id, &proxy_active, ConnectionOutcome::RelayError(failure.to_string()), up_term, down_term, None).await;
                    }
                }
            }
        }
        info = term_upstream.terminated() => {
            handle_termination(&runtime, id, &proxy_active, info, &client_reset, &upstream_reset, relay_box, term_grace, terms()).await;
        }
        info = term_downstream.terminated() => {
            handle_termination(&runtime, id, &proxy_active, info, &client_reset, &upstream_reset, relay_box, term_grace, terms()).await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_termination<F>(
    runtime: &Arc<RuntimeInner>,
    id: u64,
    proxy_active: &Arc<AtomicUsize>,
    info: TerminationInfo,
    client_reset: &Arc<TcpResetHandle>,
    upstream_reset: &Arc<TcpResetHandle>,
    relay_box: std::pin::Pin<Box<F>>,
    term_grace: Duration,
    terms: (Option<TerminationInfo>, Option<TerminationInfo>),
) where
    F: std::future::Future<Output = Result<egress_relay::RelayReport, egress_relay::RelayFailure>>,
{
    match info.request {
        TerminationRequest::Graceful => {
            // Let an active relay drain its accepted prefix; reap an idle
            // relay when the bounded grace expires.
            match timeout(term_grace, relay_box).await {
                Ok(Ok(report)) => {
                    record_close(
                        runtime,
                        id,
                        proxy_active,
                        ConnectionOutcome::GracefulTermination { drained: true },
                        terms.0,
                        terms.1,
                        Some(format!("relay {}", report.termination)),
                    )
                    .await;
                }
                Ok(Err(failure)) => {
                    if is_eggchaos_termination(&failure.source) {
                        record_close(
                            runtime,
                            id,
                            proxy_active,
                            ConnectionOutcome::GracefulTermination { drained: true },
                            terms.0,
                            terms.1,
                            Some(failure.to_string()),
                        )
                        .await;
                    } else {
                        record_close(
                            runtime,
                            id,
                            proxy_active,
                            ConnectionOutcome::RelayError(format!(
                                "graceful termination due, then relay failed: {failure}"
                            )),
                            terms.0,
                            terms.1,
                            None,
                        )
                        .await;
                    }
                }
                Err(_) => {
                    record_close(
                        runtime,
                        id,
                        proxy_active,
                        ConnectionOutcome::GracefulTermination { drained: false },
                        terms.0,
                        terms.1,
                        Some(format!(
                            "grace period of {term_grace:?} expired; relay aborted"
                        )),
                    )
                    .await;
                }
            }
        }
        TerminationRequest::HardReset => {
            // Abortive close: flag both sockets, drop the relay (whose
            // wrappers apply SO_LINGER=0 on release), then read truthful
            // per-socket outcomes.
            client_reset.request_reset();
            upstream_reset.request_reset();
            drop(relay_box);
            let client = client_reset.outcome().unwrap_or(ResetResult::Unsupported(
                "client socket outcome unavailable".into(),
            ));
            let upstream = upstream_reset.outcome().unwrap_or(ResetResult::Unsupported(
                "upstream socket outcome unavailable".into(),
            ));
            record_close(
                runtime,
                id,
                proxy_active,
                ConnectionOutcome::HardReset { client, upstream },
                terms.0,
                terms.1,
                Some(format!(
                    "hard reset requested by {}",
                    info.fault_id.as_deref().unwrap_or("unknown fault")
                )),
            )
            .await;
        }
    }
}
