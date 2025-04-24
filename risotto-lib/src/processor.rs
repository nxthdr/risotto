use anyhow::Result;
use bgpkit_parser::bmp::messages::{PeerDownNotification, PeerUpNotification, RouteMonitoring};
use bgpkit_parser::parse_bmp_msg;
use bgpkit_parser::parser::bmp::messages::BmpMessage;
use bytes::Bytes;
use metrics::{counter, gauge};
use rand::Rng;
use std::sync::mpsc::Sender;
use tracing::{debug, trace};

use crate::state::AsyncState;
use crate::state::{peer_up_withdraws_handler, synthesize_withdraw_update};
use crate::state_store::store::StateStore;
use crate::update::{decode_updates, Update, UpdateMetadata};

pub fn decode_bmp_message(bytes: &mut Bytes) -> Result<BmpMessage> {
    let message = match parse_bmp_msg(bytes) {
        Ok(message) => message,
        Err(_) => return Err(anyhow::anyhow!("failed to parse BMP message")),
    };

    Ok(message)
}

pub async fn peer_up_notification<T: StateStore>(
    state: Option<AsyncState<T>>,
    tx: Sender<Update>,
    metadata: UpdateMetadata,
    _: PeerUpNotification,
) {
    if let Some(state) = state {
        let mut state_lock = state.lock().await;

        // Add the peer to the state
        state_lock
            .add_peer(&metadata.router_socket.ip(), &metadata.peer_addr)
            .unwrap();

        gauge!(
            "risotto_peer_established",
            "router" => metadata.router_socket.ip().to_string(),
            "peer" => metadata.peer_addr.to_string()
        )
        .set(1);

        let spawn_state = state.clone();
        let random = {
            let mut rng = rand::rng();
            rng.random_range(-60.0..60.0) as u64
        };
        let sleep_time = 300 + random; // 5 minutes +/- 1 minute
        tokio::spawn(async move {
            peer_up_withdraws_handler(spawn_state, tx, metadata, sleep_time).await;
        });
    }
}

pub async fn route_monitoring<T: StateStore>(
    state: Option<AsyncState<T>>,
    tx: Sender<Update>,
    metadata: UpdateMetadata,
    body: RouteMonitoring,
) {
    let potential_updates = decode_updates(body, metadata.clone()).unwrap_or_default();
    counter!(
        "risotto_rx_updates_total",
        "router" => metadata.router_socket.ip().to_string(),
        "peer" => metadata.peer_addr.to_string(),
    )
    .increment(potential_updates.len() as u64);

    let mut legitimate_updates = Vec::new();
    if let Some(state) = &state {
        let mut state_lock = state.lock().await;
        for update in potential_updates {
            debug!("{}: {:?}", metadata.router_socket, update);
            let is_updated = state_lock
                .update(&metadata.router_socket.ip(), &metadata.peer_addr, &update)
                .unwrap();
            if is_updated {
                legitimate_updates.push(update);
            }
        }
    } else {
        legitimate_updates = potential_updates;
    }

    counter!(
        "risotto_tx_updates_total",
        "router" => metadata.router_socket.ip().to_string(),
        "peer" => metadata.peer_addr.to_string(),
    )
    .increment(legitimate_updates.len() as u64);

    for update in legitimate_updates {
        trace!("{:?}", update);

        // Sent to the event pipeline
        tx.send(update).unwrap();
    }
}

pub async fn peer_down_notification<T: StateStore>(
    state: Option<AsyncState<T>>,
    tx: Sender<Update>,
    metadata: UpdateMetadata,
    _: PeerDownNotification,
) {
    if let Some(state) = state {
        // Remove the peer and the associated updates from the state
        // We start by emiting synthetic withdraw updates
        let mut state_lock = state.lock().await;

        let mut synthetic_updates = Vec::new();
        let updates = state_lock
            .get_updates_by_peer(&metadata.router_socket.ip(), &metadata.peer_addr)
            .unwrap();
        for prefix in updates {
            synthetic_updates.push(synthesize_withdraw_update(prefix.clone(), metadata.clone()));
        }

        // Remove the peer from the state
        state_lock
            .remove_peer(&metadata.router_socket.ip(), &metadata.peer_addr)
            .unwrap();

        gauge!(
            "risotto_peer_established",
            "router" => metadata.router_socket.ip().to_string(),
            "peer" => metadata.peer_addr.to_string()
        )
        .set(0);

        counter!(
            "risotto_tx_updates_total",
            "router" => metadata.router_socket.ip().to_string(),
            "peer" => metadata.peer_addr.to_string(),
        )
        .increment(synthetic_updates.len() as u64);

        for update in synthetic_updates {
            trace!("{:?}", update);

            // Send the synthetic updates to the event pipeline
            tx.send(update).unwrap();
        }
    }
}
