pub mod processor;
pub mod state;
pub mod state_store;
pub mod update;

use bgpkit_parser::parser::bmp::messages::BmpMessageBody;
use bytes::Bytes;
use core::net::IpAddr;
use std::sync::mpsc::Sender;
use tracing::{debug, error, info, trace};

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

    trace!("[{}]:{} - {:?}", router_addr, router_port, message);

    // Extract header and peer information
    let metadata = new_metadata(router_addr, router_port, &message);

    match message.message_body {
        BmpMessageBody::InitiationMessage(body) => {
            let tlvs_info = body
                .tlvs
                .iter()
                .map(|tlv| tlv.info.clone())
                .collect::<Vec<_>>();
            info!(
                "[{}]:{} - InitiationMessage: {:?}",
                router_addr, router_port, tlvs_info
            );
            // No-Op
        }
        BmpMessageBody::PeerUpNotification(body) => {
            trace!("{:?}", body);
            if metadata.is_none() {
                error!(
                    "[{}]:{} - PeerUpNotification - no per-peer header",
                    router_addr, router_port
                );
                return;
            }
            let metadata = metadata.unwrap();
            info!(
                "[{}]:{} - PeerUpNotification - {}",
                metadata.router_addr, metadata.router_port, metadata.peer_addr
            );
            peer_up_notification(state, tx, metadata, body).await;
        }
        BmpMessageBody::RouteMonitoring(body) => {
            debug!("{:?}", body);
            if metadata.is_none() {
                error!(
                    "[{}]:{} - RouteMonitoring - no per-peer header",
                    router_addr, router_port
                );
                return;
            }
            let metadata = metadata.unwrap();
            route_monitoring(state, tx, metadata, body).await;
        }
        BmpMessageBody::RouteMirroring(_) => {
            info!("[{}]:{} - RouteMirroring", router_addr, router_port)
            // No-Op
        }
        BmpMessageBody::PeerDownNotification(body) => {
            trace!("{:?}", body);
            if metadata.is_none() {
                error!(
                    "[{}]:{} - RouteMonitoring - no per-peer header",
                    router_addr, router_port
                );
                return;
            }
            let metadata = metadata.unwrap();
            info!(
                "[{}]:{} - PeerDownNotification: - {}",
                metadata.router_addr, metadata.router_port, metadata.peer_addr
            );
            peer_down_notification(state, tx, metadata, body).await;
        }

        BmpMessageBody::TerminationMessage(_) => {
            info!("[{}]:{} - TerminationMessage", router_addr, router_port)
            // No-Op
        }
        BmpMessageBody::StatsReport(_) => {
            info!("[{}]:{} - StatsReport", router_addr, router_port)
            // No-Op
        }
    }
}
