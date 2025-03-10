use crate::state::{self, AsyncState};
use crate::update::{decode_updates, format_update, UpdateHeader};
use bgpkit_parser::bmp::messages::PerPeerFlags;
use bgpkit_parser::models::Peer;
use bgpkit_parser::parse_bmp_msg;
use bgpkit_parser::parser::bmp::messages::{BmpMessage, BmpMessageBody};
use bytes::Bytes;
use core::net::IpAddr;
use log::{error, info, trace};
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
        Err(_) => Err(Error::new(
            ErrorKind::InvalidData,
            "failed to parse BMP message".to_string(),
        )),
    }
}

async fn process_bmp_packet(
    state: AsyncState,
    tx: Sender<String>,
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
    let timestamp = (pph.timestamp * 1000.0) as i64;

    let is_post_policy = match pph.peer_flags {
        PerPeerFlags::PeerFlags(flags) => flags.is_post_policy(),
        PerPeerFlags::LocalRibPeerFlags(_) => false,
    };

    let is_adj_rib_out = match pph.peer_flags {
        PerPeerFlags::PeerFlags(flags) => flags.is_adj_rib_out(),
        PerPeerFlags::LocalRibPeerFlags(_) => false,
    };

    match message.message_body {
        BmpMessageBody::PeerUpNotification(body) => {
            trace!("{:?}", body);
            info!(
                "bmp - PeerUpNotification: {} - {}",
                router_addr, peer.peer_address
            );

            let spawn_state = state.clone();
            tokio::spawn(async move {
                state::peer_up_withdraws_handler(spawn_state, router_addr, peer, tx).await;
            });
        }
        BmpMessageBody::RouteMonitoring(body) => {
            trace!("{:?}", body);
            let header = UpdateHeader {
                timestamp,
                is_post_policy,
                is_adj_rib_out,
            };

            let potential_updates = decode_updates(body, header).unwrap_or_default();

            let mut legitimate_updates = Vec::new();
            for update in potential_updates {
                let is_updated = state_lock.update(&router_addr, &peer, &update).unwrap();
                if is_updated {
                    legitimate_updates.push(update);
                }
            }

            for mut update in legitimate_updates {
                let update = format_update(router_addr, router_port, &peer, &mut update);
                log::trace!("{:?}", update);

                // Sent to the event pipeline
                tx.send(update).unwrap();
            }
        }
        BmpMessageBody::PeerDownNotification(body) => {
            trace!("{:?}", body);
            info!(
                "bmp - PeerDownNotification: {} - {}",
                router_addr, peer.peer_address
            );

            // Remove the peer and the associated updates from the state
            // We start by emiting synthetic withdraw updates
            let mut synthetic_updates = Vec::new();
            let updates = state_lock.get_updates_by_peer(&router_addr, &peer).unwrap();
            for prefix in updates {
                synthetic_updates.push(state::synthesize_withdraw_update(prefix.clone()));
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
        _ => (),
    }
}

pub async fn handle(socket: &mut TcpStream, state: AsyncState, tx: Sender<String>) {
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
                error!("bmp - invalid BMP message: {}", e);
                error!(
                    "bmp - closing connection with {}:{}",
                    router_ip, router_port
                );
                break;
            }
            Err(e) => {
                // Other errors are unexpected
                // Close the connection
                error!("bmp - failed to unmarshal BMP message: {}", e);
                error!(
                    "bmp - closing connection with {}:{}",
                    router_ip, router_port
                );
                break;
            }
        };

        // Process the BMP message
        let process_state = state.clone();
        let process_tx = tx.clone();
        tokio::spawn(async move {
            process_bmp_packet(process_state, process_tx, router_ip, router_port, message).await;
        });
    }
}
