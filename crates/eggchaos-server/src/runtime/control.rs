use super::*;

fn map_datagram_error(error: DatagramRuntimeError) -> ControlError {
    match error {
        DatagramRuntimeError::Invalid(message) => ControlError::Invalid(message),
        DatagramRuntimeError::Duplicate(name) => {
            ControlError::Conflict(format!("datagram proxy {name} already exists"))
        }
        DatagramRuntimeError::NotFound(name) => ControlError::NotFound(name),
        DatagramRuntimeError::Bind(error) => ControlError::BindFailed {
            proxy: "datagram".into(),
            reason: error.to_string(),
        },
        DatagramRuntimeError::AssociationLimit => {
            ControlError::Invalid("datagram association limit reached".into())
        }
        DatagramRuntimeError::Conflict(message) => ControlError::Conflict(message),
    }
}

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
            "# HELP eggchaos_termination_requests_total Injected termination requests observed at close\n# TYPE eggchaos_termination_requests_total counter\neggchaos_termination_requests_total{{request=\"graceful\"}} {}\neggchaos_termination_requests_total{{request=\"hard_reset\"}} {}\n# HELP eggchaos_reset_results_total Abortive-close outcomes at the TCP edge\n# TYPE eggchaos_reset_results_total counter\neggchaos_reset_results_total{{result=\"applied\"}} {}\neggchaos_reset_results_total{{result=\"unsupported\"}} {}\neggchaos_reset_results_total{{result=\"failed\"}} {}\n# HELP eggchaos_bytes_total Bytes by flow across closed connections\n# TYPE eggchaos_bytes_total counter\neggchaos_bytes_total{{flow=\"accepted\"}} {}\neggchaos_bytes_total{{flow=\"forwarded\"}} {}\neggchaos_bytes_total{{flow=\"discarded\"}} {}\n# HELP eggchaos_policy_transitions_total Completed live-policy transitions\n# TYPE eggchaos_policy_transitions_total counter\neggchaos_policy_transitions_total {}\n# HELP eggchaos_schedule_v2_runs_total V2 schedule runs started\n# TYPE eggchaos_schedule_v2_runs_total counter\neggchaos_schedule_v2_runs_total {}\n# HELP eggchaos_schedule_v2_events_total V2 schedule events applied\n# TYPE eggchaos_schedule_v2_events_total counter\neggchaos_schedule_v2_events_total {}\n# HELP eggchaos_schedule_v2_late_events_total V2 schedule events applied after their deadline\n# TYPE eggchaos_schedule_v2_late_events_total counter\neggchaos_schedule_v2_late_events_total {}\n",
            self.runtime.metrics.graceful_requests.load(Ordering::Relaxed),
            self.runtime.metrics.hard_reset_requests.load(Ordering::Relaxed),
            self.runtime.metrics.reset_applied.load(Ordering::Relaxed),
            self.runtime.metrics.reset_unsupported.load(Ordering::Relaxed),
            self.runtime.metrics.reset_failed.load(Ordering::Relaxed),
            self.runtime.metrics.bytes_accepted.load(Ordering::Relaxed),
            self.runtime.metrics.bytes_forwarded.load(Ordering::Relaxed),
            self.runtime.metrics.bytes_discarded.load(Ordering::Relaxed),
            self.runtime.metrics.transitions.load(Ordering::Relaxed),
            self.runtime.metrics.schedule_v2_runs.load(Ordering::Relaxed),
            self.runtime.metrics.schedule_v2_events.load(Ordering::Relaxed),
            self.runtime
                .metrics
                .schedule_v2_late_events
                .load(Ordering::Relaxed),
        ));
        text.push_str("# HELP eggchaos_stream_loss_chunks_evaluated_total Logical stream-loss chunks evaluated\n# TYPE eggchaos_stream_loss_chunks_evaluated_total counter\n# HELP eggchaos_stream_loss_chunks_dropped_total Logical stream-loss chunks dropped\n# TYPE eggchaos_stream_loss_chunks_dropped_total counter\n# HELP eggchaos_stream_loss_bytes_discarded_total Bytes discarded by stream-loss faults\n# TYPE eggchaos_stream_loss_bytes_discarded_total counter\n");
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
                    for (metric, value) in [
                        (
                            "eggchaos_stream_loss_chunks_evaluated_total",
                            entry.stream_loss[index][0],
                        ),
                        (
                            "eggchaos_stream_loss_chunks_dropped_total",
                            entry.stream_loss[index][1],
                        ),
                        (
                            "eggchaos_stream_loss_bytes_discarded_total",
                            entry.stream_loss[index][2],
                        ),
                    ] {
                        text.push_str(&format!(
                            "{metric}{{proxy=\"{name}\",direction=\"{direction}\"}} {value}\n"
                        ));
                    }
                    for (metric, value) in [
                        (
                            "eggchaos_stream_loss_chunks_evaluated_total",
                            entry.stream_loss[index][0],
                        ),
                        (
                            "eggchaos_stream_loss_chunks_dropped_total",
                            entry.stream_loss[index][1],
                        ),
                        (
                            "eggchaos_stream_loss_bytes_discarded_total",
                            entry.stream_loss[index][2],
                        ),
                    ] {
                        text.push_str(&format!(
                            "{metric}{{proxy=\"{name}\",direction=\"{direction}\"}} {value}\\n"
                        ));
                    }
                }
            }
            if tables.overflow_proxy.accepted + tables.overflow_proxy.completed > 0 {
                text.push_str(&format!(
                    "eggchaos_proxy_connections_accepted_total{{proxy=\"_overflow\"}} {}\neggchaos_proxy_connections_completed_total{{proxy=\"_overflow\"}} {}\n",
                    tables.overflow_proxy.accepted, tables.overflow_proxy.completed,
                ));
                for (direction, index) in [("upstream", 0), ("downstream", 1)] {
                    for (metric, value) in [
                        (
                            "eggchaos_stream_loss_chunks_evaluated_total",
                            tables.overflow_proxy.stream_loss[index][0],
                        ),
                        (
                            "eggchaos_stream_loss_chunks_dropped_total",
                            tables.overflow_proxy.stream_loss[index][1],
                        ),
                        (
                            "eggchaos_stream_loss_bytes_discarded_total",
                            tables.overflow_proxy.stream_loss[index][2],
                        ),
                    ] {
                        text.push_str(&format!(
                            "{metric}{{proxy=\"_overflow\",direction=\"{direction}\"}} {value}\n"
                        ));
                    }
                }
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
        let datagram_proxies = self.runtime.datagrams.proxies().await;
        let datagram_evidence = self.runtime.datagrams.all_associations().await;
        text.push_str("# HELP eggchaos_datagram_associations_active Active UDP client associations\n# TYPE eggchaos_datagram_associations_active gauge\n");
        text.push_str("# HELP eggchaos_datagram_evidence Current plus retained association evidence; kind labels are fixed\n# TYPE eggchaos_datagram_evidence gauge\n");
        text.push_str("# HELP eggchaos_datagram_fault_activations Current plus retained datagram fault activations\n# TYPE eggchaos_datagram_fault_activations gauge\n");
        text.push_str("# HELP eggchaos_datagram_proxy_drops Proxy-level UDP admission/drop observations\n# TYPE eggchaos_datagram_proxy_drops gauge\n");
        text.push_str("# HELP eggchaos_datagram_administrative_discards Queued datagrams discarded by operator lifecycle actions\n# TYPE eggchaos_datagram_administrative_discards gauge\n");
        for proxy in &datagram_proxies {
            text.push_str(&format!(
                "eggchaos_datagram_associations_active{{proxy=\"{}\"}} {}\n",
                proxy.name, proxy.active_associations
            ));
            for (kind, value) in [
                ("oversize", proxy.oversize_datagrams),
                ("capacity_rejection", proxy.association_capacity_rejections),
                ("ingress_queue_overflow", proxy.ingress_queue_overflow),
                (
                    "association_setup_failure",
                    proxy.association_setup_failures,
                ),
            ] {
                text.push_str(&format!(
                    "eggchaos_datagram_proxy_drops{{proxy=\"{}\",kind=\"{kind}\"}} {value}\n",
                    proxy.name
                ));
            }
            let administrative_discards = datagram_evidence
                .iter()
                .filter(|association| association.proxy == proxy.name)
                .fold(0u64, |total, association| {
                    total.saturating_add(association.administrative_discards)
                });
            text.push_str(&format!("eggchaos_datagram_administrative_discards{{proxy=\"{}\"}} {administrative_discards}\n", proxy.name));
            for direction in ["upstream", "downstream"] {
                let mut totals = [0u64; 12];
                let mut activations = [0u64; 6];
                for association in datagram_evidence
                    .iter()
                    .filter(|association| association.proxy == proxy.name)
                {
                    let evidence = if direction == "upstream" {
                        &association.upstream_evidence
                    } else {
                        &association.downstream_evidence
                    };
                    totals[0] = totals[0].saturating_add(evidence.admitted_datagrams);
                    totals[1] = totals[1].saturating_add(evidence.admitted_bytes);
                    totals[2] = totals[2].saturating_add(evidence.emitted_datagrams);
                    totals[3] = totals[3].saturating_add(evidence.emitted_bytes);
                    totals[4] = totals[4].saturating_add(evidence.configured_loss);
                    totals[5] = totals[5].saturating_add(evidence.queue_overflow);
                    totals[6] = totals[6].saturating_add(evidence.duplicated_copies);
                    totals[7] = totals[7].saturating_add(evidence.corrupted_candidates);
                    totals[8] = totals[8].saturating_add(evidence.queued_datagrams);
                    totals[9] = totals[9].saturating_add(evidence.queued_bytes);
                    totals[10] = totals[10].saturating_add(evidence.high_water_datagrams);
                    totals[11] = totals[11].saturating_add(evidence.high_water_bytes);
                    for (slot, value) in activations.iter_mut().zip(evidence.fault_activations) {
                        *slot = slot.saturating_add(value);
                    }
                }
                for (kind, value) in [
                    "ingress_datagrams",
                    "ingress_bytes",
                    "egress_datagrams",
                    "egress_bytes",
                    "configured_loss",
                    "queue_overflow",
                    "duplicated_copies",
                    "corrupted_candidates",
                    "queued_datagrams",
                    "queued_bytes",
                    "high_water_datagrams",
                    "high_water_bytes",
                ]
                .into_iter()
                .zip(totals)
                {
                    text.push_str(&format!("eggchaos_datagram_evidence{{proxy=\"{}\",direction=\"{direction}\",kind=\"{kind}\"}} {value}\n", proxy.name));
                }
                for (fault_type, value) in eggchaos_core::DATAGRAM_FAULT_TYPE_NAMES
                    .into_iter()
                    .zip(activations)
                {
                    text.push_str(&format!("eggchaos_datagram_fault_activations{{proxy=\"{}\",direction=\"{direction}\",fault_type=\"{fault_type}\"}} {value}\n", proxy.name));
                }
            }
        }
        text
    }

    /// Create and bind a fixed-target UDP proxy in the shared runtime authority.
    pub async fn create_datagram_proxy(
        &self,
        proxy: DatagramProxySpec,
    ) -> Result<(DatagramProxyView, u64), ControlError> {
        self.runtime
            .datagrams
            .create_proxy(proxy)
            .await
            .map(|view| (view, self.next_generation()))
            .map_err(map_datagram_error)
    }

    /// List fixed-target UDP proxies.
    pub async fn datagram_proxies(&self) -> Vec<DatagramProxyView> {
        self.runtime.datagrams.proxies().await
    }

    /// Get one fixed-target UDP proxy.
    pub async fn get_datagram_proxy(&self, name: &str) -> Option<DatagramProxyView> {
        self.runtime.datagrams.proxy(name).await
    }

    /// Enable or disable a datagram listener, joining all work when disabled.
    pub async fn set_datagram_proxy_enabled(
        &self,
        name: &str,
        enabled: bool,
    ) -> Result<(DatagramProxyView, u64), ControlError> {
        let view = if enabled {
            self.runtime
                .datagrams
                .enable_proxy(name)
                .await
                .map_err(|error| match error {
                    DatagramRuntimeError::Bind(reason) => ControlError::BindFailed {
                        proxy: name.into(),
                        reason: reason.to_string(),
                    },
                    other => map_datagram_error(other),
                })?
        } else {
            self.runtime
                .datagrams
                .disable_proxy(name)
                .await
                .map_err(map_datagram_error)?;
            self.runtime
                .datagrams
                .proxy(name)
                .await
                .ok_or_else(|| ControlError::NotFound(name.into()))?
        };
        Ok((view, self.next_generation()))
    }

    /// Apply supported datagram listener/target changes with bind-before-swap
    /// behavior for a running listener.
    pub async fn update_datagram_proxy(
        &self,
        name: &str,
        listen: Option<SocketAddr>,
        upstream: Option<SocketAddr>,
        max_associations: Option<usize>,
        association_idle_timeout_ms: Option<u64>,
        enabled: Option<bool>,
    ) -> Result<(DatagramProxyView, u64), ControlError> {
        if association_idle_timeout_ms.is_some_and(|timeout| timeout == 0 || timeout > 86_400_000) {
            return Err(ControlError::Invalid(
                "association_idle_timeout_ms must be in 1..=86400000".into(),
            ));
        }
        let view = self
            .runtime
            .datagrams
            .update_proxy(
                name,
                listen,
                upstream,
                max_associations,
                association_idle_timeout_ms.map(Duration::from_millis),
                enabled,
            )
            .await
            .map_err(|error| match error {
                DatagramRuntimeError::Bind(reason) => ControlError::RestartFailed { proxy: name.into(), reason: format!("replacement listener was not bound; existing definition remains active: {reason}") },
                other => map_datagram_error(other),
            })?;
        Ok((view, self.next_generation()))
    }

    /// Delete a fixed-target UDP proxy and join all owned work.
    pub async fn delete_datagram_proxy(&self, name: &str) -> Result<u64, ControlError> {
        if !self
            .runtime
            .datagrams
            .delete_proxy(name)
            .await
            .map_err(map_datagram_error)?
        {
            return Err(ControlError::NotFound(name.to_owned()));
        }
        Ok(self.next_generation())
    }

    /// Read one datagram fault plan snapshot.
    pub async fn get_datagram_plan(
        &self,
        name: &str,
        direction: Direction,
    ) -> Result<(eggchaos_core::DatagramPlan, u64, u64), ControlError> {
        self.runtime
            .datagrams
            .fault_plan(name, direction)
            .await
            .ok_or_else(|| ControlError::NotFound(name.to_owned()))
    }

    /// Publish one datagram plan. Expected generation prevents stale scenario
    /// actions from overwriting a concurrent manual change.
    pub async fn publish_datagram_plan(
        &self,
        name: &str,
        direction: Direction,
        plan: eggchaos_core::DatagramPlan,
        seed_namespace: u64,
        expected_generation: Option<u64>,
    ) -> Result<u64, ControlError> {
        let generation = self
            .runtime
            .datagrams
            .publish_fault_plan(name, direction, plan, seed_namespace, expected_generation)
            .await
            .map_err(map_datagram_error)?;
        self.next_generation();
        Ok(generation)
    }

    /// Add a datagram fault to one direction.
    ///
    /// This is the single HTTP-independent datagram fault mutation
    /// authority shared by the native admin route and `eggchaos-embed`:
    /// same-direction duplicate detection, cross-direction ID uniqueness
    /// (required for path lookup), plan reconstruction, and
    /// generation-guarded publication all live here. Wire DTO conversion
    /// stays with the callers.
    pub async fn add_datagram_fault(
        &self,
        name: &str,
        direction: Direction,
        fault: eggchaos_core::DatagramFaultSpec,
    ) -> Result<(Direction, eggchaos_core::DatagramFaultSpec, u64), ControlError> {
        let (plan, generation, seed) = self.get_datagram_plan(name, direction).await?;
        if plan.faults().iter().any(|existing| existing.id == fault.id) {
            return Err(ControlError::Conflict(
                "datagram fault already exists".into(),
            ));
        }
        let other_direction = match direction {
            Direction::Upstream => Direction::Downstream,
            Direction::Downstream => Direction::Upstream,
        };
        if self
            .get_datagram_plan(name, other_direction)
            .await
            .is_ok_and(|(other, _, _)| {
                other
                    .faults()
                    .iter()
                    .any(|existing| existing.id == fault.id)
            })
        {
            return Err(ControlError::Conflict(
                "datagram fault id must be unique across directions for path lookup".into(),
            ));
        }
        let mut faults = plan.faults().to_vec();
        faults.push(fault.clone());
        let plan = eggchaos_core::DatagramPlan::new(faults)
            .map_err(|error| ControlError::Invalid(error.to_string()))?;
        let next = self
            .publish_datagram_plan(name, direction, plan, seed, Some(generation))
            .await?;
        Ok((direction, fault, next))
    }

    /// Fetch one datagram fault by path ID, searching upstream then
    /// downstream. Reads come from live plan snapshots.
    pub async fn get_datagram_fault(
        &self,
        name: &str,
        id: &str,
    ) -> Option<(Direction, eggchaos_core::DatagramFaultSpec)> {
        for direction in [Direction::Upstream, Direction::Downstream] {
            if let Ok((plan, _, _)) = self.get_datagram_plan(name, direction).await {
                if let Some(fault) = plan.faults().iter().find(|fault| fault.id.as_str() == id) {
                    return Some((direction, fault.clone()));
                }
            }
        }
        None
    }

    /// List both directional datagram fault plans from live snapshots.
    pub async fn list_datagram_faults(
        &self,
        name: &str,
    ) -> Option<(
        Vec<eggchaos_core::DatagramFaultSpec>,
        Vec<eggchaos_core::DatagramFaultSpec>,
    )> {
        let (upstream, _, _) = self
            .get_datagram_plan(name, Direction::Upstream)
            .await
            .ok()?;
        let (downstream, _, _) = self
            .get_datagram_plan(name, Direction::Downstream)
            .await
            .ok()?;
        Some((upstream.faults().to_vec(), downstream.faults().to_vec()))
    }

    /// Update a datagram fault's probability and/or behavior in place,
    /// preserving order. Empty patches are rejected at this semantic
    /// layer, after DTO conversion, so both surfaces agree.
    pub async fn update_datagram_fault(
        &self,
        name: &str,
        id: &str,
        patch: DatagramFaultPatch,
    ) -> Result<(Direction, eggchaos_core::DatagramFaultSpec, u64), ControlError> {
        if patch.probability.is_none() && patch.kind.is_none() {
            return Err(ControlError::Invalid(
                "fault patch must include probability or kind".into(),
            ));
        }
        if let Some(probability) = patch.probability {
            if !(0.0..=1.0).contains(&probability) || !probability.is_finite() {
                return Err(ControlError::Invalid(
                    "probability must be finite and between 0 and 1".into(),
                ));
            }
        }
        for direction in [Direction::Upstream, Direction::Downstream] {
            let Ok((plan, generation, seed)) = self.get_datagram_plan(name, direction).await else {
                continue;
            };
            let Some(mut fault) = plan
                .faults()
                .iter()
                .find(|fault| fault.id.as_str() == id)
                .cloned()
            else {
                continue;
            };
            if let Some(probability) = patch.probability {
                fault.probability = eggchaos_core::Probability::new(probability)
                    .map_err(|error| ControlError::Invalid(error.to_string()))?;
            }
            if let Some(kind) = patch.kind.clone() {
                fault.kind = kind;
            }
            let mut faults = plan.faults().to_vec();
            if let Some(existing) = faults
                .iter_mut()
                .find(|candidate| candidate.id.as_str() == id)
            {
                *existing = fault.clone();
            }
            let updated = eggchaos_core::DatagramPlan::new(faults)
                .map_err(|error| ControlError::Invalid(error.to_string()))?;
            let next = self
                .publish_datagram_plan(name, direction, updated, seed, Some(generation))
                .await?;
            return Ok((direction, fault, next));
        }
        Err(ControlError::NotFound(format!("fault {id} on {name}")))
    }

    /// Remove a datagram fault from either direction. Returns the
    /// direction that owned the fault plus the new generation.
    pub async fn remove_datagram_fault(
        &self,
        name: &str,
        id: &str,
    ) -> Result<(Direction, u64), ControlError> {
        for direction in [Direction::Upstream, Direction::Downstream] {
            let Ok((plan, generation, seed)) = self.get_datagram_plan(name, direction).await else {
                continue;
            };
            if plan.faults().iter().any(|fault| fault.id.as_str() == id) {
                let faults = plan
                    .faults()
                    .iter()
                    .filter(|fault| fault.id.as_str() != id)
                    .cloned()
                    .collect();
                let plan = eggchaos_core::DatagramPlan::new(faults)
                    .map_err(|error| ControlError::Invalid(error.to_string()))?;
                let next = self
                    .publish_datagram_plan(name, direction, plan, seed, Some(generation))
                    .await?;
                return Ok((direction, next));
            }
        }
        Err(ControlError::NotFound(format!("fault {id} on {name}")))
    }

    /// List live datagram associations.
    pub async fn datagram_associations(&self) -> Vec<DatagramAssociationSnapshot> {
        self.runtime.datagrams.all_associations().await
    }

    /// Get active or retained datagram association evidence.
    pub async fn get_datagram_association(&self, id: u64) -> Option<DatagramAssociationSnapshot> {
        self.runtime.datagrams.association(id).await
    }

    /// Administratively terminate an active datagram association.
    pub async fn kill_datagram_association(&self, id: u64) -> bool {
        self.runtime.datagrams.kill_association(id).await
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
        let mut failed_datagram_enables = Vec::new();
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
        // Reset is global: terminate each association, clear both directional
        // plans, and make disabled datagram listeners enabled again.
        let datagram_proxies = self.runtime.datagrams.proxies().await;
        for proxy in datagram_proxies {
            for association in self
                .runtime
                .datagrams
                .associations()
                .await
                .into_iter()
                .filter(|a| a.proxy == proxy.name)
            {
                self.runtime
                    .datagrams
                    .kill_association(association.id)
                    .await;
            }
            for direction in [Direction::Upstream, Direction::Downstream] {
                if let Some((_, generation, seed)) = self
                    .runtime
                    .datagrams
                    .fault_plan(&proxy.name, direction)
                    .await
                {
                    self.runtime
                        .datagrams
                        .publish_fault_plan(
                            &proxy.name,
                            direction,
                            eggchaos_core::DatagramPlan::empty(),
                            seed,
                            Some(generation),
                        )
                        .await
                        .map_err(map_datagram_error)?;
                }
            }
            if !proxy.running
                && self
                    .runtime
                    .datagrams
                    .enable_proxy(&proxy.name)
                    .await
                    .is_err()
            {
                failed_datagram_enables.push(proxy.name);
            }
        }
        Ok(ResetReport {
            generation: self.next_generation(),
            reset: true,
            failed_enables,
            failed_datagram_enables,
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
        self.runtime.datagrams.shutdown().await;
    }

    /// Current upstream/downstream generations for a stream proxy.
    /// Reads the live atomic snapshots without holding a data-plane lock.
    pub async fn current_generations(&self, name: &str) -> Option<(u64, u64)> {
        let (upstream, downstream) = self.snapshot_policies(name).await?;
        Some((upstream.generation, downstream.generation))
    }

    /// Current upstream/downstream generations for a datagram proxy.
    pub async fn datagram_current_generations(&self, name: &str) -> Option<(u64, u64)> {
        let (_, g_up, _) = self
            .runtime
            .datagrams
            .fault_plan(name, Direction::Upstream)
            .await?;
        let (_, g_down, _) = self
            .runtime
            .datagrams
            .fault_plan(name, Direction::Downstream)
            .await?;
        Some((g_up, g_down))
    }

    /// Start an owned v2 schedule run. The source compiles entirely
    /// upfront; a compile failure creates no run and no side effects.
    /// The compiled tape plays through the shared scenario JoinSet with
    /// a child of the service shutdown token, so no v2 task outlives
    /// the service untracked and no second supervisor registry exists.
    pub async fn start_schedule_v2(
        &self,
        source: crate::scenario_v2::ScenarioScheduleV2,
    ) -> Result<crate::scenario_v2::ScenarioScheduleRunRecord, EggchaosError> {
        let compiled = crate::scenario_v2::compile_schedule(&source)
            .map_err(|error| EggchaosError::Control(ControlError::Invalid(error.to_string())))?;
        let fingerprint = crate::scenario_v2::compiled_fingerprint(&compiled);
        let run_id = self.runtime.next_run_id.fetch_add(1, Ordering::AcqRel);
        let record = crate::scenario_v2::ScenarioScheduleRunRecord {
            run_id,
            seed: source.seed,
            execution_key: source.execution_key,
            schedule_fingerprint: fingerprint,
            compiler_semantics_version: compiled.compiler_semantics_version,
            isolation: source.isolation,
            cleanup_policy: source.cleanup,
            status: crate::scenario_v2::ScheduleRunStatus::Pending,
            applied: 0,
            failure: None,
            events: Vec::new(),
            cleanup: None,
        };
        {
            let mut runs = self.runtime.schedule_v2_runs.lock().await;
            let active = runs
                .values()
                .filter(|record| {
                    matches!(
                        record.status,
                        crate::scenario_v2::ScheduleRunStatus::Pending
                            | crate::scenario_v2::ScheduleRunStatus::Running
                            | crate::scenario_v2::ScheduleRunStatus::Cancelling
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
                            crate::scenario_v2::ScheduleRunStatus::Pending
                                | crate::scenario_v2::ScheduleRunStatus::Running
                                | crate::scenario_v2::ScheduleRunStatus::Cancelling
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
            .schedule_v2_tokens
            .lock()
            .await
            .insert(run_id, token.clone());
        self.runtime
            .metrics
            .schedule_v2_runs
            .fetch_add(1, Ordering::Relaxed);
        let state = self.clone();
        self.runtime.scenario_tasks.lock().await.spawn(
            crate::scenario_v2::runtime::drive_schedule_v2_run(state, run_id, compiled, token),
        );
        Ok(record)
    }

    /// Fetch one v2 schedule run record.
    pub async fn get_schedule_v2(
        &self,
        run_id: u64,
    ) -> Option<crate::scenario_v2::ScenarioScheduleRunRecord> {
        self.runtime
            .schedule_v2_runs
            .lock()
            .await
            .get(&run_id)
            .cloned()
    }

    /// Cancel an active v2 schedule run, returning its latest record.
    /// Returns `None` for unknown run IDs. Cancelling a finished run
    /// returns its final record unchanged.
    pub async fn cancel_schedule_v2(
        &self,
        run_id: u64,
    ) -> Option<crate::scenario_v2::ScenarioScheduleRunRecord> {
        let token = self.runtime.schedule_v2_tokens.lock().await.remove(&run_id);
        if let Some(token) = token {
            token.cancel();
            self.update_schedule_v2_run(run_id, |record| {
                if matches!(
                    record.status,
                    crate::scenario_v2::ScheduleRunStatus::Pending
                        | crate::scenario_v2::ScheduleRunStatus::Running
                ) {
                    record.status = crate::scenario_v2::ScheduleRunStatus::Cancelling;
                }
            })
            .await;
        }
        self.get_schedule_v2(run_id).await
    }

    /// Apply a record mutation for a v2 schedule run, if still retained.
    pub async fn update_schedule_v2_run(
        &self,
        run_id: u64,
        update: impl FnOnce(&mut crate::scenario_v2::ScenarioScheduleRunRecord),
    ) {
        if let Some(record) = self.runtime.schedule_v2_runs.lock().await.get_mut(&run_id) {
            update(record);
        }
    }

    /// Append one per-event evidence entry to a v2 run record and bump
    /// the applied count. No-op if the run was already pruned. Coarse
    /// v2 metrics count applied and late events without run, phase, or
    /// fingerprint labels.
    pub async fn append_schedule_v2_event(
        &self,
        run_id: u64,
        event: crate::scenario_v2::ScheduleEventResult,
    ) {
        let late = event.late_by_ns > 0;
        if let Some(record) = self.runtime.schedule_v2_runs.lock().await.get_mut(&run_id) {
            record.applied += 1;
            record.events.push(event);
        }
        self.runtime
            .metrics
            .schedule_v2_events
            .fetch_add(1, Ordering::Relaxed);
        if late {
            self.runtime
                .metrics
                .schedule_v2_late_events
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Drop a finished v2 run's cancellation token.
    pub async fn remove_schedule_v2_token(&self, run_id: u64) {
        self.runtime.schedule_v2_tokens.lock().await.remove(&run_id);
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
