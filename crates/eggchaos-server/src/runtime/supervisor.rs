use super::*;

pub(super) async fn proxy_supervisor(
    listener: TcpListener,
    proxy_name: String,
    runtime: Arc<RuntimeInner>,
    cancel: CancellationToken,
    done: Arc<SupervisorDone>,
) {
    // Level-triggered completion even on panic: waiters join the flag.
    let _guard = DoneGuard(done.clone());
    let mut children = JoinSet::new();
    loop {
        // Re-read the entry every accept so updates apply to new
        // connections; exit when the proxy is gone or its scope cancelled.
        let entry = runtime.proxies.read().await.get(&proxy_name).cloned();
        let Some(entry) = entry else { break };
        if cancel.is_cancelled() || runtime.shutdown_token.is_cancelled() {
            break;
        }
        tokio::select! {
            () = cancel.cancelled() => break,
            () = runtime.shutdown_token.cancelled() => break,
            accepted = listener.accept() => {
                let Ok((client, peer)) = accepted else {
                    if cancel.is_cancelled() || runtime.shutdown_token.is_cancelled() {
                        break;
                    }
                    continue;
                };
                accept_connection(&runtime, &entry, client, peer, &mut children).await;
            }
        }
    }
    // Backstop: tokens already told well-behaved connections to exit;
    // abort stragglers, then purge accounting so no record or counter leaks.
    // The done-flag resolves only after the purge, so control operations
    // joining the flag observe settled state.
    children.shutdown().await;
    let outcome = if runtime.shutdown_initiated.load(Ordering::Acquire) {
        ConnectionOutcome::ServiceShutdown
    } else {
        ConnectionOutcome::ProxyRemoved
    };
    purge_proxy_connections(&runtime, &proxy_name, outcome).await;
    done.finish();
}

#[allow(clippy::too_many_lines)]
async fn accept_connection(
    runtime: &Arc<RuntimeInner>,
    entry: &ManagedProxy,
    client: TcpStream,
    peer: SocketAddr,
    children: &mut JoinSet<()>,
) {
    let current = runtime.active.fetch_add(1, Ordering::AcqRel) + 1;
    let current_proxy = entry.active.fetch_add(1, Ordering::AcqRel) + 1;
    let over_global = current > runtime.params.limits.global_connections;
    let over_proxy = entry
        .spec
        .max_connections
        .is_some_and(|limit| current_proxy > limit);
    if over_global || over_proxy {
        runtime.active.fetch_sub(1, Ordering::AcqRel);
        entry.active.fetch_sub(1, Ordering::AcqRel);
        runtime.metrics.rejected.fetch_add(1, Ordering::Relaxed);
        return;
    }
    runtime.metrics.accepted.fetch_add(1, Ordering::Relaxed);
    {
        let mut tables = runtime.metrics.tables.lock().expect("metrics lock");
        tables.record_accept(&entry.spec.name);
    }
    let id = runtime.next_conn_id.fetch_add(1, Ordering::AcqRel);
    let key = entry.ordinal.fetch_add(1, Ordering::AcqRel);
    let generation = runtime.generation.load(Ordering::Acquire);
    // Accept-time policy state comes from one atomic snapshot per
    // direction, so accepted generations and namespaces always describe
    // the plans the new streams will compile.
    let upstream_snapshot = entry.spec.upstream_policy.snapshot();
    let downstream_snapshot = entry.spec.downstream_policy.snapshot();
    let conn_token = entry.cancel.child_token();
    runtime.connections.write().await.insert(
        id,
        ConnectionSnapshot {
            id,
            proxy: entry.spec.name.clone(),
            ordinal: key,
            peer,
            upstream: entry.spec.upstream,
            state: ConnectionState::Connecting,
            connection_key: key,
            seed: runtime.params.seed ^ entry.spec.seed,
            generation,
            accepted_upstream_generation: upstream_snapshot.generation,
            accepted_downstream_generation: downstream_snapshot.generation,
            accepted_upstream_seed: upstream_snapshot.seed_namespace,
            accepted_downstream_seed: downstream_snapshot.seed_namespace,
            observed_upstream_generation: upstream_snapshot.generation,
            observed_downstream_generation: downstream_snapshot.generation,
            pending_upstream_generation: None,
            pending_downstream_generation: None,
            upstream_seed_namespace: upstream_snapshot.seed_namespace,
            downstream_seed_namespace: downstream_snapshot.seed_namespace,
            upstream_transitions: 0,
            downstream_transitions: 0,
            upstream_bytes: DirectionBytes::default(),
            downstream_bytes: DirectionBytes::default(),
            upstream_stream_loss_chunks_evaluated: 0,
            downstream_stream_loss_chunks_evaluated: 0,
            upstream_stream_loss_chunks_dropped: 0,
            downstream_stream_loss_chunks_dropped: 0,
            upstream_stream_loss_bytes_discarded: 0,
            downstream_stream_loss_bytes_discarded: 0,
            upstream_faults: Vec::new(),
            upstream_faults_truncated: false,
            downstream_faults: Vec::new(),
            downstream_faults_truncated: false,
            rng_version: RngVersion::V1,
        },
    );
    runtime
        .cancellations
        .write()
        .await
        .insert(id, conn_token.clone());
    let params = ProxyConnParams {
        name: entry.spec.name.clone(),
        upstream: entry.spec.upstream,
        connect_timeout: entry.spec.connect_timeout,
        upstream_policy: entry.spec.upstream_policy.clone(),
        downstream_policy: entry.spec.downstream_policy.clone(),
    };
    let runtime_child = runtime.clone();
    let proxy_cancel = entry.cancel.clone();
    let proxy_active = entry.active.clone();
    children.spawn(run_connection(
        client,
        id,
        key,
        params,
        runtime_child,
        conn_token,
        proxy_cancel,
        proxy_active,
        runtime.params.half_close,
        runtime.params.relay_buffer,
        runtime.params.term_grace,
    ));
}
