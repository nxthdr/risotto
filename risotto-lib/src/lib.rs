pub mod processor;
pub mod state;
pub mod state_store;
pub mod statistics;
pub mod update;

use bgpkit_parser::parser::bmp::messages::BmpMessageBody;
use bytes::Bytes;
use core::net::IpAddr;
use statistics::AsyncStatistics;
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
    statistics: AsyncStatistics,
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

    let mut statistics_lock = statistics.lock().await;

    trace!("[{}]:{} - {:?}", router_addr, router_port, message);
    statistics_lock.rx_bmp_messages += 1;

    // Extract header and peer information
    let metadata = new_metadata(router_addr, router_port, &message);

    match message.message_body {
        BmpMessageBody::InitiationMessage(body) => {
            trace!("{:?}", body);
            let tlvs_info = body
                .tlvs
                .iter()
                .map(|tlv| tlv.info.clone())
                .collect::<Vec<_>>();
            debug!(
                "[{}]:{} - InitiationMessage: {:?}",
                router_addr, router_port, tlvs_info
            );
            statistics_lock.rx_bmp_initiation += 1;
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
            debug!(
                "[{}]:{} - PeerUpNotification - {}",
                metadata.router_addr, metadata.router_port, metadata.peer_addr
            );
            statistics_lock.rx_bmp_peer_up += 1;
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
            statistics_lock.rx_bmp_route_monitoring += 1;
            route_monitoring(state, tx, metadata, body).await;
        }
        BmpMessageBody::RouteMirroring(body) => {
            trace!("{:?}", body);
            debug!("[{}]:{} - RouteMirroring", router_addr, router_port);
            statistics_lock.rx_bmp_route_mirroring += 1;
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
            debug!(
                "[{}]:{} - PeerDownNotification: - {}",
                metadata.router_addr, metadata.router_port, metadata.peer_addr
            );
            statistics_lock.rx_bmp_peer_down += 1;
            peer_down_notification(state, tx, metadata, body).await;
        }

        BmpMessageBody::TerminationMessage(body) => {
            trace!("{:?}", body);
            info!("[{}]:{} - TerminationMessage", router_addr, router_port);
            statistics_lock.rx_bmp_termination += 1;
            // No-Op
        }
        BmpMessageBody::StatsReport(body) => {
            trace!("{:?}", body);
            info!("[{}]:{} - StatsReport", router_addr, router_port);
            statistics_lock.rx_bmp_stats_report += 1;
            // No-Op
        }
    };
}
