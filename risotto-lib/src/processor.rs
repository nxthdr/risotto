use anyhow::Result;
use bgpkit_parser::bmp::messages::{PeerDownNotification, PeerUpNotification, RouteMonitoring};
use bgpkit_parser::parse_bmp_msg;
use bgpkit_parser::parser::bmp::messages::BmpMessage;
use bytes::Bytes;
use rand::Rng;
use std::sync::mpsc::Sender;
use tracing::trace;

use crate::state::AsyncState;
use crate::state::{peer_up_withdraws_handler, synthesize_withdraw_update};
use crate::update::{decode_updates, new_peer_from_metadata, Update, UpdateMetadata};

pub fn decode_bmp_message(bytes: &mut Bytes) -> Result<BmpMessage> {
    let message = match parse_bmp_msg(bytes) {
        Ok(message) => message,
        Err(_) => return Err(anyhow::anyhow!("failed to parse BMP message")),
    };

    Ok(message)
}

pub async fn peer_up_notification(
    state: Option<AsyncState>,
    tx: Sender<Update>,
    metadata: UpdateMetadata,
    _: PeerUpNotification,
) {
    if let Some(spawn_state) = state {
        let spawn_state = spawn_state.clone();
        tokio::spawn(async move {
            let random = {
                let mut rng = rand::rng();
                rng.random_range(-60.0..60.0) as u64
            };
            let sleep_time = 300 + random; // 5 minutes +/- 1 minute
            peer_up_withdraws_handler(spawn_state, tx, metadata, sleep_time).await;
        });
    }
}

pub async fn route_monitoring(
    state: Option<AsyncState>,
    tx: Sender<Update>,
    metadata: UpdateMetadata,
    body: RouteMonitoring,
) {
    let potential_updates = decode_updates(body, metadata.clone()).unwrap_or_default();

    let mut legitimate_updates = Vec::new();
    if let Some(state) = &state {
        let peer = new_peer_from_metadata(metadata.clone());
        let mut state_lock = state.lock().unwrap();
        for update in potential_updates {
            let is_updated = state_lock
                .update(&metadata.router_addr.clone(), &peer, &update)
                .unwrap();
            if is_updated {
                legitimate_updates.push(update);
            }
        }
    } else {
        legitimate_updates = potential_updates;
    }

    for update in legitimate_updates {
        trace!("{:?}", update);

        // Sent to the event pipeline
        tx.send(update).unwrap();
    }
}

pub async fn peer_down_notification(
    state: Option<AsyncState>,
    tx: Sender<Update>,
    metadata: UpdateMetadata,
    _: PeerDownNotification,
) {
    if let Some(state) = state {
        // Remove the peer and the associated updates from the state
        // We start by emiting synthetic withdraw updates

        let peer = new_peer_from_metadata(metadata.clone());
        let mut state_lock = state.lock().unwrap();

        let mut synthetic_updates = Vec::new();
        let updates = state_lock
            .get_updates_by_peer(&metadata.router_addr, &peer)
            .unwrap();
        for prefix in updates {
            synthetic_updates.push(synthesize_withdraw_update(prefix.clone(), metadata.clone()));
        }

        // Then update the state
        state_lock
            .remove_updates(&metadata.router_addr, &peer)
            .unwrap();

        for update in synthetic_updates {
            trace!("{:?}", update);

            // Send the synthetic updates to the event pipeline
            tx.send(update).unwrap();
        }
    }
}
