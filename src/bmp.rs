use crate::state::{self, AsyncState};
use crate::update::{decode_updates, format_update, Update};
use bgpkit_parser::models::{Origin, Peer};
use bgpkit_parser::parse_bmp_msg;
use bgpkit_parser::parser::bmp::messages::{BmpMessage, BmpMessageBody};
use bytes::Bytes;
use chrono::Utc;
use core::net::IpAddr;
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
            ErrorKind::InvalidInput,
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
    state: AsyncState,
    tx: Sender<Vec<u8>>,
    router_addr: IpAddr,
    router_port: u16,
    message: BmpMessage,
) {
    let mut state_lock = state.lock().unwrap();

    // Get peer information
    let Some(pph) = message.per_peer_header else {
        return;
    };
    let peer = Peer::new(pph.peer_bgp_id, pph.peer_ip, pph.peer_asn);
    let ts = (pph.timestamp * 1000.0) as i64;

    match message.message_body {
        BmpMessageBody::PeerUpNotification(body) => {
            log::trace!("{:?}", body);
            log::info!(
                "bmp - PeerUpNotification: {} - {}",
                router_addr,
                peer.peer_address
            );

            let spawn_state = state.clone();
            tokio::spawn(async move {
                state::peer_up_withdraws_handler(spawn_state, router_addr, peer, tx).await;
            });
        }
        BmpMessageBody::RouteMonitoring(body) => {
            log::trace!("{:?}", body);
            let potential_updates = decode_updates(body, ts).unwrap_or(Vec::new());

            let mut legitimate_updates = Vec::new();
            for update in potential_updates {
                let is_updated = state_lock.update(&router_addr, &peer, &update).unwrap();
                if is_updated {
                    legitimate_updates.push(update);
                }
            }

            let mut buffer = vec![];
            for update in legitimate_updates {
                let update = format_update(router_addr, router_port, &peer, &update);
                log::trace!("{:?}", update);
                buffer.extend(update.as_bytes());
                buffer.extend(b"\n");
            }

            // Sent to the event pipeline
            tx.send(buffer).unwrap();
        }
        BmpMessageBody::PeerDownNotification(body) => {
            log::trace!("{:?}", body);
            log::info!(
                "bmp - PeerDownNotification: {} - {}",
                router_addr,
                peer.peer_address
            );

            // Remove the peer and the associated prefixes
            // To do so, we start by emiting synthetic withdraw updates
            let mut synthetic_updates = Vec::new();
            let updates = state_lock.get_updates_by_peer(&router_addr, &peer).unwrap();
            for prefix in updates {
                let update_to_withdrawn = Update {
                    prefix: prefix.prefix.clone(),
                    announced: false,
                    origin: Origin::INCOMPLETE,
                    path: None,
                    communities: vec![],
                    timestamp: Utc::now(),
                    synthetic: true,
                };

                synthetic_updates.push(update_to_withdrawn);
            }

            // And we then update the state
            state_lock.remove_updates(&router_addr, &peer).unwrap();

            let mut buffer = vec![];
            for update in synthetic_updates {
                let update = format_update(router_addr, router_port, &peer, &update);
                log::trace!("{:?}", update);
                buffer.extend(update.as_bytes());
                buffer.extend(b"\n");
            }

            // Sent to the event pipeline
            tx.send(buffer).unwrap();
        }
        _ => (),
    }
}

pub async fn handle(socket: &mut TcpStream, state: AsyncState, tx: Sender<Vec<u8>>) {
    // Get router IP information
    let socket_info = socket.peer_addr().unwrap();
    let router_ip = socket_info.ip();
    let router_port = socket_info.port();

    loop {
        // Get BMP message
        let message = match unmarshal_bmp_packet(socket).await {
            Ok(message) => message,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                // Empty message, continue
                continue;
            }
            Err(e) if e.kind() == ErrorKind::InvalidData => {
                // Invalid message, continue without processing
                // From what I can see, it's often because of a packet length issue
                // So for now, we will close the connection
                log::error!("bmp - invalid BMP message: {}", e);
                log::error!(
                    "bmp - closing connection with {}:{}",
                    router_ip,
                    router_port
                );
                break;
            }
            Err(e) => {
                // Other errors are unexpected
                // Close the connection
                log::error!("bmp - failed to unmarshal BMP message: {}", e);
                log::error!(
                    "bmp - closing connection with {}:{}",
                    router_ip,
                    router_port
                );
                break;
            }
        };

        // Process the BMP message
        let process_state = state.clone();
        let process_tx = tx.clone();
        tokio::spawn(async move {
            process(process_state, process_tx, router_ip, router_port, message).await;
        });
    }
}
