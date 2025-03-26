use anyhow::Result;
use bgpkit_parser::bmp::messages::{PerPeerFlags, RouteMonitoring};
use bgpkit_parser::models::Peer;
use bgpkit_parser::parse_bmp_msg;
use bgpkit_parser::parser::bmp::messages::{BmpMessage, BmpMessageBody};
use bytes::Bytes;
use core::net::IpAddr;
use std::sync::mpsc::Sender;
use tracing::{error, info, trace};

use crate::state::AsyncState;
use crate::state::{peer_up_withdraws_handler, synthesize_withdraw_update};
use crate::update::{decode_updates, format_update, UpdateHeader};

pub fn decode_bmp_message(bytes: &mut Bytes) -> Result<BmpMessage> {
    let message = match parse_bmp_msg(bytes) {
        Ok(message) => message,
        Err(_) => return Err(anyhow::anyhow!("failed to parse BMP message")),
    };

    Ok(message)
}

pub async fn process_peer_up_notification(
    state: Option<AsyncState>,
    tx: Sender<String>,
    router_addr: IpAddr,
    router_port: u16,
    peer: Peer,
) {
    if let Some(spawn_state) = state {
        let spawn_state = spawn_state.clone();
        tokio::spawn(async move {
            peer_up_withdraws_handler(spawn_state, router_addr, router_port, peer, tx).await;
        });
    }
}

pub async fn process_route_monitoring(
    state: Option<AsyncState>,
    tx: Sender<String>,
    router_addr: IpAddr,
    router_port: u16,
    peer: Peer,
    header: UpdateHeader,
    body: RouteMonitoring,
) {
    let potential_updates = decode_updates(body, header).unwrap_or_default();

    let mut legitimate_updates = Vec::new();
    if let Some(state) = &state {
        let mut state_lock = state.lock().unwrap();
        for update in potential_updates {
            let is_updated = state_lock.update(&router_addr, &peer, &update).unwrap();
            if is_updated {
                legitimate_updates.push(update);
            }
        }
    } else {
        legitimate_updates = potential_updates;
    }

    for mut update in legitimate_updates {
        let update = format_update(router_addr, router_port, &peer, &mut update);
        trace!("{:?}", update);

        // Sent to the event pipeline
        tx.send(update).unwrap();
    }
}

pub async fn process_peer_down_notification(
    state: Option<AsyncState>,
    tx: Sender<String>,
    router_addr: IpAddr,
    router_port: u16,
    peer: Peer,
) {
    if let Some(state) = state {
        let mut state_lock = state.lock().unwrap();

        // Remove the peer and the associated updates from the state
        // We start by emiting synthetic withdraw updates
        let mut synthetic_updates = Vec::new();
        let updates = state_lock.get_updates_by_peer(&router_addr, &peer).unwrap();
        for prefix in updates {
            synthetic_updates.push(synthesize_withdraw_update(prefix.clone()));
        }

        // Then update the state
        state_lock.remove_updates(&router_addr, &peer).unwrap();

        for mut update in synthetic_updates {
            let update = format_update(router_addr, router_port, &peer, &mut update);
            trace!("{:?}", update);

            // Send the synthetic updates to the event pipeline
            tx.send(update).unwrap();
        }
    }
}

pub async fn process_bmp_message(
    state: Option<AsyncState>,
    tx: Sender<String>,
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

    // Get peer information
    let Some(pph) = message.per_peer_header else {
        return;
    };
    let peer = Peer::new(pph.peer_bgp_id, pph.peer_ip, pph.peer_asn);

    // Get header information
    let timestamp = (pph.timestamp * 1000.0) as i64;

    let is_post_policy = match pph.peer_flags {
        PerPeerFlags::PeerFlags(flags) => flags.is_post_policy(),
        PerPeerFlags::LocalRibPeerFlags(_) => false,
    };

    let is_adj_rib_out = match pph.peer_flags {
        PerPeerFlags::PeerFlags(flags) => flags.is_adj_rib_out(),
        PerPeerFlags::LocalRibPeerFlags(_) => false,
    };

    let header = UpdateHeader {
        timestamp,
        is_post_policy,
        is_adj_rib_out,
    };

    match message.message_body {
        BmpMessageBody::PeerUpNotification(body) => {
            trace!("{:?}", body);
            info!(
                "PeerUpNotification: {} - {}",
                router_addr, peer.peer_address
            );

            process_peer_up_notification(state, tx, router_addr, router_port, peer).await;
        }
        BmpMessageBody::RouteMonitoring(body) => {
            trace!("{:?}", body);

            process_route_monitoring(state, tx, router_addr, router_port, peer, header, body).await;
        }
        BmpMessageBody::PeerDownNotification(body) => {
            trace!("{:?}", body);
            info!(
                "PeerDownNotification: {} - {}",
                router_addr, peer.peer_address
            );

            process_peer_down_notification(state, tx, router_addr, router_port, peer).await;
        }
        _ => (),
    }
}
