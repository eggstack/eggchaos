//! Datagram association lifecycle: setup reservation/publication, the
//! per-association worker loop, batched egress accounting, and teardown. The
//! registry lock is never held across UDP socket setup; see
//! [`resolve_association`].
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use bytes::Bytes;
use eggchaos_core::{
    DatagramDirectionEngine, DatagramEvidence, DatagramScheduled, Direction, RngVersion,
};
use tokio::{
    net::UdpSocket,
    sync::{mpsc, Mutex as AsyncMutex, Notify},
    task::JoinHandle,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use super::{
    model::{AssociationRecord, DatagramProxySpec, DatagramRuntimeError, ProxyCounters},
    registry::ProxyState,
    UDP_RECEIVE_BUFFER_BYTES,
};

/// Registry entry for one client address. `Starting` reserves global and
/// per-proxy capacity while UDP bind/connect runs without holding the
/// registry lock; exactly one of the racing creators owns setup and every
/// other first-datagram waiter either observes the published `Active`
/// association or takes over creation if setup was abandoned.
#[derive(Clone)]
pub(crate) enum AssociationSlot {
    Starting { notify: Arc<Notify> },
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
pub(crate) struct Association {
    pub(crate) id: u64,
    pub(crate) client: SocketAddr,
    pub(crate) sender: mpsc::Sender<Ingress>,
    pub(crate) pending_ingress: AtomicUsize,
    pub(crate) administrative_cancel: AtomicUsize,
    pub(crate) cancel: CancellationToken,
    pub(crate) worker: AsyncMutex<Option<JoinHandle<()>>>,
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
/// Resolve the live association for one client datagram. Concurrent first
/// datagrams for the same client converge on one setup: exactly one caller
/// reserves the slot and runs UDP bind/connect without holding the registry
/// lock, while the others yield until the slot publishes or is abandoned
/// (in which case a waiter takes over creation). Capacity accounting is
/// exact: reservations hold one global count and one per-proxy slot from
/// reservation until publication, setup failure, or administrative drain.
pub(crate) async fn resolve_association(
    socket: Arc<UdpSocket>,
    state: &Arc<ProxyState>,
    client: SocketAddr,
) -> Result<Arc<Association>, DatagramRuntimeError> {
    // A stuck setup always resolves because the owner publishes or abandons
    // its slot synchronously after bind/connect; the bound only guards
    // against yielding forever under adversarial scheduling.
    for _ in 0..10_000 {
        let reservation = {
            let mut associations = state.associations.lock().await;
            match associations.get(&client) {
                Some(AssociationSlot::Active(association)) => return Ok(association.clone()),
                Some(AssociationSlot::Starting { .. }) => None,
                None => {
                    let spec = state.spec.read().expect("datagram proxy spec").clone();
                    if associations.len() >= spec.max_associations {
                        return Err(DatagramRuntimeError::AssociationLimit);
                    }
                    let prior = state.global_active.fetch_add(1, Ordering::AcqRel);
                    if prior >= state.limits.max_associations {
                        state.global_active.fetch_sub(1, Ordering::AcqRel);
                        return Err(DatagramRuntimeError::AssociationLimit);
                    }
                    let notify = Arc::new(Notify::new());
                    associations.insert(
                        client,
                        AssociationSlot::Starting {
                            notify: notify.clone(),
                        },
                    );
                    Some((spec, notify))
                }
            }
        };
        match reservation {
            Some((spec, notify)) => {
                return publish_association(socket, state, client, spec, notify).await;
            }
            None => tokio::task::yield_now().await,
        }
    }
    Err(DatagramRuntimeError::Conflict(
        "association setup did not resolve".into(),
    ))
}

/// Run UDP socket setup without the registry lock, then publish the live
/// association under the lock. Setup failures and administrative drains that
/// removed the reservation both release global capacity exactly once and
/// wake any waiters; a drained-while-setting-up association is torn down
/// before it can leak a task or an upstream socket.
async fn publish_association(
    socket: Arc<UdpSocket>,
    state: &Arc<ProxyState>,
    client: SocketAddr,
    spec: DatagramProxySpec,
    notify: Arc<Notify>,
) -> Result<Arc<Association>, DatagramRuntimeError> {
    let local = match spec.upstream.ip() {
        IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
        IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
    };
    let upstream = match UdpSocket::bind(local).await {
        Ok(socket) => socket,
        Err(error) => {
            abandon_starting(state, client, &notify).await;
            return Err(DatagramRuntimeError::Bind(error));
        }
    };
    if let Err(error) = upstream.connect(spec.upstream).await {
        abandon_starting(state, client, &notify).await;
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
        record: record.clone(),
    });
    let worker = tokio::spawn(association_loop(
        upstream,
        socket,
        spec,
        receiver,
        association.clone(),
        record,
        state.counters.clone(),
    ));
    *association.worker.lock().await = Some(worker);
    {
        let mut associations = state.associations.lock().await;
        let ours = matches!(
            associations.get(&client),
            Some(AssociationSlot::Starting { notify: current }) if Arc::ptr_eq(current, &notify)
        );
        if ours {
            associations.insert(client, AssociationSlot::Active(association.clone()));
            notify.notify_waiters();
            return Ok(association);
        }
    }
    // An administrative drain (proxy update/delete, listener stop) removed
    // our reservation while setup ran. Tear down the unpublished worker so no
    // task, socket, or capacity count leaks, and report a conflict so the
    // receive path records a setup failure rather than a capacity rejection.
    association.cancel.cancel();
    if let Some(worker) = association.worker.lock().await.take() {
        let _ = worker.await;
    }
    state.global_active.fetch_sub(1, Ordering::AcqRel);
    notify.notify_waiters();
    Err(DatagramRuntimeError::Conflict(
        "association drained during setup".into(),
    ))
}

/// Release one setup reservation after bind/connect failure: remove the slot
/// only if it is still ours, release the global count exactly once, and wake
/// waiters so one of them can take over creation.
async fn abandon_starting(state: &ProxyState, client: SocketAddr, notify: &Arc<Notify>) {
    let mut associations = state.associations.lock().await;
    let ours = matches!(
        associations.get(&client),
        Some(AssociationSlot::Starting { notify: current }) if Arc::ptr_eq(current, notify)
    );
    if ours {
        associations.remove(&client);
        state.global_active.fetch_sub(1, Ordering::AcqRel);
    }
    notify.notify_waiters();
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
    if let Some(worker) = association.worker.lock().await.take() {
        let _ = worker.await;
    }
    state.global_active.fetch_sub(1, Ordering::AcqRel);
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
