//! Datagram control authority: proxy definitions, listener ownership, the
//! association registry, and bounded history. `DatagramRuntime` is the single
//! mutation authority; association workers and the listener supervisor act
//! through it and never duplicate registry state.
use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, RwLock as StdRwLock,
    },
    time::Duration,
};

use eggchaos_core::{DatagramPlan, Direction};
use tokio::{net::UdpSocket, sync::Mutex as AsyncMutex, task::JoinHandle, time::Instant};
use tokio_util::sync::CancellationToken;

#[cfg(test)]
use super::association::SetupHook;
use super::{
    association::{drain_associations, stop_association, AssociationSlot, IngressReservation},
    model::{
        DatagramAssociationSnapshot, DatagramProxySpec, DatagramProxyView, DatagramRuntimeError,
        DatagramRuntimeLimits, ProxyCounters,
    },
    supervisor::spawn_listener,
};

pub(crate) struct RuntimeInner {
    limits: DatagramRuntimeLimits,
    proxies: AsyncMutex<HashMap<String, ManagedProxy>>,
    active_associations: Arc<AtomicUsize>,
    next_association: Arc<AtomicU64>,
    next_reservation: Arc<AtomicU64>,
    ingress_buffered_bytes: Arc<AtomicUsize>,
    history: Arc<Mutex<VecDeque<DatagramAssociationSnapshot>>>,
}

pub(crate) struct ManagedProxy {
    state: Arc<ProxyState>,
    listener: Option<ListenerOwner>,
    cancel_start: bool,
}

pub(crate) struct ListenerOwner {
    pub(crate) bound: SocketAddr,
    pub(crate) cancel: CancellationToken,
    pub(crate) task: JoinHandle<()>,
    pub(crate) drain_on_exit: Arc<std::sync::atomic::AtomicBool>,
}

pub(crate) struct ProxyState {
    pub(crate) spec: StdRwLock<DatagramProxySpec>,
    pub(crate) capacity_mutation: Mutex<()>,
    pub(crate) policy_mutation: Mutex<()>,
    pub(crate) associations: StdRwLock<HashMap<SocketAddr, AssociationSlot>>,
    pub(crate) global_active: Arc<AtomicUsize>,
    pub(crate) proxy_active: Arc<AtomicUsize>,
    pub(crate) next_id: Arc<AtomicU64>,
    pub(crate) next_reservation: Arc<AtomicU64>,
    pub(crate) ingress_buffered_bytes: Arc<AtomicUsize>,
    pub(crate) counters: Arc<ProxyCounters>,
    pub(crate) limits: DatagramRuntimeLimits,
    pub(crate) history: Arc<Mutex<VecDeque<DatagramAssociationSnapshot>>>,
    #[cfg(test)]
    pub(crate) setup_hook: Mutex<Option<Arc<SetupHook>>>,
}
/// Independent fixed-target UDP listener and association runtime.
#[derive(Clone)]
pub struct DatagramRuntime {
    inner: Arc<RuntimeInner>,
}

impl DatagramRuntime {
    /// Create a bounded runtime without listeners.
    pub fn new(limits: DatagramRuntimeLimits) -> Result<Self, DatagramRuntimeError> {
        let limits = limits.validate()?;
        Ok(Self {
            inner: Arc::new(RuntimeInner {
                limits,
                proxies: AsyncMutex::new(HashMap::new()),
                active_associations: Arc::new(AtomicUsize::new(0)),
                next_association: Arc::new(AtomicU64::new(1)),
                next_reservation: Arc::new(AtomicU64::new(1)),
                ingress_buffered_bytes: Arc::new(AtomicUsize::new(0)),
                history: Arc::new(Mutex::new(VecDeque::new())),
            }),
        })
    }

    #[cfg(test)]
    pub(crate) async fn state_for_test(&self, name: &str) -> Option<Arc<ProxyState>> {
        self.inner
            .proxies
            .lock()
            .await
            .get(name)
            .map(|managed| managed.state.clone())
    }

    #[cfg(test)]
    pub(crate) async fn install_setup_hook(&self, name: &str, hook: SetupHook) {
        let proxies = self.inner.proxies.lock().await;
        proxies
            .get(name)
            .expect("datagram proxy")
            .state
            .setup_hook
            .lock()
            .expect("setup hook")
            .replace(Arc::new(hook));
    }

    /// Bind and start one fixed-target proxy before reporting success.
    pub async fn create_proxy(
        &self,
        spec: DatagramProxySpec,
    ) -> Result<DatagramProxyView, DatagramRuntimeError> {
        spec.validate(self.inner.limits.max_associations)?;
        {
            let proxies = self.inner.proxies.lock().await;
            if proxies.contains_key(&spec.name) {
                return Err(DatagramRuntimeError::Duplicate(spec.name));
            }
            if proxies.len() >= self.inner.limits.max_proxies {
                return Err(DatagramRuntimeError::Invalid(
                    "proxy definition limit reached".into(),
                ));
            }
        }
        let socket = Arc::new(
            UdpSocket::bind(spec.listen)
                .await
                .map_err(DatagramRuntimeError::Bind)?,
        );
        let bound = socket.local_addr().map_err(DatagramRuntimeError::Bind)?;
        let state = Arc::new(ProxyState {
            spec: StdRwLock::new(spec.clone()),
            capacity_mutation: Mutex::new(()),
            policy_mutation: Mutex::new(()),
            associations: StdRwLock::new(HashMap::new()),
            global_active: self.inner.active_associations.clone(),
            proxy_active: Arc::new(AtomicUsize::new(0)),
            next_id: self.inner.next_association.clone(),
            next_reservation: self.inner.next_reservation.clone(),
            ingress_buffered_bytes: self.inner.ingress_buffered_bytes.clone(),
            counters: Arc::new(ProxyCounters::default()),
            limits: self.inner.limits,
            history: self.inner.history.clone(),
            #[cfg(test)]
            setup_hook: Mutex::new(None),
        });
        // Insert-then-spawn: the proxy is visible before its listener can
        // receive, so no datagram arrives while the proxy is invisible and
        // no duplicate race orphans history.
        {
            let mut proxies = self.inner.proxies.lock().await;
            if proxies.contains_key(&spec.name) {
                return Err(DatagramRuntimeError::Duplicate(spec.name));
            }
            if proxies.len() >= self.inner.limits.max_proxies {
                return Err(DatagramRuntimeError::Invalid(
                    "proxy definition limit reached".into(),
                ));
            }
            proxies.insert(
                spec.name.clone(),
                ManagedProxy {
                    state: state.clone(),
                    listener: None,
                    cancel_start: false,
                },
            );
        }
        let owner = spawn_listener(socket, state.clone());
        let mut running = true;
        {
            let mut proxies = self.inner.proxies.lock().await;
            if let Some(managed) = proxies.get_mut(&spec.name) {
                if managed.cancel_start {
                    running = false;
                    owner.drain_on_exit.store(false, Ordering::Release);
                    owner.cancel.cancel();
                    if let Err(error) = owner.task.await {
                        tracing::warn!("datagram listener supervisor join failed: {error}");
                    }
                } else {
                    managed.listener = Some(owner);
                }
            } else {
                // Proxy vanished between insert and spawn: stop the orphan.
                owner.cancel.cancel();
                if let Err(error) = owner.task.await {
                    tracing::warn!("datagram listener supervisor join failed: {error}");
                }
                return Err(DatagramRuntimeError::NotFound(spec.name));
            }
        }
        Ok(proxy_view(&state, running.then_some(bound), running).await)
    }

    /// Stop a listener and its associations while retaining the definition.
    pub async fn disable_proxy(&self, name: &str) -> Result<(), DatagramRuntimeError> {
        let (owner, state) = {
            let mut proxies = self.inner.proxies.lock().await;
            let managed = proxies
                .get_mut(name)
                .ok_or_else(|| DatagramRuntimeError::NotFound(name.into()))?;
            if managed.listener.is_none() {
                managed.cancel_start = true;
            }
            (managed.listener.take(), managed.state.clone())
        };
        if let Some(owner) = owner {
            owner.cancel.cancel();
            if let Err(error) = owner.task.await {
                tracing::warn!("datagram listener supervisor join failed: {error}");
            }
        }
        for association in drain_associations(&state, false).await {
            stop_association(&state, association, true).await;
        }
        Ok(())
    }

    /// Restart a disabled proxy listener, binding before changing visible state.
    pub async fn enable_proxy(
        &self,
        name: &str,
    ) -> Result<DatagramProxyView, DatagramRuntimeError> {
        let (state, listen) = {
            let mut proxies = self.inner.proxies.lock().await;
            let managed = proxies
                .get_mut(name)
                .ok_or_else(|| DatagramRuntimeError::NotFound(name.into()))?;
            if managed.listener.is_some() {
                return Ok(proxy_view(
                    &managed.state,
                    managed.listener.as_ref().map(|x| x.bound),
                    true,
                )
                .await);
            }
            let listen = managed
                .state
                .spec
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .listen;
            managed.cancel_start = false;
            (managed.state.clone(), listen)
        };
        let socket = Arc::new(
            UdpSocket::bind(listen)
                .await
                .map_err(DatagramRuntimeError::Bind)?,
        );
        let bound = socket.local_addr().map_err(DatagramRuntimeError::Bind)?;
        let owner = spawn_listener(socket, state.clone());
        let mut proxies = self.inner.proxies.lock().await;
        let managed = proxies
            .get_mut(name)
            .ok_or_else(|| DatagramRuntimeError::NotFound(name.into()))?;
        if managed.listener.is_some() {
            owner.drain_on_exit.store(false, Ordering::Release);
            owner.cancel.cancel();
            if let Err(error) = owner.task.await {
                tracing::warn!("datagram listener supervisor join failed: {error}");
            }
            return Ok(proxy_view(
                &managed.state,
                managed.listener.as_ref().map(|x| x.bound),
                true,
            )
            .await);
        }
        if managed.cancel_start {
            owner.drain_on_exit.store(false, Ordering::Release);
            owner.cancel.cancel();
            if let Err(error) = owner.task.await {
                tracing::warn!("datagram listener supervisor join failed: {error}");
            }
            return Err(DatagramRuntimeError::Conflict(
                "datagram proxy was disabled while enabling".into(),
            ));
        }
        managed.listener = Some(owner);
        Ok(proxy_view(&state, Some(bound), true).await)
    }

    /// Remove the definition, stop the listener, and join every association task.
    pub async fn delete_proxy(&self, name: &str) -> Result<bool, DatagramRuntimeError> {
        let Some(mut managed) = self.inner.proxies.lock().await.remove(name) else {
            return Ok(false);
        };
        if let Some(owner) = managed.listener.take() {
            owner.cancel.cancel();
            if let Err(error) = owner.task.await {
                tracing::warn!("datagram listener supervisor join failed: {error}");
            }
        }
        for association in drain_associations(&managed.state, false).await {
            stop_association(&managed.state, association, true).await;
        }
        Ok(true)
    }

    /// Current proxy definitions and actual listener state.
    pub async fn proxies(&self) -> Vec<DatagramProxyView> {
        let proxies = self.inner.proxies.lock().await;
        let mut views = Vec::with_capacity(proxies.len());
        for managed in proxies.values() {
            views.push(
                proxy_view(
                    &managed.state,
                    managed.listener.as_ref().map(|x| x.bound),
                    managed.listener.is_some(),
                )
                .await,
            );
        }
        views.sort_by(|a, b| a.name.cmp(&b.name));
        views
    }

    /// Snapshot one named proxy.
    pub async fn proxy(&self, name: &str) -> Option<DatagramProxyView> {
        let proxies = self.inner.proxies.lock().await;
        let managed = proxies.get(name)?;
        Some(
            proxy_view(
                &managed.state,
                managed.listener.as_ref().map(|owner| owner.bound),
                managed.listener.is_some(),
            )
            .await,
        )
    }

    /// Update listener/target/lifecycle settings. Address changes bind a
    /// replacement listener before stopping the current one; target changes
    /// retain the listener but cancel existing associations so only future
    /// associations use the new fixed target.
    // M059: single bind-before-stop update transaction; splitting would fork
    // the prebind/stop/publish sequencing.
    #[allow(clippy::too_many_lines)]
    pub async fn update_proxy(
        &self,
        name: &str,
        listen: Option<SocketAddr>,
        upstream: Option<SocketAddr>,
        max_associations: Option<usize>,
        association_idle_timeout: Option<Duration>,
        enabled: Option<bool>,
    ) -> Result<DatagramProxyView, DatagramRuntimeError> {
        let (state, running, mut next) = {
            let proxies = self.inner.proxies.lock().await;
            let managed = proxies
                .get(name)
                .ok_or_else(|| DatagramRuntimeError::NotFound(name.into()))?;
            let next = managed
                .state
                .spec
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            (managed.state.clone(), managed.listener.is_some(), next)
        };
        let old = state
            .spec
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(listen) = listen {
            next.listen = listen;
        }
        if let Some(upstream) = upstream {
            next.upstream = upstream;
        }
        if let Some(max_associations) = max_associations {
            next.max_associations = max_associations;
        }
        if let Some(timeout) = association_idle_timeout {
            next.association_idle_timeout = timeout;
        }
        next.validate(self.inner.limits.max_associations)?;
        if state.proxy_active.load(Ordering::Acquire) > next.max_associations {
            return Err(DatagramRuntimeError::Invalid(
                "max_associations cannot be lower than the current active count".into(),
            ));
        }
        let listen_changed = next.listen != old.listen;
        let upstream_changed = next.upstream != old.upstream;
        let desired_running = enabled.unwrap_or(running);
        let prebound = if desired_running && (!running || listen_changed) {
            Some(Arc::new(
                UdpSocket::bind(next.listen)
                    .await
                    .map_err(DatagramRuntimeError::Bind)?,
            ))
        } else {
            None
        };
        let bound = if let Some(socket) = &prebound {
            Some(socket.local_addr().map_err(DatagramRuntimeError::Bind)?)
        } else {
            None
        };
        let must_stop_old = running && (!desired_running || listen_changed);
        if must_stop_old && state.proxy_active.load(Ordering::Acquire) > next.max_associations {
            return Err(DatagramRuntimeError::Invalid(
                "max_associations cannot be lower than the current active count".into(),
            ));
        }
        let old_owner = if must_stop_old {
            let mut proxies = self.inner.proxies.lock().await;
            proxies
                .get_mut(name)
                .and_then(|managed| managed.listener.take())
        } else {
            None
        };
        if let Some(owner) = old_owner {
            owner.cancel.cancel();
            if let Err(error) = owner.task.await {
                tracing::warn!("datagram listener supervisor join failed: {error}");
            }
        }
        if must_stop_old {
            let current = state
                .spec
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if current.listen != old.listen
                || current.upstream != old.upstream
                || current.max_associations != old.max_associations
                || current.association_idle_timeout != old.association_idle_timeout
            {
                return Err(DatagramRuntimeError::Conflict(
                    "datagram proxy changed concurrently".into(),
                ));
            }
            {
                let _capacity = state
                    .capacity_mutation
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if state.proxy_active.load(Ordering::Acquire) > next.max_associations {
                    return Err(DatagramRuntimeError::Invalid(
                        "max_associations cannot be lower than the current active count".into(),
                    ));
                }
                *state
                    .spec
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = next;
            }
            for association in drain_associations(&state, false).await {
                stop_association(&state, association, true).await;
            }
        } else {
            // The `Sync` association registry's read guard is `!Send`, so
            // the guard must not cross the below `drain_associations().await`.
            // Acquire the guard, validate the cap, write the spec, then drop.
            {
                let _associations = state
                    .associations
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                // Re-check under the write path: reject concurrent updates
                // that changed the spec since the stale pre-validation clone
                // instead of last-writer-wins overwriting them.
                let current = state
                    .spec
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                if current.listen != old.listen
                    || current.upstream != old.upstream
                    || current.max_associations != old.max_associations
                    || current.association_idle_timeout != old.association_idle_timeout
                {
                    return Err(DatagramRuntimeError::Conflict(
                        "datagram proxy changed concurrently".into(),
                    ));
                }
                if state.proxy_active.load(Ordering::Acquire) > next.max_associations {
                    return Err(DatagramRuntimeError::Invalid(
                        "max_associations cannot be lower than the current active count".into(),
                    ));
                }
                let _capacity = state
                    .capacity_mutation
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if state.proxy_active.load(Ordering::Acquire) > next.max_associations {
                    return Err(DatagramRuntimeError::Invalid(
                        "max_associations cannot be lower than the current active count".into(),
                    ));
                }
                *state
                    .spec
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = next;
            }
            if upstream_changed {
                for association in drain_associations(&state, running).await {
                    stop_association(&state, association, true).await;
                }
            }
        }
        if let Some(socket) = prebound {
            let owner = spawn_listener(socket, state.clone());
            let mut proxies = self.inner.proxies.lock().await;
            let managed = proxies
                .get_mut(name)
                .ok_or_else(|| DatagramRuntimeError::NotFound(name.into()));
            let Some(managed) = managed.ok() else {
                owner.drain_on_exit.store(false, Ordering::Release);
                owner.cancel.cancel();
                if let Err(error) = owner.task.await {
                    tracing::warn!("datagram listener supervisor join failed: {error}");
                }
                return Err(DatagramRuntimeError::NotFound(name.into()));
            };
            managed.listener = Some(owner);
        }
        let proxies = self.inner.proxies.lock().await;
        let managed = proxies
            .get(name)
            .ok_or_else(|| DatagramRuntimeError::NotFound(name.into()))?;
        Ok(proxy_view(
            &state,
            bound.or_else(|| managed.listener.as_ref().map(|owner| owner.bound)),
            managed.listener.is_some(),
        )
        .await)
    }

    /// Read a directional plan and its generation/namespace as one snapshot.
    pub async fn fault_plan(
        &self,
        name: &str,
        direction: Direction,
    ) -> Option<(DatagramPlan, u64, u64)> {
        let proxies = self.inner.proxies.lock().await;
        let state = &proxies.get(name)?.state;
        let spec = state
            .spec
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let snapshot = match direction {
            Direction::Upstream => spec.upstream_policy.snapshot(),
            Direction::Downstream => spec.downstream_policy.snapshot(),
        };
        Some((
            (*snapshot.plan).clone(),
            snapshot.generation,
            snapshot.seed_namespace,
        ))
    }

    /// Publish one directional plan with optional expected-generation guard.
    pub async fn publish_fault_plan(
        &self,
        name: &str,
        direction: Direction,
        plan: DatagramPlan,
        seed_namespace: u64,
        expected_generation: Option<u64>,
    ) -> Result<u64, DatagramRuntimeError> {
        plan.validate()
            .map_err(|error| DatagramRuntimeError::Invalid(error.into()))?;
        let proxies = self.inner.proxies.lock().await;
        let state = &proxies
            .get(name)
            .ok_or_else(|| DatagramRuntimeError::NotFound(name.into()))?
            .state;
        let _guard = state
            .policy_mutation
            .lock()
            .expect("datagram policy mutation");
        let spec = state
            .spec
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let policy = match direction {
            Direction::Upstream => &spec.upstream_policy,
            Direction::Downstream => &spec.downstream_policy,
        };
        let current = policy.snapshot();
        if expected_generation.is_some_and(|expected| expected != current.generation) {
            return Err(DatagramRuntimeError::Conflict(format!(
                "generation moved from {} to {}",
                expected_generation.unwrap(),
                current.generation
            )));
        }
        policy
            .publish(plan, seed_namespace)
            .map(|snapshot| snapshot.generation)
            .map_err(|error| DatagramRuntimeError::Invalid(error.into()))
    }

    /// Active association snapshots. Associations still in UDP setup are
    /// omitted; they have no evidence yet and appear on their first snapshot
    /// poll after publication.
    pub async fn associations(&self) -> Vec<DatagramAssociationSnapshot> {
        let proxies = self.inner.proxies.lock().await;
        let mut views = Vec::new();
        for managed in proxies.values() {
            let associations = managed
                .state
                .associations
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for association in associations.values().filter_map(AssociationSlot::active) {
                views.push(
                    association
                        .record
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .snapshot(Instant::now()),
                );
            }
        }
        views.sort_by_key(|x| x.id);
        views
    }

    /// Active plus retained completed association evidence, newest last.
    pub async fn all_associations(&self) -> Vec<DatagramAssociationSnapshot> {
        let mut views = self
            .inner
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        views.extend(self.associations().await);
        views.sort_by_key(|view| view.id);
        views
    }

    /// Active or retained association snapshot.
    pub async fn association(&self, id: u64) -> Option<DatagramAssociationSnapshot> {
        let proxies = self.inner.proxies.lock().await;
        for managed in proxies.values() {
            let associations = managed
                .state
                .associations
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(association) = associations
                .values()
                .filter_map(AssociationSlot::active)
                .find(|x| x.id == id)
            {
                return Some(
                    association
                        .record
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .snapshot(Instant::now()),
                );
            }
        }
        self.inner
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .find(|x| x.id == id)
            .cloned()
    }

    /// Explicitly cancel one live association and record queued
    /// administrative discards. Associations still in UDP setup have no
    /// stable id yet; a kill that races setup resolves against the published
    /// association on retry.
    pub async fn kill_association(&self, id: u64) -> bool {
        let states: Vec<_> = self
            .inner
            .proxies
            .lock()
            .await
            .values()
            .map(|managed| managed.state.clone())
            .collect();
        for state in states {
            let found = {
                let mut associations = state
                    .associations
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let key = associations.iter().find_map(|(key, slot)| {
                    matches!(slot, AssociationSlot::Active(association) if association.id == id)
                        .then_some(*key)
                });
                key.and_then(|key| associations.remove(&key))
            };
            if let Some(AssociationSlot::Active(association)) = found {
                stop_association(&state, association, true).await;
                return true;
            }
        }
        false
    }

    /// Stop every listener and join all association tasks owned by this runtime.
    pub async fn shutdown(&self) {
        let names: Vec<_> = self.inner.proxies.lock().await.keys().cloned().collect();
        for name in names {
            let _ = self.delete_proxy(&name).await;
        }
    }
}
// M059: `async` retained so view call sites share one await shape even
// though the body is currently lock- and snapshot-only. Millisecond
// snapshots saturate at `u64::MAX`; truncation is the documented bound.
#[allow(clippy::unused_async, clippy::cast_possible_truncation)]
async fn proxy_view(
    state: &ProxyState,
    bound: Option<SocketAddr>,
    running: bool,
) -> DatagramProxyView {
    let spec = state
        .spec
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    DatagramProxyView {
        name: spec.name.clone(),
        listen: spec.listen,
        bound_addr: bound,
        upstream: spec.upstream,
        running,
        max_associations: spec.max_associations,
        association_idle_timeout_ms: spec
            .association_idle_timeout
            .as_millis()
            .min(u128::from(u64::MAX)) as u64,
        max_datagram_size: spec.queue_limits.max_datagram_bytes.get(),
        max_queued_datagrams: spec.queue_limits.max_queued_datagrams.get(),
        max_queued_bytes: spec.queue_limits.max_queued_bytes.get(),
        seed: spec.seed,
        active_associations: state
            .associations
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .filter(|slot| matches!(slot, AssociationSlot::Active(_)))
            .count(),
        upstream_generation: spec.upstream_policy.snapshot().generation,
        downstream_generation: spec.downstream_policy.snapshot().generation,
        oversize_datagrams: state.counters.oversize_datagrams.load(Ordering::Relaxed),
        association_capacity_rejections: state.counters.capacity_rejections.load(Ordering::Relaxed),
        ingress_queue_overflow: state.counters.ingress_overflow.load(Ordering::Relaxed),
        association_setup_failures: state
            .counters
            .association_setup_failures
            .load(Ordering::Relaxed),
    }
}
pub(crate) fn reserve_ingress(
    counter: Arc<AtomicUsize>,
    bytes: usize,
    limit: usize,
) -> Option<IngressReservation> {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current.checked_add(bytes).filter(|next| *next <= limit)
        })
        .ok()?;
    Some(IngressReservation { counter, bytes })
}
