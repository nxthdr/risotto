use anyhow::Result;
use bgpkit_parser::bmp::messages::{PeerDownNotification, PeerUpNotification, RouteMonitoring};
use bgpkit_parser::parse_bmp_msg;
use bgpkit_parser::parser::bmp::messages::{BmpMessage, BmpMessageBody};
use bytes::Bytes;
use core::net::IpAddr;
use std::sync::mpsc::Sender;
use tracing::{error, info, trace};

use crate::state::AsyncState;
use crate::state::{peer_up_withdraws_handler, synthesize_withdraw_update};
use crate::update::{decode_updates, new_metadata, new_peer_from_metadata, Update, UpdateMetadata};

pub fn decode_bmp_message(bytes: &mut Bytes) -> Result<BmpMessage> {
    let message = match parse_bmp_msg(bytes) {
        Ok(message) => message,
        Err(_) => return Err(anyhow::anyhow!("failed to parse BMP message")),
    };

    Ok(message)
}

pub async fn process_peer_up_notification(
    state: Option<AsyncState>,
    tx: Sender<Update>,
    metadata: UpdateMetadata,
    _: PeerUpNotification,
) {
    if let Some(spawn_state) = state {
        let spawn_state = spawn_state.clone();
        tokio::spawn(async move {
            peer_up_withdraws_handler(spawn_state, tx, metadata).await;
        });
    }
}

pub async fn process_route_monitoring(
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

pub async fn process_peer_down_notification(
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

pub async fn process_bmp_message(
    state: Option<AsyncState>,
    tx: Sender<Update>,
    router_addr: IpAddr,
    router_port: u16,
    bytes: &mut Bytes,
) {
    // Parse the BMP message
    let message = match decode_bmp_message(bytes) {
        Ok(message) => message,
        Err(e) => {
            error!("failed to decode BMP message: {}", e);
            return;
        }
    };

    // Extract header and peer information
    let metadata = match new_metadata(router_addr, router_port, message.clone()) {
        Some(m) => m,
        None => return,
    };

    match message.message_body {
        BmpMessageBody::InitiationMessage(_) => {
            info!(
                "InitiationMessage: {} - {}",
                metadata.router_addr, metadata.peer_addr
            )
            // No-Op
        }
        BmpMessageBody::PeerUpNotification(body) => {
            trace!("{:?}", body);
            info!(
                "PeerUpNotification: {} - {}",
                metadata.router_addr, metadata.peer_addr
            );
            process_peer_up_notification(state, tx, metadata, body).await;
        }
        BmpMessageBody::RouteMonitoring(body) => {
            trace!("{:?}", body);
            process_route_monitoring(state, tx, metadata, body).await;
        }
        BmpMessageBody::RouteMirroring(_) => {
            info!(
                "RouteMirroring: {} - {}",
                metadata.router_addr, metadata.peer_addr
            )
            // No-Op
        }
        BmpMessageBody::PeerDownNotification(body) => {
            trace!("{:?}", body);
            info!(
                "PeerDownNotification: {} - {}",
                metadata.router_addr, metadata.peer_addr
            );
            process_peer_down_notification(state, tx, metadata, body).await;
        }

        BmpMessageBody::TerminationMessage(_) => {
            info!(
                "TerminationMessage: {} - {}",
                metadata.router_addr, metadata.peer_addr
            )
            // No-Op
        }
        BmpMessageBody::StatsReport(_) => {
            info!(
                "StatsReport: {} - {}",
                metadata.router_addr, metadata.peer_addr
            )
            // No-Op
        }
    }
}
