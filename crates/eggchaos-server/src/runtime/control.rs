use super::*;

impl ControlState {
    /// Create state holding validated but not yet started definitions.
    /// Definitions report `running: false` until `start_all` or
    /// `create_proxy` binds them, so reads never claim a listener that
    /// does not exist.
    pub fn new(proxies: impl IntoIterator<Item = ProxySpec>) -> Self {
        let state = Self::default();
        if let Ok(mut map) = state.runtime.proxies.try_write() {
            for mut proxy in proxies {
                if map.contains_key(&proxy.name) {
                    continue;
                }
                proxy.init_policies(state.runtime.params.seed);
                map.insert(
                    proxy.name.clone(),
                    ManagedProxy {
                        spec: proxy,
                        running: false,
                        bound_addr: None,
                        cancel: state.runtime.shutdown_token.child_token(),
                        done: Arc::new(SupervisorDone::default()),
                        active: Arc::new(AtomicUsize::new(0)),
                        ordinal: Arc::new(AtomicU64::new(1)),
                    },
                );
            }
        }
        state
    }

    /// Create state with explicit runtime tuning.
    pub fn with_params(params: RuntimeParams) -> Self {
        Self {
            runtime: Arc::new(RuntimeInner::new(params)),
        }
    }

    /// Render low-cardinality Prometheus text. Labels are bounded by
    /// construction: proxy names come from a capped table, directions and
    /// fault types are fixed vocabularies, and no connection ID, peer
    /// address, scenario run, or arbitrary fault ID ever becomes a label.
    pub async fn metrics_text(&self) -> String {
        let mut text = format!(
            "# HELP eggchaos_config_generation Current configuration generation\n# TYPE eggchaos_config_generation gauge\neggchaos_config_generation {}\n# HELP eggchaos_connections_accepted_total Accepted connections\n# TYPE eggchaos_connections_accepted_total counter\neggchaos_connections_accepted_total {}\n# HELP eggchaos_connections_completed_total Completed connections\n# TYPE eggchaos_connections_completed_total counter\neggchaos_connections_completed_total {}\n# HELP eggchaos_connections_rejected_total Rejected connections\n# TYPE eggchaos_connections_rejected_total counter\neggchaos_connections_rejected_total {}\n# HELP eggchaos_connections_active Active connections\n# TYPE eggchaos_connections_active gauge\neggchaos_connections_active {}\n",
            self.generation(),
            self.runtime.metrics.accepted.load(Ordering::Relaxed),
            self.runtime.metrics.completed.load(Ordering::Relaxed),
            self.runtime.metrics.rejected.load(Ordering::Relaxed),
            self.runtime.active.load(Ordering::Relaxed),
        );
        for (index, name) in OUTCOME_CLASS_NAMES.iter().enumerate() {
            text.push_str(&format!(
                "eggchaos_connection_outcomes_total{{outcome=\"{name}\"}} {}\n",
                self.runtime.metrics.outcomes[index].load(Ordering::Relaxed),
            ));
        }
        text.push_str(&format!(
            "# HELP eggchaos_termination_requests_total Injected termination requests observed at close\n# TYPE eggchaos_termination_requests_total counter\neggchaos_termination_requests_total{{request=\"graceful\"}} {}\neggchaos_termination_requests_total{{request=\"hard_reset\"}} {}\n# HELP eggchaos_reset_results_total Abortive-close outcomes at the TCP edge\n# TYPE eggchaos_reset_results_total counter\neggchaos_reset_results_total{{result=\"applied\"}} {}\neggchaos_reset_results_total{{result=\"unsupported\"}} {}\neggchaos_reset_results_total{{result=\"failed\"}} {}\n# HELP eggchaos_bytes_total Bytes by flow across closed connections\n# TYPE eggchaos_bytes_total counter\neggchaos_bytes_total{{flow=\"accepted\"}} {}\neggchaos_bytes_total{{flow=\"forwarded\"}} {}\neggchaos_bytes_total{{flow=\"discarded\"}} {}\n# HELP eggchaos_policy_transitions_total Completed live-policy transitions\n# TYPE eggchaos_policy_transitions_total counter\neggchaos_policy_transitions_total {}\n",
            self.runtime.metrics.graceful_requests.load(Ordering::Relaxed),
            self.runtime.metrics.hard_reset_requests.load(Ordering::Relaxed),
            self.runtime.metrics.reset_applied.load(Ordering::Relaxed),
            self.runtime.metrics.reset_unsupported.load(Ordering::Relaxed),
            self.runtime.metrics.reset_failed.load(Ordering::Relaxed),
            self.runtime.metrics.bytes_accepted.load(Ordering::Relaxed),
            self.runtime.metrics.bytes_forwarded.load(Ordering::Relaxed),
            self.runtime.metrics.bytes_discarded.load(Ordering::Relaxed),
            self.runtime.metrics.transitions.load(Ordering::Relaxed),
        ));
        {
            let tables = self.runtime.metrics.tables.lock().expect("metrics lock");
            let mut proxies: Vec<_> = tables.proxies.iter().collect();
            proxies.sort_by_key(|(name, _)| *name);
            for (name, entry) in proxies {
                text.push_str(&format!(
                    "eggchaos_proxy_connections_accepted_total{{proxy=\"{name}\"}} {}\neggchaos_proxy_connections_completed_total{{proxy=\"{name}\"}} {}\n",
                    entry.accepted, entry.completed,
                ));
                for (direction, index) in [("upstream", 0), ("downstream", 1)] {
                    for (flow, value) in ["accepted", "forwarded", "discarded"]
                        .iter()
                        .zip(entry.bytes[index])
                    {
                        text.push_str(&format!(
                            "eggchaos_proxy_bytes_total{{proxy=\"{name}\",direction=\"{direction}\",flow=\"{flow}\"}} {value}\n",
                        ));
                    }
                }
            }
            if tables.overflow_proxy.accepted + tables.overflow_proxy.completed > 0 {
                text.push_str(&format!(
                    "eggchaos_proxy_connections_accepted_total{{proxy=\"_overflow\"}} {}\neggchaos_proxy_connections_completed_total{{proxy=\"_overflow\"}} {}\n",
                    tables.overflow_proxy.accepted, tables.overflow_proxy.completed,
                ));
            }
            let mut activations: Vec<_> = tables.activations.iter().collect();
            activations.sort();
            for ((proxy, direction, fault_type), count) in activations {
                text.push_str(&format!(
                    "eggchaos_fault_activations_total{{proxy=\"{proxy}\",direction=\"{direction}\",fault_type=\"{fault_type}\"}} {count}\n",
                ));
            }
            if tables.overflow_activations > 0 {
                text.push_str(&format!(
                    "eggchaos_fault_activations_total{{proxy=\"_overflow\",direction=\"_overflow\",fault_type=\"_overflow\"}} {}\n",
                    tables.overflow_activations,
                ));
            }
        }
        // Live gauges read directly from policies: per-proxy active
        // connections, queued bytes summed from live evidence, and current
        // policy generations.
        let map = self.runtime.proxies.read().await;
        let evidence = self.runtime.evidence.read().await;
        let connections = self.runtime.connections.read().await;
        let mut names: Vec<_> = map.keys().collect();
        names.sort();
        // Queue bytes per proxy+direction from live evidence handles.
        let mut queued: HashMap<(&str, &str), u64> = HashMap::new();
        for snapshot in connections.values() {
            let Some(record) = evidence.get(&snapshot.id) else {
                continue;
            };
            for (handle, direction) in [
                (&record.upstream, "upstream"),
                (&record.downstream, "downstream"),
            ] {
                let (accepted, forwarded, discarded) = handle.byte_counts();
                let current = accepted.saturating_sub(forwarded.saturating_add(discarded));
                let slot = queued
                    .entry((snapshot.proxy.as_str(), direction))
                    .or_default();
                *slot = slot.saturating_add(current);
            }
        }
        for name in names {
            let entry = &map[name];
            let upstream = entry.spec.upstream_policy.snapshot();
            let downstream = entry.spec.downstream_policy.snapshot();
            text.push_str(&format!(
                "eggchaos_proxy_connections_active{{proxy=\"{name}\"}} {}\neggchaos_proxy_policy_generation{{proxy=\"{name}\",direction=\"upstream\"}} {}\neggchaos_proxy_policy_generation{{proxy=\"{name}\",direction=\"downstream\"}} {}\neggchaos_proxy_queue_bytes{{proxy=\"{name}\",direction=\"upstream\"}} {}\neggchaos_proxy_queue_bytes{{proxy=\"{name}\",direction=\"downstream\"}} {}\n",
                entry.active.load(Ordering::Relaxed),
                upstream.generation,
                downstream.generation,
                queued.get(&(name.as_str(), "upstream")).copied().unwrap_or(0),
                queued.get(&(name.as_str(), "downstream")).copied().unwrap_or(0),
            ));
        }
        text
    }

    /// Current configuration generation.
    pub fn generation(&self) -> u64 {
        self.runtime.generation.load(Ordering::Acquire)
    }

    fn next_generation(&self) -> u64 {
        self.runtime.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Snapshot all proxy views with actual listener state.
    pub async fn list(&self) -> Vec<ProxyView> {
        self.runtime
            .proxies
            .read()
            .await
            .values()
            .map(RuntimeInner::view_of)
            .collect()
    }

    /// Get one proxy view.
    pub async fn get(&self, name: &str) -> Option<ProxyView> {
        self.runtime
            .proxies
            .read()
            .await
            .get(name)
            .map(RuntimeInner::view_of)
    }

    /// Snapshot active runtime connections, merging live stream evidence
    /// (observed/pending generations, counters, active faults) at read
    /// time. Connections whose evidence is not yet registered report
    /// accept-time values.
    pub async fn connections(&self) -> Vec<ConnectionSnapshot> {
        let map = self.runtime.connections.read().await;
        let evidence = self.runtime.evidence.read().await;
        map.values()
            .map(|snapshot| merge_evidence(snapshot, evidence.get(&snapshot.id)))
            .collect()
    }

    /// Snapshot one active connection with live evidence merged.
    pub async fn get_connection(&self, id: u64) -> Option<ConnectionSnapshot> {
        let map = self.runtime.connections.read().await;
        let snapshot = map.get(&id)?.clone();
        let evidence = self.runtime.evidence.read().await;
        Some(merge_evidence(&snapshot, evidence.get(&id)))
    }

    /// Snapshot bounded closed-connection history, newest last.
    pub async fn history(&self) -> Vec<ClosedConnection> {
        self.runtime.history.read().await.iter().cloned().collect()
    }

    /// Terminate an active connection, if present. Cancellation is
    /// level-triggered: a kill issued before the relay begins waiting is
    /// still observed. Returns true only when the ID was active.
    pub async fn kill(&self, id: u64) -> bool {
        let token = self.runtime.cancellations.write().await.remove(&id);
        if let Some(token) = token {
            token.cancel();
            true
        } else {
            false
        }
    }

    /// Store a validated definition without binding. Live policies are
    /// initialized from the configured plans so reads never diverge from
    /// what streams will compile. The proxy reports `running: false`
    /// until started.
    pub async fn import_definition(&self, mut proxy: ProxySpec) -> Result<u64, ControlError> {
        proxy.validate().map_err(|error| match error {
            EggchaosError::InvalidProxy(message) => ControlError::Invalid(message),
            EggchaosError::InvalidPlan(error) => {
                ControlError::Invalid(format!("invalid fault plan: {error}"))
            }
            other => ControlError::Invalid(other.to_string()),
        })?;
        proxy.init_policies(self.runtime.params.seed);
        let mut map = self.runtime.proxies.write().await;
        if map.contains_key(&proxy.name) {
            return Err(ControlError::Conflict(format!(
                "proxy {} already exists",
                proxy.name
            )));
        }
        map.insert(
            proxy.name.clone(),
            ManagedProxy {
                spec: proxy,
                running: false,
                bound_addr: None,
                cancel: self.runtime.shutdown_token.child_token(),
                done: Arc::new(SupervisorDone::default()),
                active: Arc::new(AtomicUsize::new(0)),
                ordinal: Arc::new(AtomicU64::new(1)),
            },
        );
        Ok(self.next_generation())
    }

    /// Bind and supervise every enabled definition that is not running.
    pub async fn start_all(&self) -> Result<u64, ControlError> {
        let names: Vec<String> = self
            .runtime
            .proxies
            .read()
            .await
            .iter()
            .filter(|(_, entry)| entry.spec.enabled && !entry.running)
            .map(|(name, _)| name.clone())
            .collect();
        for name in names {
            let spec = self
                .runtime
                .proxies
                .read()
                .await
                .get(&name)
                .map(|entry| entry.spec.clone())
                .ok_or_else(|| ControlError::NotFound(name.clone()))?;
            self.start_stored(&name, &spec).await?;
        }
        Ok(self.generation())
    }

    /// Validate, bind, publish initial policies, and supervise a proxy.
    /// The listener is bound before visible success; a bind failure leaves
    /// no registered proxy. Returns the operator view (including the actual
    /// bound address, resolving port 0) and the new generation.
    pub async fn create_proxy(
        &self,
        mut proxy: ProxySpec,
    ) -> Result<(ProxyView, u64), ControlError> {
        proxy.validate().map_err(|error| match error {
            EggchaosError::InvalidProxy(message) => ControlError::Invalid(message),
            EggchaosError::InvalidPlan(error) => {
                ControlError::Invalid(format!("invalid fault plan: {error}"))
            }
            other => ControlError::Invalid(other.to_string()),
        })?;
        {
            let map = self.runtime.proxies.read().await;
            if map.contains_key(&proxy.name) {
                return Err(ControlError::Conflict(format!(
                    "proxy {} already exists",
                    proxy.name
                )));
            }
        }
        let listener =
            TcpListener::bind(proxy.listen)
                .await
                .map_err(|source| ControlError::BindFailed {
                    proxy: proxy.name.clone(),
                    reason: source.to_string(),
                })?;
        let bound_addr = listener
            .local_addr()
            .map_err(|source| ControlError::BindFailed {
                proxy: proxy.name.clone(),
                reason: source.to_string(),
            })?;
        // Canonical plans and live policies start together at generation
        // one: the stored spec carries the same policy snapshots the
        // streams will compile from.
        proxy.init_policies(self.runtime.params.seed);
        proxy.enabled = true;
        let name = proxy.name.clone();
        let cancel = self.runtime.shutdown_token.child_token();
        let mut entry = ManagedProxy {
            spec: proxy,
            running: true,
            bound_addr: Some(bound_addr),
            cancel: cancel.clone(),
            done: Arc::new(SupervisorDone::default()),
            active: Arc::new(AtomicUsize::new(0)),
            ordinal: Arc::new(AtomicU64::new(1)),
        };
        let done = self
            .spawn_supervisor(listener, name.clone(), cancel.clone())
            .await;
        entry.done = done;
        {
            let mut map = self.runtime.proxies.write().await;
            if map.contains_key(&name) {
                // Lost a creation race after binding: stop the orphan
                // supervisor through its own scope and release the
                // listener rather than leaking an untracked acceptor.
                drop(map);
                cancel.cancel();
                entry.done.join().await;
                self.untrack(&name).await;
                return Err(ControlError::Conflict(format!(
                    "proxy {name} already exists"
                )));
            }
            map.insert(name.clone(), entry);
            let view = RuntimeInner::view_of(map.get(&name).expect("proxy was just inserted"));
            drop(map);
            Ok((view, self.next_generation()))
        }
    }

    /// Drop the finished tracked supervisor handle for a proxy. Only call
    /// after the supervisor's done-flag confirms completion, so a running
    /// task is never detached.
    async fn untrack(&self, name: &str) {
        let mut root = self.runtime.root.lock().await;
        if let Some(position) = root.iter().position(|(tracked, _)| tracked == name) {
            let (_, handle) = root.swap_remove(position);
            drop(handle);
        }
    }

    async fn start_stored(&self, name: &str, spec: &ProxySpec) -> Result<SocketAddr, ControlError> {
        let listener =
            TcpListener::bind(spec.listen)
                .await
                .map_err(|source| ControlError::BindFailed {
                    proxy: name.to_owned(),
                    reason: source.to_string(),
                })?;
        let bound_addr = listener
            .local_addr()
            .map_err(|source| ControlError::BindFailed {
                proxy: name.to_owned(),
                reason: source.to_string(),
            })?;
        let cancel = self.runtime.shutdown_token.child_token();
        let done = self
            .spawn_supervisor(listener, name.to_owned(), cancel.clone())
            .await;
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(name) else {
            // Proxy vanished while binding: stop the orphan supervisor.
            drop(map);
            cancel.cancel();
            done.join().await;
            self.untrack(name).await;
            return Err(ControlError::NotFound(name.to_owned()));
        };
        entry.spec.enabled = true;
        entry.running = true;
        entry.bound_addr = Some(bound_addr);
        entry.cancel = cancel;
        entry.done = done;
        // Stored definitions already carry synchronized plans/policies.
        Ok(bound_addr)
    }

    async fn spawn_supervisor(
        &self,
        listener: TcpListener,
        name: String,
        cancel: CancellationToken,
    ) -> Arc<SupervisorDone> {
        let runtime = self.runtime.clone();
        let done = Arc::new(SupervisorDone::default());
        let task_done = done.clone();
        let task_name = name.clone();
        let handle = tokio::spawn(proxy_supervisor(
            listener, task_name, runtime, cancel, task_done,
        ));
        self.runtime.root.lock().await.push((name, handle));
        done
    }

    /// Stop the listener, terminate its connections, await supervisor exit,
    /// and remove the definition. Success is reported only after the
    /// runtime transition commits.
    pub async fn delete_proxy(&self, name: &str) -> Result<u64, ControlError> {
        let (cancel, done) = {
            let mut map = self.runtime.proxies.write().await;
            let Some(entry) = map.remove(name) else {
                return Err(ControlError::NotFound(name.to_owned()));
            };
            (entry.cancel, entry.done)
        };
        cancel.cancel();
        done.join().await;
        self.untrack(name).await;
        // Connections observe the proxy cancellation through child tokens;
        // the supervisor purges stragglers before its done-flag resolves.
        Ok(self.next_generation())
    }

    /// Enable or disable a proxy. Disabling stops the listener and
    /// terminates active connections while retaining the definition and
    /// fault plans. Enabling rebinds through the same machinery as create.
    pub async fn set_enabled(&self, name: &str, enabled: bool) -> Result<u64, ControlError> {
        if enabled {
            let spec = self
                .runtime
                .proxies
                .read()
                .await
                .get(name)
                .map(|entry| (entry.spec.clone(), entry.running))
                .ok_or_else(|| ControlError::NotFound(name.to_owned()))?;
            if spec.1 {
                return Ok(self.generation());
            }
            self.start_stored(name, &spec.0).await?;
            return Ok(self.next_generation());
        }
        let (cancel, done) = {
            let mut map = self.runtime.proxies.write().await;
            let Some(entry) = map.get_mut(name) else {
                return Err(ControlError::NotFound(name.to_owned()));
            };
            if !entry.running {
                return Ok(self.generation());
            }
            entry.running = false;
            entry.bound_addr = None;
            entry.spec.enabled = false;
            // Fresh scope for the next enable; active connections observe
            // the old scope's cancellation.
            let old =
                std::mem::replace(&mut entry.cancel, self.runtime.shutdown_token.child_token());
            (old, entry.done.clone())
        };
        cancel.cancel();
        done.join().await;
        self.untrack(name).await;
        Ok(self.next_generation())
    }

    /// Update proxy fields. Fault-only changes are out of scope here (use
    /// fault CRUD). `max_connections`/`connect_timeout` apply to new
    /// connections. Listen/upstream changes are restart-class: the
    /// replacement binds before the old listener stops, so a bind failure
    /// keeps the old listener serving and reports `RestartFailed` without
    /// changing the spec.
    pub async fn update_proxy(
        &self,
        name: &str,
        patch: ProxyPatch,
    ) -> Result<(ProxyView, u64), ControlError> {
        if let Some(enabled) = patch.enabled {
            self.set_enabled(name, enabled).await?;
        }
        if let Some(connect_timeout_ms) = patch.connect_timeout_ms {
            if connect_timeout_ms == 0 {
                return Err(ControlError::Invalid(
                    "connect timeout must be non-zero".into(),
                ));
            }
        }
        let current = self
            .runtime
            .proxies
            .read()
            .await
            .get(name)
            .map(|entry| (entry.spec.clone(), entry.running))
            .ok_or_else(|| ControlError::NotFound(name.to_owned()))?;
        let restart = patch
            .listen
            .is_some_and(|listen| listen != current.0.listen)
            || patch
                .upstream
                .is_some_and(|upstream| upstream != current.0.upstream);
        let mut spec = current.0;
        if let Some(listen) = patch.listen {
            spec.listen = listen;
        }
        if let Some(upstream) = patch.upstream {
            spec.upstream = upstream;
        }
        if let Some(max_connections) = patch.max_connections {
            spec.max_connections = max_connections;
        }
        if let Some(connect_timeout_ms) = patch.connect_timeout_ms {
            spec.connect_timeout = Duration::from_millis(connect_timeout_ms);
        }
        spec.validate().map_err(|error| match error {
            EggchaosError::InvalidProxy(message) => ControlError::Invalid(message),
            EggchaosError::InvalidPlan(error) => {
                ControlError::Invalid(format!("invalid fault plan: {error}"))
            }
            other => ControlError::Invalid(other.to_string()),
        })?;
        if !restart || !current.1 {
            // Live fields, or a stopped proxy whose spec simply updates.
            let mut map = self.runtime.proxies.write().await;
            let Some(entry) = map.get_mut(name) else {
                return Err(ControlError::NotFound(name.to_owned()));
            };
            let running = entry.running;
            let bound_addr = entry.bound_addr;
            let cancel = entry.cancel.clone();
            let done = entry.done.clone();
            let active = entry.active.clone();
            let ordinal = entry.ordinal.clone();
            // Preserve live policies: fault plans are untouched by this
            // patch, so policies stay synchronized by construction.
            let mut next = spec.clone();
            next.upstream_policy = entry.spec.upstream_policy.clone();
            next.downstream_policy = entry.spec.downstream_policy.clone();
            *entry = ManagedProxy {
                spec: next,
                running,
                bound_addr,
                cancel,
                done,
                active,
                ordinal,
            };
            let view = RuntimeInner::view_of(entry);
            return Ok((view, self.next_generation()));
        }
        // Restart-class: pre-bind the replacement before touching the old
        // listener, so a bind failure keeps the old listener serving with
        // its bound address and spec untouched.
        let prebound = match TcpListener::bind(spec.listen).await {
            Ok(listener) => listener,
            Err(source) => {
                return Err(ControlError::RestartFailed {
                    proxy: name.to_owned(),
                    reason: format!(
                        "replacement bind failed ({source}); old listener still serving"
                    ),
                });
            }
        };
        let bound_addr = prebound
            .local_addr()
            .map_err(|source| ControlError::RestartFailed {
                proxy: name.to_owned(),
                reason: format!("replacement bound but address unreadable: {source}"),
            })?;
        let (old_cancel, old_done) = {
            let mut map = self.runtime.proxies.write().await;
            let Some(entry) = map.get_mut(name) else {
                return Err(ControlError::NotFound(name.to_owned()));
            };
            entry.running = false;
            entry.bound_addr = None;
            let old_cancel =
                std::mem::replace(&mut entry.cancel, self.runtime.shutdown_token.child_token());
            (old_cancel, entry.done.clone())
        };
        old_cancel.cancel();
        old_done.join().await;
        self.untrack(name).await;
        let cancel = self.runtime.shutdown_token.child_token();
        let done = self
            .spawn_supervisor(prebound, name.to_owned(), cancel.clone())
            .await;
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(name) else {
            // Proxy vanished while restarting: stop the orphan.
            drop(map);
            cancel.cancel();
            done.join().await;
            self.untrack(name).await;
            return Err(ControlError::NotFound(name.to_owned()));
        };
        let mut next = spec.clone();
        next.upstream_policy = entry.spec.upstream_policy.clone();
        next.downstream_policy = entry.spec.downstream_policy.clone();
        entry.spec = next;
        entry.running = true;
        entry.bound_addr = Some(bound_addr);
        entry.cancel = cancel;
        entry.done = done;
        let view = RuntimeInner::view_of(entry);
        Ok((view, self.next_generation()))
    }

    fn build_fault_spec(upsert: &FaultUpsert) -> Result<FaultSpec, ControlError> {
        if upsert.id.is_empty() || upsert.id.len() > 128 {
            return Err(ControlError::Invalid(
                "fault id must be 1..=128 bytes".into(),
            ));
        }
        if !(0.0..=1.0).contains(&upsert.probability) || !upsert.probability.is_finite() {
            return Err(ControlError::Invalid(
                "probability must be finite and between 0 and 1".into(),
            ));
        }
        Ok(FaultSpec {
            id: eggchaos_core::FaultId::new(upsert.id.clone())
                .map_err(|error| ControlError::Invalid(error.to_string()))?,
            probability: eggchaos_core::Probability::new(upsert.probability)
                .map_err(|error| ControlError::Invalid(error.to_string()))?,
            kind: upsert.kind,
        })
    }

    /// Add a fault to one direction, updating the canonical plan and the
    /// live policy together under one lock. Fault IDs are unique within a
    /// direction/proxy.
    pub async fn add_fault(
        &self,
        proxy: &str,
        upsert: FaultUpsert,
    ) -> Result<(Direction, FaultSpec, u64), ControlError> {
        let spec = Self::build_fault_spec(&upsert)?;
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(proxy) else {
            return Err(ControlError::NotFound(proxy.to_owned()));
        };
        // The base plan comes from one atomic snapshot load, so a
        // concurrent publication cannot slip between the read and the
        // publish below: the write lock serializes publishers.
        let (base, policy) = match upsert.direction {
            Direction::Upstream => (
                entry.spec.upstream_policy.snapshot(),
                entry.spec.upstream_policy.clone(),
            ),
            Direction::Downstream => (
                entry.spec.downstream_policy.snapshot(),
                entry.spec.downstream_policy.clone(),
            ),
        };
        if base.plan.get(spec.id.as_str()).is_some() {
            return Err(ControlError::Conflict(format!(
                "fault {} already exists on {}/{}",
                spec.id,
                proxy,
                upsert.direction.as_str()
            )));
        }
        let mut next_faults = base.plan.faults().to_vec();
        next_faults.push(spec.clone());
        let next = FaultPlan::new(next_faults)
            .map_err(|error| ControlError::Invalid(error.to_string()))?;
        let published = policy
            .publish(next, base.seed_namespace)
            .map_err(|error| ControlError::Invalid(error.to_string()))?;
        // Canonical mirror refreshes from the published snapshot, never
        // from a locally built plan.
        match upsert.direction {
            Direction::Upstream => entry.spec.upstream_faults = (*published.plan).clone(),
            Direction::Downstream => entry.spec.downstream_faults = (*published.plan).clone(),
        }
        Ok((upsert.direction, spec, self.next_generation()))
    }

    /// Update a fault's probability and/or behavior in place, preserving
    /// order and keeping canonical and live state synchronized.
    pub async fn update_fault(
        &self,
        proxy: &str,
        id: &str,
        patch: FaultPatch,
    ) -> Result<(Direction, FaultSpec, u64), ControlError> {
        if let Some(probability) = patch.probability {
            if !(0.0..=1.0).contains(&probability) || !probability.is_finite() {
                return Err(ControlError::Invalid(
                    "probability must be finite and between 0 and 1".into(),
                ));
            }
        }
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(proxy) else {
            return Err(ControlError::NotFound(proxy.to_owned()));
        };
        for direction in [Direction::Upstream, Direction::Downstream] {
            let policy = match direction {
                Direction::Upstream => entry.spec.upstream_policy.clone(),
                Direction::Downstream => entry.spec.downstream_policy.clone(),
            };
            let base = policy.snapshot();
            let Some(existing) = base.plan.get(id) else {
                continue;
            };
            let mut next_spec = existing.clone();
            if let Some(probability) = patch.probability {
                next_spec.probability = eggchaos_core::Probability::new(probability)
                    .map_err(|error| ControlError::Invalid(error.to_string()))?;
            }
            if let Some(kind) = patch.kind {
                next_spec.kind = kind;
            }
            let mut next_faults = base.plan.faults().to_vec();
            let position = next_faults
                .iter()
                .position(|fault| fault.id.as_str() == id)
                .expect("fault found in base");
            next_faults[position] = next_spec.clone();
            let next = FaultPlan::new(next_faults)
                .map_err(|error| ControlError::Invalid(error.to_string()))?;
            let published = policy
                .publish(next, base.seed_namespace)
                .map_err(|error| ControlError::Invalid(error.to_string()))?;
            match direction {
                Direction::Upstream => entry.spec.upstream_faults = (*published.plan).clone(),
                Direction::Downstream => entry.spec.downstream_faults = (*published.plan).clone(),
            }
            return Ok((direction, next_spec, self.next_generation()));
        }
        Err(ControlError::NotFound(format!("fault {id} on {proxy}")))
    }

    /// Remove a fault from either direction.
    pub async fn remove_fault(&self, proxy: &str, id: &str) -> Result<u64, ControlError> {
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(proxy) else {
            return Err(ControlError::NotFound(proxy.to_owned()));
        };
        for direction in [Direction::Upstream, Direction::Downstream] {
            let policy = match direction {
                Direction::Upstream => entry.spec.upstream_policy.clone(),
                Direction::Downstream => entry.spec.downstream_policy.clone(),
            };
            let base = policy.snapshot();
            if base.plan.get(id).is_none() {
                continue;
            }
            let next = (*base.plan).clone().without_fault(id);
            next.validate()
                .map_err(|error| ControlError::Invalid(error.to_string()))?;
            let published = policy
                .publish(next, base.seed_namespace)
                .map_err(|error| ControlError::Invalid(error.to_string()))?;
            match direction {
                Direction::Upstream => entry.spec.upstream_faults = (*published.plan).clone(),
                Direction::Downstream => entry.spec.downstream_faults = (*published.plan).clone(),
            }
            return Ok(self.next_generation());
        }
        Err(ControlError::NotFound(format!("fault {id} on {proxy}")))
    }

    /// Fetch one fault, searching upstream then downstream. Plans come
    /// from live snapshots, so reads agree with what streams compile.
    pub async fn get_fault(&self, proxy: &str, id: &str) -> Option<(Direction, FaultSpec)> {
        let map = self.runtime.proxies.read().await;
        let entry = map.get(proxy)?;
        if let Some(fault) = entry.spec.upstream_policy.snapshot().plan.get(id) {
            return Some((Direction::Upstream, fault.clone()));
        }
        entry
            .spec
            .downstream_policy
            .snapshot()
            .plan
            .get(id)
            .map(|fault| (Direction::Downstream, fault.clone()))
    }

    /// List both directional fault plans from live snapshots.
    pub async fn list_faults(&self, proxy: &str) -> Option<(Vec<FaultSpec>, Vec<FaultSpec>)> {
        let map = self.runtime.proxies.read().await;
        let entry = map.get(proxy)?;
        Some((
            entry.spec.upstream_policy.snapshot().plan.faults().to_vec(),
            entry
                .spec
                .downstream_policy
                .snapshot()
                .plan
                .faults()
                .to_vec(),
        ))
    }

    /// Publish complete fault-plan generations for an existing proxy,
    /// updating live policies and canonical mirrors together. Seed
    /// namespaces are retained.
    pub async fn publish_plans(
        &self,
        name: &str,
        upstream: FaultPlan,
        downstream: FaultPlan,
    ) -> Result<u64, ControlError> {
        upstream
            .validate()
            .map_err(|error| ControlError::Invalid(error.to_string()))?;
        downstream
            .validate()
            .map_err(|error| ControlError::Invalid(error.to_string()))?;
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(name) else {
            return Err(ControlError::NotFound(name.to_owned()));
        };
        let base_upstream = entry.spec.upstream_policy.snapshot();
        let base_downstream = entry.spec.downstream_policy.snapshot();
        let published_upstream = entry
            .spec
            .upstream_policy
            .publish(upstream, base_upstream.seed_namespace)
            .map_err(|error| ControlError::Invalid(error.to_string()))?;
        let published_downstream = entry
            .spec
            .downstream_policy
            .publish(downstream, base_downstream.seed_namespace)
            .map_err(|error| ControlError::Invalid(error.to_string()))?;
        entry.spec.upstream_faults = (*published_upstream.plan).clone();
        entry.spec.downstream_faults = (*published_downstream.plan).clone();
        Ok(self.next_generation())
    }

    /// Publish plans only when both directions still sit on the expected
    /// generations, deriving no seed change: a concurrent publication
    /// fails fast with `Conflict` instead of being silently overwritten
    /// by a stale base. Scenario events use this path. Returns the global
    /// generation plus the two published policy generations for trails.
    pub async fn publish_plans_expected(
        &self,
        name: &str,
        publish: ExpectedPublish,
    ) -> Result<(u64, u64, u64), ControlError> {
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(name) else {
            return Err(ControlError::NotFound(name.to_owned()));
        };
        let published_upstream = entry
            .spec
            .upstream_policy
            .publish_expected(
                publish.upstream,
                publish.upstream_seed,
                publish.expected_upstream,
            )
            .map_err(|error| match error {
                eggchaos_core::PublishError::Invalid(error) => {
                    ControlError::Invalid(error.to_string())
                }
                eggchaos_core::PublishError::Conflict(conflict) => {
                    ControlError::Conflict(conflict_message(name, conflict))
                }
            })?;
        let published_downstream = entry
            .spec
            .downstream_policy
            .publish_expected(
                publish.downstream,
                publish.downstream_seed,
                publish.expected_downstream,
            )
            .map_err(|error| match error {
                eggchaos_core::PublishError::Invalid(error) => {
                    ControlError::Invalid(error.to_string())
                }
                eggchaos_core::PublishError::Conflict(conflict) => {
                    ControlError::Conflict(conflict_message(name, conflict))
                }
            })?;
        entry.spec.upstream_faults = (*published_upstream.plan).clone();
        entry.spec.downstream_faults = (*published_downstream.plan).clone();
        Ok((
            self.next_generation(),
            published_upstream.generation,
            published_downstream.generation,
        ))
    }

    /// Maximum retained scenario run records.
    pub const MAX_SCENARIO_RUNS: usize = 32;

    /// Publish one direction only when it still sits on the expected
    /// generation. Scenario events use this so an event touches exactly
    /// its target direction: the other direction keeps its plan,
    /// generation, and seed namespace. Returns the global generation
    /// plus the published policy generation.
    pub async fn publish_direction_expected(
        &self,
        name: &str,
        direction: Direction,
        plan: FaultPlan,
        seed_namespace: u64,
        expected: u64,
    ) -> Result<(u64, u64), ControlError> {
        let mut map = self.runtime.proxies.write().await;
        let Some(entry) = map.get_mut(name) else {
            return Err(ControlError::NotFound(name.to_owned()));
        };
        let policy = match direction {
            Direction::Upstream => entry.spec.upstream_policy.clone(),
            Direction::Downstream => entry.spec.downstream_policy.clone(),
        };
        let published = policy
            .publish_expected(plan, seed_namespace, expected)
            .map_err(|error| match error {
                eggchaos_core::PublishError::Invalid(error) => {
                    ControlError::Invalid(error.to_string())
                }
                eggchaos_core::PublishError::Conflict(conflict) => {
                    ControlError::Conflict(conflict_message(name, conflict))
                }
            })?;
        match direction {
            Direction::Upstream => entry.spec.upstream_faults = (*published.plan).clone(),
            Direction::Downstream => entry.spec.downstream_faults = (*published.plan).clone(),
        }
        Ok((self.next_generation(), published.generation))
    }

    /// Atomically snapshot both directional policies for a proxy: the
    /// plans a scenario event must base its publication on.
    pub async fn snapshot_policies(
        &self,
        name: &str,
    ) -> Option<(
        Arc<eggchaos_core::PublishedPolicy>,
        Arc<eggchaos_core::PublishedPolicy>,
    )> {
        let map = self.runtime.proxies.read().await;
        let entry = map.get(name)?;
        Some((
            entry.spec.upstream_policy.snapshot(),
            entry.spec.downstream_policy.snapshot(),
        ))
    }

    /// Start an owned scenario run. The document validates entirely
    /// upfront; the run task is supervised by the service and cancelled
    /// on shutdown, never detached.
    pub async fn start_scenario(
        &self,
        scenario: crate::scenario::Scenario,
    ) -> Result<crate::scenario::ScenarioRunRecord, EggchaosError> {
        crate::scenario::validate_scenario(self, &scenario).await?;
        let run_id = self.runtime.next_run_id.fetch_add(1, Ordering::AcqRel);
        let record = crate::scenario::ScenarioRunRecord {
            run_id,
            seed: scenario.seed,
            status: crate::scenario::ScenarioRunStatus::Pending,
            applied: 0,
            failure: None,
            trail: Vec::new(),
        };
        {
            let mut runs = self.runtime.scenario_runs.lock().await;
            // Prune oldest finished runs first; active runs always fit
            // until the active cap, which fails fast instead of queuing.
            let active = runs
                .values()
                .filter(|record| {
                    matches!(
                        record.status,
                        crate::scenario::ScenarioRunStatus::Pending
                            | crate::scenario::ScenarioRunStatus::Running
                            | crate::scenario::ScenarioRunStatus::Cancelling
                    )
                })
                .count();
            if active >= Self::MAX_SCENARIO_RUNS {
                return Err(EggchaosError::Control(ControlError::Conflict(
                    "too many active scenario runs".into(),
                )));
            }
            while runs.len() >= Self::MAX_SCENARIO_RUNS {
                let oldest_finished = runs
                    .iter()
                    .find(|(_, record)| {
                        !matches!(
                            record.status,
                            crate::scenario::ScenarioRunStatus::Pending
                                | crate::scenario::ScenarioRunStatus::Running
                                | crate::scenario::ScenarioRunStatus::Cancelling
                        )
                    })
                    .map(|(id, _)| *id);
                let Some(oldest) = oldest_finished else {
                    break;
                };
                runs.remove(&oldest);
            }
            runs.insert(run_id, record.clone());
        }
        let token = self.runtime.shutdown_token.child_token();
        self.runtime
            .scenario_tokens
            .lock()
            .await
            .insert(run_id, token.clone());
        let state = self.clone();
        self.runtime
            .scenario_tasks
            .lock()
            .await
            .spawn(crate::scenario::drive_scenario_run(
                state, run_id, scenario, token,
            ));
        Ok(record)
    }

    /// Fetch one scenario run record.
    pub async fn get_scenario(&self, run_id: u64) -> Option<crate::scenario::ScenarioRunRecord> {
        self.runtime
            .scenario_runs
            .lock()
            .await
            .get(&run_id)
            .cloned()
    }

    /// Cancel an active scenario run, returning its latest record.
    /// Returns `None` for unknown run IDs. Cancelling a finished run
    /// returns its final record unchanged.
    pub async fn cancel_scenario(&self, run_id: u64) -> Option<crate::scenario::ScenarioRunRecord> {
        let token = self.runtime.scenario_tokens.lock().await.remove(&run_id);
        if let Some(token) = token {
            token.cancel();
            self.update_scenario_run(run_id, |record| {
                if matches!(
                    record.status,
                    crate::scenario::ScenarioRunStatus::Pending
                        | crate::scenario::ScenarioRunStatus::Running
                ) {
                    record.status = crate::scenario::ScenarioRunStatus::Cancelling;
                }
            })
            .await;
        }
        self.get_scenario(run_id).await
    }

    /// Apply a record mutation for a scenario run, if still retained.
    pub async fn update_scenario_run(
        &self,
        run_id: u64,
        update: impl FnOnce(&mut crate::scenario::ScenarioRunRecord),
    ) {
        if let Some(record) = self.runtime.scenario_runs.lock().await.get_mut(&run_id) {
            update(record);
        }
    }

    /// Drop a finished run's cancellation token.
    pub async fn remove_scenario_token(&self, run_id: u64) {
        self.runtime.scenario_tokens.lock().await.remove(&run_id);
    }

    /// Reset the service: retain definitions and listen/upstream addresses,
    /// enable every proxy, replace all fault plans with empty plans, and
    /// terminate active connections. Exactly one generation covers the
    /// transaction; proxies whose bind fails stay disabled and are reported.
    pub async fn reset(&self) -> Result<ResetReport, ControlError> {
        let mut failed_enables = Vec::new();
        let to_start: Vec<String> = {
            let mut map = self.runtime.proxies.write().await;
            // Terminate active connections through level-triggered tokens.
            for token in self.runtime.cancellations.read().await.values() {
                token.cancel();
            }
            for entry in map.values_mut() {
                entry.spec.enabled = true;
                entry.spec.upstream_faults = FaultPlan::empty();
                entry.spec.downstream_faults = FaultPlan::empty();
                // Reset clears plans but retains seed namespaces; the next
                // scenario or manual publication sets its own.
                let upstream_ns = entry.spec.upstream_policy.seed_namespace();
                let downstream_ns = entry.spec.downstream_policy.seed_namespace();
                let _ = entry
                    .spec
                    .upstream_policy
                    .publish(FaultPlan::empty(), upstream_ns);
                let _ = entry
                    .spec
                    .downstream_policy
                    .publish(FaultPlan::empty(), downstream_ns);
            }
            map.iter()
                .filter(|(_, entry)| !entry.running)
                .map(|(name, _)| name.clone())
                .collect()
        };
        for name in to_start {
            let spec = self
                .runtime
                .proxies
                .read()
                .await
                .get(&name)
                .map(|entry| entry.spec.clone());
            let Some(spec) = spec else { continue };
            if let Err(error) = self.start_stored(&name, &spec).await {
                tracing::warn!("reset could not enable proxy {name}: {error}");
                failed_enables.push(name);
            }
        }
        Ok(ResetReport {
            generation: self.next_generation(),
            reset: true,
            failed_enables,
        })
    }

    /// Mark service shutdown and cascade cancellation to every proxy and
    /// connection scope. Level-triggered: tasks that begin waiting later
    /// still observe it.
    pub fn initiate_shutdown(&self) {
        self.runtime
            .shutdown_initiated
            .store(true, Ordering::Release);
        self.runtime.shutdown_token.cancel();
    }

    /// Shut down and wait until every supervised listener and connection
    /// task has drained. Owned scenario runs observe shutdown through
    /// their tokens and are joined here, so none outlive the service.
    /// Idempotent.
    pub async fn shutdown_and_join(&self) {
        self.initiate_shutdown();
        let tracked: Vec<(String, JoinHandle<()>)> = {
            let mut root = self.runtime.root.lock().await;
            std::mem::take(&mut *root)
        };
        for (_, handle) in tracked {
            let _ = handle.await;
        }
        let mut scenarios = self.runtime.scenario_tasks.lock().await;
        while scenarios.join_next().await.is_some() {}
    }

    /// Actual bound addresses for running proxies.
    pub async fn bound_addresses(&self) -> HashMap<String, SocketAddr> {
        self.runtime
            .proxies
            .read()
            .await
            .iter()
            .filter_map(|(name, entry)| entry.bound_addr.map(|addr| (name.clone(), addr)))
            .collect()
    }
}
