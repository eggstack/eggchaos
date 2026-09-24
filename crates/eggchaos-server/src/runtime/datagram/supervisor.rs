//! Datagram listener supervision: the proxy receive loop, client-datagram
//! admission, and idle reaping. Listener tasks are owned by the registry and
//! associations are resolved through the association lifecycle; this module
//! owns no registry or policy state itself.
use std::{
    net::SocketAddr,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use bytes::Bytes;
use tokio::{
    net::UdpSocket,
    sync::mpsc,
    time::{interval, Instant, MissedTickBehavior},
};
use tokio_util::sync::CancellationToken;

use super::{
    association::{resolve_association, stop_association, Association, AssociationSlot, Ingress},
    model::DatagramRuntimeError,
    registry::{reserve_ingress, ListenerOwner, ProxyState},
    UDP_RECEIVE_BUFFER_BYTES,
};

pub(crate) fn spawn_listener(socket: Arc<UdpSocket>, state: Arc<ProxyState>) -> ListenerOwner {
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let bound = socket
        .local_addr()
        .expect("bound UDP socket has local address");
    let task = tokio::spawn(async move {
        listener_loop(socket, state, task_cancel).await;
    });
    ListenerOwner {
        bound,
        cancel,
        task,
    }
}
async fn listener_loop(socket: Arc<UdpSocket>, state: Arc<ProxyState>, cancel: CancellationToken) {
    let mut recv = vec![0u8; UDP_RECEIVE_BUFFER_BYTES];
    let mut reaper = interval(Duration::from_millis(10));
    reaper.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            received = socket.recv_from(&mut recv) => {
                let Ok((size, client)) = received else { continue };
                receive_client_datagram(&socket, &state, client, Bytes::copy_from_slice(&recv[..size])).await;
            }
            _ = reaper.tick() => reap_idle(&state).await,
        }
    }
    for association in super::association::drain_associations(&state, false).await {
        stop_association(&state, association, true).await;
    }
}

async fn receive_client_datagram(
    socket: &Arc<UdpSocket>,
    state: &Arc<ProxyState>,
    client: SocketAddr,
    payload: Bytes,
) {
    let spec = state.spec.read().expect("datagram proxy spec").clone();
    if payload.len() as u64 > spec.queue_limits.max_datagram_bytes.get() {
        state
            .counters
            .oversize_datagrams
            .fetch_add(1, Ordering::Relaxed);
        if let Some(association) = state
            .associations
            .lock()
            .await
            .get(&client)
            .and_then(AssociationSlot::active)
        {
            let mut record = association.record.lock().expect("association record");
            record.oversize_datagrams = record.oversize_datagrams.saturating_add(1);
            record.last_activity = Instant::now();
        }
        return;
    }
    let association = match resolve_association(socket.clone(), state, client).await {
        Ok(association) => association,
        Err(DatagramRuntimeError::AssociationLimit) => {
            state
                .counters
                .capacity_rejections
                .fetch_add(1, Ordering::Relaxed);
            return;
        }
        Err(_) => {
            state
                .counters
                .association_setup_failures
                .fetch_add(1, Ordering::Relaxed);
            return;
        }
    };
    let policy = spec.upstream_policy.snapshot();
    let ingress_bytes = payload.len() as u64;
    let Some(reservation) = reserve_ingress(
        state.ingress_buffered_bytes.clone(),
        payload.len(),
        state.limits.max_ingress_queue_bytes,
    ) else {
        state
            .counters
            .ingress_overflow
            .fetch_add(1, Ordering::Relaxed);
        let mut record = association.record.lock().expect("association record");
        record.ingress_queue_overflow = record.ingress_queue_overflow.saturating_add(1);
        record.last_activity = Instant::now();
        return;
    };
    let ingress = Ingress {
        payload,
        policy,
        _reservation: reservation,
    };
    association.pending_ingress.fetch_add(1, Ordering::Relaxed);
    match association.sender.try_send(ingress) {
        Ok(()) => {
            let mut record = association.record.lock().expect("association record");
            record.ingress_datagrams = record.ingress_datagrams.saturating_add(1);
            record.ingress_bytes = record.ingress_bytes.saturating_add(ingress_bytes);
            record.last_activity = Instant::now();
        }
        Err(mpsc::error::TrySendError::Full(_)) => {
            association.pending_ingress.fetch_sub(1, Ordering::Relaxed);
            state
                .counters
                .ingress_overflow
                .fetch_add(1, Ordering::Relaxed);
            let mut record = association.record.lock().expect("association record");
            record.ingress_queue_overflow = record.ingress_queue_overflow.saturating_add(1);
            record.last_activity = Instant::now();
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            association.pending_ingress.fetch_sub(1, Ordering::Relaxed);
        }
    }
}
async fn reap_idle(state: &Arc<ProxyState>) {
    let now = Instant::now();
    let idle_timeout = state
        .spec
        .read()
        .expect("datagram proxy spec")
        .association_idle_timeout;
    let expired: Vec<(SocketAddr, Arc<Association>)> = {
        let associations = state.associations.lock().await;
        associations
            .iter()
            .filter_map(|(client, slot)| {
                let association = slot.active()?;
                let (queues_empty, pending, idle) = {
                    let record = association.record.lock().expect("association record");
                    (
                        record.upstream_evidence.queued_datagrams == 0
                            && record.downstream_evidence.queued_datagrams == 0,
                        association.pending_ingress.load(Ordering::Relaxed),
                        now.saturating_duration_since(record.last_activity),
                    )
                };
                (queues_empty && pending == 0 && idle >= idle_timeout)
                    .then_some((*client, association))
            })
            .collect()
    };
    for (client, association) in expired {
        // Remove only if the slot still holds this exact association: a new
        // setup for the same client must never be reaped as stale.
        let removed = {
            let mut associations = state.associations.lock().await;
            let ours = matches!(
                associations.get(&client),
                Some(AssociationSlot::Active(current)) if Arc::ptr_eq(current, &association)
            );
            if ours {
                associations.remove(&client).is_some()
            } else {
                false
            }
        };
        if removed {
            stop_association(state, association, false).await;
        }
    }
}
