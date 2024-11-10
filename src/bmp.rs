use crate::db::DB;
use crate::router::new_router;
use crate::update::{decode_updates, format_update};
use bgpkit_parser::models::Peer;
use bgpkit_parser::parse_bmp_msg;
use bgpkit_parser::parser::bmp::messages::{BmpMessage, BmpMessageBody};
use bytes::Bytes;
use chrono::Utc;
use core::net::IpAddr;
use log::{debug, error, trace};
use std::io::{Error, ErrorKind, Result};
use std::sync::mpsc::Sender;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

pub async fn unmarshal_bmp_packet(socket: &mut TcpStream) -> Result<BmpMessage> {
    // Get minimal packet length to get how many bytes to remove from the socket
    let mut min_buff = [0; 6];
    socket.peek(&mut min_buff).await?;

    // Get the packet length from the `Message Length` BMP field
    let packet_length = u32::from_be_bytes(min_buff[1..5].try_into().unwrap());
    let packet_length = usize::try_from(packet_length).unwrap();

    if packet_length == 0 {
        return Err(Error::new(ErrorKind::NotFound, "BMP message length is 0"));
    }

    if packet_length > 4096 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("BMP message length is too big: {} bytes", packet_length),
        ));
    }

    // Exactly read the number of bytes found in the BMP message
    let mut buf = vec![0; packet_length];
    socket.read_exact(&mut buf).await?;

    let mut bytes = Bytes::copy_from_slice(&buf);

    // Parse the BMP message
    match parse_bmp_msg(&mut bytes) {
        Ok(message) => Ok(message),
        Err(_) => {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("failed to parse BMP message"),
            ));
        }
    }
}

async fn process(
    db: DB,
    tx: Sender<Vec<u8>>,
    router_ip: IpAddr,
    router_port: u16,
    message: BmpMessage,
) {
    // Get peer information
    let Some(pph) = message.per_peer_header else {
        return;
    };
    let peer = Peer::new(pph.peer_bgp_id, pph.peer_ip, pph.peer_asn);
    let ts = (pph.timestamp * 1000.0) as i64;

    // Fetch router from the DB
    let mut routers = db.lock().unwrap();
    let router = routers
        .entry(router_ip)
        .or_insert_with(|| new_router(router_ip, router_port));

    match message.message_body {
        BmpMessageBody::PeerUpNotification(body) => {
            trace!("{:?}", body);
            // Simply add the peer if we did not see it before
            router.add_peer(&peer);
        }
        BmpMessageBody::RouteMonitoring(body) => {
            trace!("{:?}", body);
            let potential_updates = decode_updates(body, ts).unwrap_or(Vec::new());

            let mut legitimate_updates = Vec::new();
            for update in potential_updates {
                let is_updated = router.update(&peer, &update);
                if is_updated {
                    legitimate_updates.push(update);
                }
            }

            let mut buffer = vec![];
            for update in legitimate_updates {
                let update = format_update(&router, &peer, &update);
                debug!("{:?}", update);
                buffer.extend(update.as_bytes());
                buffer.extend(b"\n");
            }
            // let buffer = BufReader::new(buffer);

            // Sent to the event pipeline
            tx.send(buffer).unwrap();
        }
        BmpMessageBody::PeerDownNotification(body) => {
            trace!("{:?}", body);
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

            let mut buffer = vec![];
            for update in synthetic_updates {
                let update = format_update(&router, &peer, &update);
                debug!("{:?}", update);
                buffer.extend(update.as_bytes());
                buffer.extend(b"\n");
            }

            // Sent to the event pipeline
            tx.send(buffer).unwrap();
        }
        _ => (),
    }
}

pub async fn handle(socket: &mut TcpStream, db: DB, tx: Sender<Vec<u8>>) {
    // Get router IP information
    let socket_info = socket.peer_addr().unwrap();
    let router_ip = socket_info.ip();
    let router_port = socket_info.port();

    loop {
        // Get BMP message
        let message = match unmarshal_bmp_packet(socket).await {
            Ok(message) => message,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                continue;
            }
            Err(e) if e.kind() == ErrorKind::InvalidData => {
                error!("bmp - failed to unmarshal BMP message: {}", e);
                // TODO: potentially we could do better than just closing the connection
                error!("bmp - closing connection to {}:{}", router_ip, router_port);
                break;
            }
            Err(e) => {
                // Other errors are unexpected
                // Close the connection and log the error
                error!("bmp - failed to unmarshal BMP message: {}", e);
                error!("bmp - closing connection to {}:{}", router_ip, router_port);
                break;
            }
        };

        // Process the BMP message
        let process_db = db.clone();
        let process_tx = tx.clone();
        tokio::spawn(async move {
            process(process_db, process_tx, router_ip, router_port, message).await;
        });
    }
}
