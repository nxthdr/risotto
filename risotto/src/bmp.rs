use bytes::Bytes;
use std::io::{Error, ErrorKind, Result};
use std::sync::mpsc::Sender;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tracing::error;

use risotto_lib::handler::process_bmp_message;
use risotto_lib::state::AsyncState;

pub async fn unmarshal_bmp_packet(socket: &mut TcpStream) -> Result<Bytes> {
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

    Ok(Bytes::copy_from_slice(&buf))
}

pub async fn handle(socket: &mut TcpStream, state: Option<AsyncState>, tx: Sender<String>) {
    // Get router IP information
    let socket_info = socket.peer_addr().unwrap();
    let router_ip = socket_info.ip();
    let router_port = socket_info.port();

    loop {
        // Get BMP message
        let mut bytes = match unmarshal_bmp_packet(socket).await {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                // Empty message, continue
                continue;
            }
            Err(e) if e.kind() == ErrorKind::InvalidData => {
                // Invalid message, continue without processing
                // From what I can see, it's often because of a packet length issue
                // So for now, we will close the connection
                error!("invalid BMP message: {}", e);
                error!("closing connection with {}:{}", router_ip, router_port);
                break;
            }
            Err(e) => {
                // Other errors are unexpected
                // Close the connection
                error!("failed to unmarshal BMP message: {}", e);
                error!("closing connection with {}:{}", router_ip, router_port);
                break;
            }
        };

        // Process the received bytes
        let process_state = state.clone();
        let process_tx = tx.clone();
        tokio::spawn(async move {
            process_bmp_message(
                process_state,
                process_tx,
                router_ip,
                router_port,
                &mut bytes,
            )
            .await;
        });
    }
}
