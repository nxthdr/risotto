pub mod processor;
pub mod state;
pub mod state_store;
pub mod update;

use bgpkit_parser::parser::bmp::messages::BmpMessageBody;
use bytes::Bytes;
use core::net::IpAddr;
use std::sync::mpsc::Sender;
use tracing::{error, info, trace};

use crate::processor::{
    decode_bmp_message, peer_down_notification, peer_up_notification, route_monitoring,
};
use crate::state::AsyncState;
use crate::state_store::store::StateStore;
use crate::update::{new_metadata, Update};

pub async fn process_bmp_message<T: StateStore>(
    state: Option<AsyncState<T>>,
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
            peer_up_notification(state, tx, metadata, body).await;
        }
        BmpMessageBody::RouteMonitoring(body) => {
            trace!("{:?}", body);
            route_monitoring(state, tx, metadata, body).await;
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
            peer_down_notification(state, tx, metadata, body).await;
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
