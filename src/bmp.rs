use bgpkit_parser::models::Peer;
use bgpkit_parser::parse_bmp_msg;
use bgpkit_parser::parser::bmp::messages::{BmpMessage, BmpMessageBody};
use bytes::Bytes;
use chrono::Utc;
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
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "BMP message too big: {} bytes",
        ));
    }

    // Exactly read the number of bytes found in the BMP message
    let mut buf = vec![0; packet_length];
    socket.read_exact(&mut buf).await?;

    let mut bytes = Bytes::copy_from_slice(&buf);

    // Parse the BMP message
    let message = parse_bmp_msg(&mut bytes).unwrap();

    return Ok(Some(message));
}

pub async fn handle(socket: &mut TcpStream, db: DB) {
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

    // Fetch router from the DB
    let mut routers = db.routers.lock().unwrap();
    let router = routers
        .entry(router_ip)
        .or_insert_with(|| new_router(router_ip, router_port));

    match message.message_body {
        BmpMessageBody::PeerUpNotification(_) => {
            // Simply add the peer if we did not see it before
            router.add_peer(&peer);
        }
        BmpMessageBody::RouteMonitoring(body) => {
            let potential_updates = decode_updates(body).unwrap_or(Vec::new());

            let mut legitimate_updates = Vec::new();
            for update in potential_updates {
                let is_updated = router.update(&peer, &update);
                if is_updated {
                    legitimate_updates.push(update);
                }
            }

            // TODO: Handle multiple event pipelines (stdout, CSV file, Kafka, ...)
            for update in legitimate_updates {
                let update = format_update(&router, &peer, &update);
                println!("{:?}", update);
                send_to_kafka("broker.nxthdr.dev:9092", "bgp-updates", update.as_bytes());
            }
        }
        BmpMessageBody::PeerDownNotification(_) => {
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

            // TODO: Handle multiple event pipelines (stdout, CSV file, Kafka, ...)
            for update in synthetic_updates {
                let update = format_update(&router, &peer, &update);
                println!("{:?}", update);
                send_to_kafka("broker.nxthdr.dev:9092", "bgp-updates", update.as_bytes());
            }
        }
        _ => (),
    }
}
