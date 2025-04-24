pub mod processor;
pub mod state;
pub mod state_store;
pub mod update;

use bgpkit_parser::parser::bmp::messages::BmpMessageBody;
use bytes::Bytes;
use metrics::counter;
use std::net::SocketAddr;
use std::sync::mpsc::Sender;
use tracing::{debug, error, trace};

use crate::processor::{
    decode_bmp_message, peer_down_notification, peer_up_notification, route_monitoring,
};
use crate::state::AsyncState;
use crate::state_store::store::StateStore;
use crate::update::{new_metadata, Update};

pub async fn process_bmp_message<T: StateStore>(
    state: Option<AsyncState<T>>,
    tx: Sender<Update>,
    socket: SocketAddr,
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

    trace!("{} - {:?}", socket, message);

    // Extract header and peer information
    let metadata = new_metadata(socket.clone(), &message);

    let metric_name = "risotto_bmp_messages_total";
    match message.message_body {
        BmpMessageBody::InitiationMessage(body) => {
            trace!("{}: {:?}", socket.to_string(), body);
            let tlvs_info = body
                .tlvs
                .iter()
                .map(|tlv| tlv.info.clone())
                .collect::<Vec<_>>();
            debug!("{}: InitiationMessage: {:?}", socket.to_string(), tlvs_info);
            counter!(metric_name, "router" =>  socket.ip().to_string(), "type" => "initiation")
                .increment(1);
            // No-Op
        }
        BmpMessageBody::PeerUpNotification(body) => {
            trace!("{}: {:?}", socket.to_string(), body);
            if metadata.is_none() {
                error!(
                    "{}: PeerUpNotification: no per-peer header",
                    socket.to_string()
                );
                return;
            }
            let metadata = metadata.unwrap();
            debug!(
                "{}: PeerUpNotification: {}",
                socket.to_string(),
                metadata.peer_addr
            );
            counter!(metric_name, "router" =>  socket.ip().to_string(), "type" => "peer_up_notification")
                .increment(1);
            peer_up_notification(state, tx, metadata, body).await;
        }
        BmpMessageBody::RouteMonitoring(body) => {
            trace!("{}: {:?}", socket.to_string(), body);
            if metadata.is_none() {
                error!(
                    "{}: RouteMonitoring - no per-peer header",
                    socket.to_string()
                );
                return;
            }
            let metadata = metadata.unwrap();
            // We do not process the message if the peer address is unspecified
            // Most likely a local RIB update
            if !metadata.peer_addr.is_unspecified() {
                counter!(metric_name, "router" =>  socket.ip().to_string(), "type" => "route_monitoring")
                .increment(1);
                route_monitoring(state, tx, metadata, body).await;
            }
        }
        BmpMessageBody::RouteMirroring(body) => {
            trace!("{}: {:?}", socket.to_string(), body);
            debug!("{}: RouteMirroring", socket.to_string());
            counter!(metric_name, "router" =>  socket.ip().to_string(), "type" => "route_mirroring")
                .increment(1);
            // No-Op
        }
        BmpMessageBody::PeerDownNotification(body) => {
            trace!("{}: {:?}", socket.to_string(), body);
            if metadata.is_none() {
                error!(
                    "{}: PeerDownNotification: no per-peer header",
                    socket.to_string()
                );
                return;
            }
            let metadata = metadata.unwrap();
            debug!(
                "{}: PeerDownNotification: {}. Reason: {:?}",
                socket.to_string(),
                metadata.peer_addr,
                body.reason
            );
            counter!(metric_name, "router" =>  socket.ip().to_string(), "type" => "peer_down_notification")
                .increment(1);
            peer_down_notification(state, tx, metadata, body).await;
        }

        BmpMessageBody::TerminationMessage(body) => {
            trace!("{}: {:?}", socket.to_string(), body);
            debug!("{}: TerminationMessage", socket.to_string());
            counter!(metric_name, "router" =>  socket.ip().to_string(), "type" => "termination")
                .increment(1);
            // No-Op
        }
        BmpMessageBody::StatsReport(body) => {
            trace!("{}: {:?}", socket.to_string(), body);
            debug!("{}: StatsReport", socket.to_string());
            counter!(metric_name, "router" =>  socket.ip().to_string(), "type" => "stats_report")
                .increment(1);
            // No-Op
        }
    };
}
