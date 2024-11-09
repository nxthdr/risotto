use bgpkit_parser::models::Peer;
use bgpkit_parser::parse_bmp_msg;
use bgpkit_parser::parser::bmp::messages::{BmpMessage, BmpMessageBody};
use bytes::Bytes;
use chrono::Utc;
use config::Config;
use log::{debug, info, warn};
use std::io;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

use crate::db::DB;
use crate::pipeline::send_to_kafka;
use crate::router::new_router;
use crate::update::{decode_updates, format_update};

pub async fn unmarshal_bmp_packet(socket: &mut TcpStream) -> io::Result<Option<BmpMessage>> {
    // Get minimal packet length to get how many bytes to remove from the socket
    let mut min_buff = [0; 6];
    socket.peek(&mut min_buff).await?;

    // Get the packet length from the `Message Length` BMP field
    let packet_length = u32::from_be_bytes(min_buff[1..5].try_into().unwrap());
    let packet_length = usize::try_from(packet_length).unwrap();

    if packet_length == 0 {
        return Ok(None);
    }

    if packet_length > 4096 {
        warn!(
            "bmp - failed to parse BMP message: message too big: {} bytes",
            packet_length
        );
        return Ok(None);
    }

    // Exactly read the number of bytes found in the BMP message
    let mut buf = vec![0; packet_length];
    socket.read_exact(&mut buf).await?;

    let mut bytes = Bytes::copy_from_slice(&buf);

    // Parse the BMP message
    match parse_bmp_msg(&mut bytes) {
        Ok(message) => Ok(Some(message)),
        Err(_) => {
            warn!("bmp - failed to parse BMP message");
            Ok(None)
        }
    }
}

pub async fn handle(socket: &mut TcpStream, db: DB, settings: Config) {
    // Get router IP information
    let socket_info = socket.peer_addr().unwrap();
    let router_ip = socket_info.ip();
    let router_port = socket_info.port();

    // Get BMP message
    let result = unmarshal_bmp_packet(socket).await.unwrap();
    let Some(message) = result else { return };

    // Get peer information
    let Some(pph) = message.per_peer_header else {
        return;
    };
    let peer = Peer::new(pph.peer_bgp_id, pph.peer_ip, pph.peer_asn);
    let ts = (pph.timestamp * 1000.0) as i64;

    // Fetch router from the DB
    let mut routers = db.routers.lock().unwrap();
    let router = routers
        .entry(router_ip)
        .or_insert_with(|| new_router(router_ip, router_port));

    match message.message_body {
        BmpMessageBody::PeerUpNotification(body) => {
            debug!("{:?}", body);
            // Simply add the peer if we did not see it before
            router.add_peer(&peer);
        }
        BmpMessageBody::RouteMonitoring(body) => {
            debug!("{:?}", body);
            let potential_updates = decode_updates(body, ts).unwrap_or(Vec::new());

            let mut legitimate_updates = Vec::new();
            for update in potential_updates {
                let is_updated = router.update(&peer, &update);
                if is_updated {
                    legitimate_updates.push(update);
                }
            }

            // TODO: handle multiple event pipelines (stdout, CSV file, Kafka, ...)
            for update in legitimate_updates {
                let update = format_update(&router, &peer, &update);
                info!("{:?}", update);
                send_to_kafka(&settings, update.as_bytes());
            }
        }
        BmpMessageBody::PeerDownNotification(body) => {
            debug!("{:?}", body);
            // Remove the peer and the associated prefixes
            // To do so, we start by emiting synthetic withdraw updates
            let mut synthetic_updates = Vec::new();
            for (_, update_to_withdrawn) in router.peers[&peer].clone() {
                let mut update_to_withdrawn = update_to_withdrawn.clone();
                update_to_withdrawn.announced = false;
                update_to_withdrawn.timestamp = Utc::now();
                update_to_withdrawn.synthetic = true;

                synthetic_updates.push(update_to_withdrawn);
            }

            // And we then update the internal state
            router.remove_peer(&peer);

            // TODO: handle multiple event pipelines (stdout, CSV file, Kafka, ...)
            for update in synthetic_updates {
                let update = format_update(&router, &peer, &update);
                info!("{:?}", update);
                send_to_kafka(&settings, update.as_bytes());
            }
        }
        _ => (),
    }
}
