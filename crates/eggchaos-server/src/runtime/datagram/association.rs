//! Datagram association lifecycle: setup reservation/publication, the
//! per-association worker loop, batched egress accounting, and teardown. The
//! registry lock is never held across UDP socket setup; see
//! [`resolve_association`].
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use bytes::Bytes;
use eggchaos_core::{
    DatagramDirectionEngine, DatagramEvidence, DatagramScheduled, Direction, RngVersion,
};
use tokio::{
    net::UdpSocket,
    sync::{mpsc, watch, Mutex as AsyncMutex},
    task::JoinHandle,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use super::{
    model::{AssociationRecord, DatagramProxySpec, DatagramRuntimeError, ProxyCounters},
    registry::ProxyState,
    UDP_RECEIVE_BUFFER_BYTES,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SetupOutcome {
    Starting,
    Published,
    Abandoned { retryable: bool },
}

impl SetupOutcome {
    fn terminal(self) -> bool {
        matches!(
            self,
            SetupOutcome::Published | SetupOutcome::Abandoned { .. }
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SetupTransition {
    reservation_id: u64,
    version: u64,
    outcome: SetupOutcome,
}

pub(crate) struct StartingReservation {
    id: u64,
    events: watch::Sender<SetupTransition>,
    version: AtomicU64,
    terminal: AtomicBool,
    capacity: Arc<CapacityLease>,
    #[cfg(test)]
    waiters: watch::Sender<usize>,
}

impl StartingReservation {
    fn new(id: u64, capacity: Arc<CapacityLease>) -> Arc<Self> {
        let (events, _) = watch::channel(SetupTransition {
            reservation_id: id,
            version: 0,
            outcome: SetupOutcome::Starting,
        });
        Arc::new(Self {
            id,
            events,
            version: AtomicU64::new(1),
            terminal: AtomicBool::new(false),
            capacity,
            #[cfg(test)]
            waiters: watch::Sender::new(0),
        })
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    fn subscribe(&self) -> watch::Receiver<SetupTransition> {
        #[cfg(test)]
        {
            let count = self.waiters.borrow().saturating_add(1);
            self.waiters.send_replace(count);
        }
        self.events.subscribe()
    }

    fn transition(&self, outcome: SetupOutcome) {
        if self.terminal.swap(true, Ordering::AcqRel) {
            return;
        }
        let version = self.version.fetch_add(1, Ordering::Relaxed);
        self.events.send_replace(SetupTransition {
            reservation_id: self.id,
            version,
            outcome,
        });
    }

    fn is_terminal(&self) -> bool {
        self.terminal.load(Ordering::Acquire)
    }

    fn capacity(&self) -> &Arc<CapacityLease> {
        &self.capacity
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_waiters(&self, count: usize) {
        let mut receiver = self.waiters.subscribe();
        receiver
            .wait_for(|value| *value >= count)
            .await
            .expect("starting reservation waiter channel");
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_terminal(&self) {
        let mut receiver = self.events.subscribe();
        receiver
            .wait_for(|transition| transition.version > 0 && transition.outcome.terminal())
            .await
            .expect("starting reservation transition channel");
    }
}

pub(crate) struct CapacityLease {
    global: Arc<AtomicUsize>,
    proxy: Arc<AtomicUsize>,
    released: AtomicBool,
}

impl CapacityLease {
    fn try_acquire(state: &ProxyState, proxy_limit: usize) -> Option<Arc<Self>> {
        state
            .proxy_active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1).filter(|next| *next <= proxy_limit)
            })
            .ok()?;
        if state
            .global_active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(1)
                    .filter(|next| *next <= state.limits.max_associations)
            })
            .is_err()
        {
            state.proxy_active.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Arc::new(Self {
            global: state.global_active.clone(),
            proxy: state.proxy_active.clone(),
            released: AtomicBool::new(false),
        }))
    }

    fn release(&self) {
        if !self.released.swap(true, Ordering::AcqRel) {
            self.global.fetch_sub(1, Ordering::AcqRel);
            self.proxy.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

impl Drop for CapacityLease {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct SetupTestGate {
    enabled: Arc<AtomicBool>,
    entered: watch::Sender<bool>,
    release: watch::Sender<bool>,
}

#[cfg(test)]
impl SetupTestGate {
    fn new() -> Self {
        let (entered, _) = watch::channel(false);
        let (release, _) = watch::channel(false);
        Self {
            enabled: Arc::new(AtomicBool::new(false)),
            entered,
            release,
        }
    }

    pub(crate) fn hold(&self) {
        self.enabled.store(true, Ordering::Release);
    }

    pub(crate) async fn wait_entered(&self) {
        if !self.enabled.load(Ordering::Acquire) {
            return;
        }
        let mut receiver = self.entered.subscribe();
        receiver
            .wait_for(|entered| *entered)
            .await
            .expect("setup gate entry channel");
    }

    pub(crate) fn release(&self) {
        self.release.send_replace(true);
    }

    async fn pause(&self) {
        if !self.enabled.load(Ordering::Acquire) {
            return;
        }
        self.entered.send_replace(true);
        let mut receiver = self.release.subscribe();
        receiver
            .wait_for(|released| *released)
            .await
            .expect("setup gate release channel");
    }
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct SetupHook {
    before_bind: SetupTestGate,
    before_publish: SetupTestGate,
    fail_next: Arc<AtomicBool>,
}

#[cfg(test)]
impl SetupHook {
    pub(crate) fn new() -> Self {
        Self {
            before_bind: SetupTestGate::new(),
            before_publish: SetupTestGate::new(),
            fail_next: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn hold_before_bind(&self) {
        self.before_bind.hold();
    }

    pub(crate) async fn wait_before_bind(&self) {
        self.before_bind.wait_entered().await;
    }

    pub(crate) fn release_before_bind(&self) {
        self.before_bind.release();
    }

    pub(crate) fn hold_before_publish(&self) {
        self.before_publish.hold();
    }

    pub(crate) async fn wait_before_publish(&self) {
        self.before_publish.wait_entered().await;
    }

    pub(crate) fn release_before_publish(&self) {
        self.before_publish.release();
    }

    pub(crate) fn fail_next(&self) {
        self.fail_next.store(true, Ordering::Release);
    }

    fn take_failure(&self) -> bool {
        self.fail_next.swap(false, Ordering::AcqRel)
    }
}

/// Registry entry for one client address. `Starting` reserves global and
/// per-proxy capacity while UDP bind/connect runs without holding the
/// registry lock; exactly one of the racing creators owns setup and every
/// other first-datagram waiter either observes the published `Active`
/// association or takes over creation if setup was abandoned.
#[derive(Clone)]
pub(crate) enum AssociationSlot {
    Starting {
        reservation: Arc<StartingReservation>,
    },
    Active(Arc<Association>),
}

impl AssociationSlot {
    pub(crate) fn active(&self) -> Option<Arc<Association>> {
        match self {
            AssociationSlot::Active(association) => Some(association.clone()),
            AssociationSlot::Starting { .. } => None,
        }
    }
}

pub(crate) struct WorkerHandle {
    handle: Option<JoinHandle<()>>,
}

impl WorkerHandle {
    fn new(handle: JoinHandle<()>) -> Self {
        Self {
            handle: Some(handle),
        }
    }

    async fn join(&mut self) {
        if let Some(handle) = self.handle.as_mut() {
            let _ = handle.await;
        }
        self.handle = None;
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

pub(crate) struct Association {
    pub(crate) id: u64,
    pub(crate) client: SocketAddr,
    pub(crate) sender: mpsc::Sender<Ingress>,
    pub(crate) pending_ingress: AtomicUsize,
    pub(crate) administrative_cancel: AtomicUsize,
    pub(crate) cancel: CancellationToken,
    pub(crate) worker: AsyncMutex<Option<WorkerHandle>>,
    pub(crate) capacity: Arc<CapacityLease>,
    pub(crate) record: Arc<Mutex<AssociationRecord>>,
}

pub(crate) struct Ingress {
    pub(crate) payload: Bytes,
    pub(crate) policy: Arc<eggchaos_core::PublishedDatagramPolicy>,
    pub(crate) _reservation: IngressReservation,
}

pub(crate) struct IngressReservation {
    pub(crate) counter: Arc<AtomicUsize>,
    pub(crate) bytes: usize,
}

impl Drop for IngressReservation {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
struct AssociationSendContext<'a> {
    upstream_socket: &'a UdpSocket,
    listener_socket: &'a UdpSocket,
    client: SocketAddr,
    record: &'a Arc<Mutex<AssociationRecord>>,
    cancel: &'a CancellationToken,
}
fn same_reservation_slot(
    slot: Option<&AssociationSlot>,
    expected: &Arc<StartingReservation>,
) -> bool {
    matches!(
        slot,
        Some(AssociationSlot::Starting { reservation: current })
            if current.id() == expected.id() && Arc::ptr_eq(current, expected)
    )
}

struct SetupOwnerGuard {
    state: Arc<ProxyState>,
    client: SocketAddr,
    reservation: Arc<StartingReservation>,
    armed: bool,
}

impl SetupOwnerGuard {
    fn new(
        state: Arc<ProxyState>,
        client: SocketAddr,
        reservation: Arc<StartingReservation>,
    ) -> Self {
        Self {
            state,
            client,
            reservation,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for SetupOwnerGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Ok(mut associations) = self.state.associations.try_write() {
            if same_reservation_slot(associations.get(&self.client), &self.reservation) {
                associations.remove(&self.client);
            }
            self.reservation
                .transition(SetupOutcome::Abandoned { retryable: true });
            self.reservation.capacity().release();
            return;
        }
        self.reservation
            .transition(SetupOutcome::Abandoned { retryable: true });
        self.reservation.capacity().release();
    }
}

enum ResolveStep {
    Setup {
        spec: DatagramProxySpec,
        reservation: Arc<StartingReservation>,
    },
    Wait {
        reservation_id: u64,
        version: u64,
        receiver: watch::Receiver<SetupTransition>,
    },
    Retry,
}

pub(crate) async fn resolve_association(
    socket: Arc<UdpSocket>,
    state: &Arc<ProxyState>,
    client: SocketAddr,
) -> Result<Arc<Association>, DatagramRuntimeError> {
    loop {
        // Hot path: a read-locked lookup avoids write-lock acquisition
        // for the common already-active case. The miss path re-checks
        // under the write guard, so the optimistic read cannot admit a
        // second setup owner.
        if let Some(association) = state
            .associations
            .read()
            .expect("datagram association registry")
            .get(&client)
            .and_then(AssociationSlot::active)
        {
            return Ok(association);
        }
        let step = {
            let mut associations = state
                .associations
                .write()
                .expect("datagram association registry");
            match associations.get(&client) {
                Some(AssociationSlot::Active(association)) => return Ok(association.clone()),
                Some(AssociationSlot::Starting { reservation }) if reservation.is_terminal() => {
                    if same_reservation_slot(associations.get(&client), reservation) {
                        associations.remove(&client);
                    }
                    ResolveStep::Retry
                }
                Some(AssociationSlot::Starting { reservation }) => ResolveStep::Wait {
                    reservation_id: reservation.id(),
                    version: 0,
                    receiver: reservation.subscribe(),
                },
                None => {
                    let spec = state.spec.read().expect("datagram proxy spec").clone();
                    let capacity = CapacityLease::try_acquire(state, spec.max_associations)
                        .ok_or(DatagramRuntimeError::AssociationLimit)?;
                    let reservation = StartingReservation::new(
                        state.next_reservation.fetch_add(1, Ordering::Relaxed),
                        capacity,
                    );
                    associations.insert(
                        client,
                        AssociationSlot::Starting {
                            reservation: reservation.clone(),
                        },
                    );
                    ResolveStep::Setup { spec, reservation }
                }
            }
        };
        match step {
            ResolveStep::Retry => continue,
            ResolveStep::Wait {
                reservation_id,
                version,
                mut receiver,
            } => {
                let outcome = receiver
                    .wait_for(|transition| {
                        transition.reservation_id == reservation_id
                            && transition.version > version
                            && transition.outcome.terminal()
                    })
                    .await
                    .map(|transition| transition.outcome)
                    .map_err(|_| {
                        DatagramRuntimeError::Conflict("association setup waiter closed".into())
                    })?;
                if matches!(outcome, SetupOutcome::Abandoned { retryable: false }) {
                    return Err(DatagramRuntimeError::Conflict(
                        "association setup drained".into(),
                    ));
                }
            }
            ResolveStep::Setup { spec, reservation } => {
                return publish_association(socket, state, client, spec, reservation).await;
            }
        }
    }
}

async fn publish_association(
    socket: Arc<UdpSocket>,
    state: &Arc<ProxyState>,
    client: SocketAddr,
    spec: DatagramProxySpec,
    reservation: Arc<StartingReservation>,
) -> Result<Arc<Association>, DatagramRuntimeError> {
    let mut owner_guard = SetupOwnerGuard::new(state.clone(), client, reservation.clone());
    #[cfg(test)]
    let setup_hook = state.setup_hook.lock().expect("setup hook").clone();
    #[cfg(test)]
    if let Some(hook) = setup_hook.as_ref() {
        hook.before_bind.pause().await;
        if hook.take_failure() {
            abandon_starting(state, client, &reservation, true).await;
            owner_guard.disarm();
            return Err(DatagramRuntimeError::Bind(std::io::Error::other(
                "injected datagram setup failure",
            )));
        }
    }
    let local = match spec.upstream.ip() {
        IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    };
    let upstream = match UdpSocket::bind(local).await {
        Ok(socket) => socket,
        Err(error) => {
            abandon_starting(state, client, &reservation, true).await;
            owner_guard.disarm();
            return Err(DatagramRuntimeError::Bind(error));
        }
    };
    if let Err(error) = upstream.connect(spec.upstream).await {
        abandon_starting(state, client, &reservation, true).await;
        owner_guard.disarm();
        return Err(DatagramRuntimeError::Bind(error));
    }
    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let now = Instant::now();
    let record = Arc::new(Mutex::new(AssociationRecord {
        id,
        proxy: spec.name.clone(),
        client,
        upstream: spec.upstream,
        created: now,
        last_activity: now,
        ingress_datagrams: 0,
        ingress_bytes: 0,
        egress_datagrams: 0,
        egress_bytes: 0,
        ingress_queue_overflow: 0,
        oversize_datagrams: 0,
        administrative_discards: 0,
        send_errors: 0,
        upstream_evidence: DatagramEvidence::default(),
        downstream_evidence: DatagramEvidence::default(),
    }));
    let (sender, receiver) = mpsc::channel(state.limits.ingress_per_association);
    let cancel = CancellationToken::new();
    let association = Arc::new(Association {
        id,
        client,
        sender,
        pending_ingress: AtomicUsize::new(0),
        administrative_cancel: AtomicUsize::new(0),
        cancel: cancel.clone(),
        worker: AsyncMutex::new(None),
        capacity: reservation.capacity().clone(),
        record: record.clone(),
    });
    let worker = WorkerHandle::new(tokio::spawn(association_loop(
        upstream,
        socket,
        spec,
        receiver,
        association.clone(),
        record,
        state.counters.clone(),
    )));
    *association.worker.lock().await = Some(worker);
    #[cfg(test)]
    let setup_hook = state.setup_hook.lock().expect("setup hook").clone();
    #[cfg(test)]
    if let Some(hook) = setup_hook.as_ref() {
        hook.before_publish.pause().await;
    }
    let published = {
        let mut associations = state
            .associations
            .write()
            .expect("datagram association registry");
        if same_reservation_slot(associations.get(&client), &reservation) {
            associations.insert(client, AssociationSlot::Active(association.clone()));
            reservation.transition(SetupOutcome::Published);
            true
        } else {
            false
        }
    };
    if published {
        owner_guard.disarm();
        return Ok(association);
    }
    abandon_starting(state, client, &reservation, false).await;
    association.cancel.cancel();
    if let Some(mut worker) = association.worker.lock().await.take() {
        worker.join().await;
    }
    owner_guard.disarm();
    Err(DatagramRuntimeError::Conflict(
        "association drained during setup".into(),
    ))
}

async fn abandon_starting(
    state: &ProxyState,
    client: SocketAddr,
    reservation: &Arc<StartingReservation>,
    retryable: bool,
) {
    let mut associations = state
        .associations
        .write()
        .expect("datagram association registry");
    if same_reservation_slot(associations.get(&client), reservation) {
        associations.remove(&client);
    }
    reservation.transition(SetupOutcome::Abandoned { retryable });
    reservation.capacity().release();
}

pub(crate) async fn drain_associations(
    state: &ProxyState,
    retryable: bool,
) -> Vec<Arc<Association>> {
    let slots = {
        let mut associations = state
            .associations
            .write()
            .expect("datagram association registry");
        let slots = associations
            .drain()
            .map(|(_, slot)| slot)
            .collect::<Vec<_>>();
        for slot in &slots {
            if let AssociationSlot::Starting { reservation } = slot {
                reservation.transition(SetupOutcome::Abandoned { retryable });
                reservation.capacity().release();
            }
        }
        slots
    };
    slots
        .into_iter()
        .filter_map(|slot| match slot {
            AssociationSlot::Active(association) => Some(association),
            AssociationSlot::Starting { .. } => None,
        })
        .collect()
}
async fn association_loop(
    upstream_socket: UdpSocket,
    listener_socket: Arc<UdpSocket>,
    spec: DatagramProxySpec,
    mut ingress: mpsc::Receiver<Ingress>,
    association: Arc<Association>,
    record: Arc<Mutex<AssociationRecord>>,
    counters: Arc<ProxyCounters>,
) {
    let mut upstream = DatagramDirectionEngine::new(
        spec.queue_limits,
        &spec.name,
        association.id,
        Direction::Upstream,
        RngVersion::V1,
    )
    .expect("validated queue limits");
    let mut downstream = DatagramDirectionEngine::new(
        spec.queue_limits,
        &spec.name,
        association.id,
        Direction::Downstream,
        RngVersion::V1,
    )
    .expect("validated queue limits");
    let mut recv = vec![0u8; UDP_RECEIVE_BUFFER_BYTES];
    let send_context = AssociationSendContext {
        upstream_socket: &upstream_socket,
        listener_socket: &listener_socket,
        client: association.client,
        record: &record,
        cancel: &association.cancel,
    };
    loop {
        let now = Instant::now();
        let mut evidence_dirty = false;
        if emit_ready(
            &send_context,
            &mut upstream,
            &mut downstream,
            now,
            &mut evidence_dirty,
        )
        .await
        {
            break;
        }
        let next = [upstream.next_deadline(), downstream.next_deadline()]
            .into_iter()
            .flatten()
            .min();
        tokio::select! {
            biased;
            _ = association.cancel.cancelled() => break,
            item = ingress.recv() => match item {
                Some(item) => {
                    association.pending_ingress.fetch_sub(1, Ordering::Relaxed);
                    match upstream.admit(Instant::now(), item.payload, &item.policy) {
                        eggchaos_core::DatagramAdmission::Immediate(ready) => {
                            if send_upstream(&send_context, ready, &mut evidence_dirty).await {
                                break;
                            }
                        }
                        eggchaos_core::DatagramAdmission::Queued
                        | eggchaos_core::DatagramAdmission::Consumed => {}
                    }
                    evidence_dirty = true;
                    let mut r = record.lock().expect("association record"); r.last_activity = Instant::now();
                }
                None => break,
            },
            received = upstream_socket.recv(&mut recv) => match received {
                Ok(size) => {
                    if size as u64 > spec.queue_limits.max_datagram_bytes.get() {
                        counters.oversize_datagrams.fetch_add(1, Ordering::Relaxed);
                        evidence_dirty = true;
                        let mut r = record.lock().expect("association record"); r.oversize_datagrams = r.oversize_datagrams.saturating_add(1); r.last_activity = Instant::now();
                    } else {
                        match downstream.admit(Instant::now(), Bytes::copy_from_slice(&recv[..size]), &spec.downstream_policy.snapshot()) {
                            eggchaos_core::DatagramAdmission::Immediate(ready) => {
                                if send_downstream(&send_context, ready, &mut evidence_dirty).await {
                                    break;
                                }
                            }
                            eggchaos_core::DatagramAdmission::Queued
                            | eggchaos_core::DatagramAdmission::Consumed => {}
                        }
                        evidence_dirty = true;
                        let mut r = record.lock().expect("association record"); r.last_activity = Instant::now();
                    }
                }
                Err(_) => { evidence_dirty = true; let mut r = record.lock().expect("association record"); r.send_errors = r.send_errors.saturating_add(1); }
            },
            _ = wait_deadline(next) => {},
        }
        // Evidence snapshots move to this single observation point and only
        // when admission, emission, or error state actually changed, instead
        // of cloning both evidence records on every loop turn.
        if evidence_dirty {
            update_evidence(&record, &upstream, &downstream);
        }
    }
    let discarded = upstream
        .discard_all()
        .saturating_add(downstream.discard_all()) as u64
        + ingress.len() as u64;
    let mut r = record.lock().expect("association record");
    if association.administrative_cancel.load(Ordering::Relaxed) > 0 {
        r.administrative_discards = r.administrative_discards.saturating_add(discarded);
    }
    r.upstream_evidence = upstream.evidence().clone();
    r.downstream_evidence = downstream.evidence().clone();
}

async fn wait_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending::<()>().await,
    }
}

/// Drain both schedulers and forward ready datagrams. Egress accounting is
/// batched so each direction takes the record lock at most once no matter how
/// many datagrams are ready. Returns true when cancellation requires the
/// association loop to stop; `evidence_dirty` reports whether emission state
/// changed.
async fn emit_ready(
    context: &AssociationSendContext<'_>,
    upstream: &mut DatagramDirectionEngine,
    downstream: &mut DatagramDirectionEngine,
    now: Instant,
    evidence_dirty: &mut bool,
) -> bool {
    let ready = upstream.take_ready(now);
    if !ready.is_empty() && send_upstream(context, ready, evidence_dirty).await {
        return true;
    }
    let ready = downstream.take_ready(now);
    if !ready.is_empty() && send_downstream(context, ready, evidence_dirty).await {
        return true;
    }
    false
}

/// Forward upstream-ready datagrams toward the fixed target, batching egress
/// counters into a single record update. A datagram cancelled mid-batch and
/// everything after it count as administrative discards, matching the
/// previous per-datagram cancellation accounting exactly.
async fn send_upstream(
    context: &AssociationSendContext<'_>,
    ready: Vec<DatagramScheduled>,
    evidence_dirty: &mut bool,
) -> bool {
    let mut egress_datagrams = 0u64;
    let mut egress_bytes = 0u64;
    let mut send_errors = 0u64;
    for (index, DatagramScheduled { payload, .. }) in ready.iter().enumerate() {
        if context.cancel.is_cancelled() {
            let mut r = context.record.lock().expect("association record");
            r.egress_datagrams = r.egress_datagrams.saturating_add(egress_datagrams);
            r.egress_bytes = r.egress_bytes.saturating_add(egress_bytes);
            r.send_errors = r.send_errors.saturating_add(send_errors);
            r.administrative_discards = r
                .administrative_discards
                .saturating_add((ready.len() - index) as u64);
            *evidence_dirty = true;
            return true;
        }
        match context.upstream_socket.send(payload).await {
            Ok(size) if size == payload.len() => {
                egress_datagrams = egress_datagrams.saturating_add(1);
                egress_bytes = egress_bytes.saturating_add(size as u64);
            }
            _ => {
                send_errors = send_errors.saturating_add(1);
            }
        }
    }
    if egress_datagrams > 0 || send_errors > 0 {
        let mut r = context.record.lock().expect("association record");
        r.egress_datagrams = r.egress_datagrams.saturating_add(egress_datagrams);
        r.egress_bytes = r.egress_bytes.saturating_add(egress_bytes);
        r.send_errors = r.send_errors.saturating_add(send_errors);
        *evidence_dirty = true;
    }
    false
}

/// Forward downstream-ready datagrams toward the client, with the same
/// batched accounting as [`send_upstream`].
async fn send_downstream(
    context: &AssociationSendContext<'_>,
    ready: Vec<DatagramScheduled>,
    evidence_dirty: &mut bool,
) -> bool {
    let mut egress_datagrams = 0u64;
    let mut egress_bytes = 0u64;
    let mut send_errors = 0u64;
    for (index, DatagramScheduled { payload, .. }) in ready.iter().enumerate() {
        if context.cancel.is_cancelled() {
            let mut r = context.record.lock().expect("association record");
            r.egress_datagrams = r.egress_datagrams.saturating_add(egress_datagrams);
            r.egress_bytes = r.egress_bytes.saturating_add(egress_bytes);
            r.send_errors = r.send_errors.saturating_add(send_errors);
            r.administrative_discards = r
                .administrative_discards
                .saturating_add((ready.len() - index) as u64);
            *evidence_dirty = true;
            return true;
        }
        match context
            .listener_socket
            .send_to(payload, context.client)
            .await
        {
            Ok(size) if size == payload.len() => {
                egress_datagrams = egress_datagrams.saturating_add(1);
                egress_bytes = egress_bytes.saturating_add(size as u64);
            }
            _ => {
                send_errors = send_errors.saturating_add(1);
            }
        }
    }
    if egress_datagrams > 0 || send_errors > 0 {
        let mut r = context.record.lock().expect("association record");
        r.egress_datagrams = r.egress_datagrams.saturating_add(egress_datagrams);
        r.egress_bytes = r.egress_bytes.saturating_add(egress_bytes);
        r.send_errors = r.send_errors.saturating_add(send_errors);
        *evidence_dirty = true;
    }
    false
}

fn update_evidence(
    record: &Arc<Mutex<AssociationRecord>>,
    upstream: &DatagramDirectionEngine,
    downstream: &DatagramDirectionEngine,
) {
    let mut r = record.lock().expect("association record");
    r.upstream_evidence = upstream.evidence().clone();
    r.downstream_evidence = downstream.evidence().clone();
}

pub(crate) async fn stop_association(
    state: &ProxyState,
    association: Arc<Association>,
    administrative: bool,
) {
    if administrative {
        association
            .administrative_cancel
            .store(1, Ordering::Relaxed);
    }
    association.cancel.cancel();
    if let Some(mut worker) = association.worker.lock().await.take() {
        worker.join().await;
    }
    association.capacity.release();
    let snapshot = association
        .record
        .lock()
        .expect("association record")
        .snapshot(Instant::now());
    let mut history = state.history.lock().expect("association history");
    if state.limits.history > 0 {
        while history.len() >= state.limits.history {
            history.pop_front();
        }
        history.push_back(snapshot);
    }
}
