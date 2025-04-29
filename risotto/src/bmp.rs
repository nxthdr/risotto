use anyhow::Result;
use bytes::Bytes;

use std::sync::mpsc::Sender;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tracing::{debug, error, trace};

use risotto_lib::process_bmp_message;
use risotto_lib::state::AsyncState;
use risotto_lib::state_store::store::StateStore;
use risotto_lib::update::Update;

pub async fn handle<T: StateStore>(
    stream: &mut TcpStream,
    state: Option<AsyncState<T>>,
    tx: Sender<Update>,
) -> Result<()> {
    // Get router IP information
    let socket = stream.peer_addr().unwrap();
    debug!("{}: session established", socket.to_string());

    // Wait for the stream to be readable
    stream.readable().await?;

    loop {
        // Try to peek the common header length
        let mut common_header = [0; 6];
        let n_bytes_peeked = match stream.peek(&mut common_header).await {
            Ok(n) => n,
            Err(e) => {
                error!(
                    "{}: failed to peek from socket; error = {:?}",
                    socket.to_string(),
                    e
                );
                return Err(e.into());
            }
        };

        if n_bytes_peeked == 0 {
            debug!("{}: connection closed by peer", socket.to_string());
            break;
        }

        // Check if we peeked the expected number of bytes
        if n_bytes_peeked != 6 {
            trace!(
                "{}: incomplete peek, expected 6 bytes, got {}",
                socket.to_string(),
                n_bytes_peeked
            );
            // We continue to read the stream to avoid restarting the connection too fast
            continue;
        }

        // Get the message version from the `Version` BMP field
        let message_version = u8::from_be(common_header[0]);
        if message_version != 3 {
            let error_message = "not supported BMP message version";
            error!("{}: {}", socket.to_string(), error_message);
            trace!("{}: {:02x?}", socket.to_string(), common_header);
            anyhow::bail!(error_message);
        }

        // Get the message length from the `Message Length` BMP field
        let packet_length = u32::from_be_bytes(common_header[1..5].try_into().unwrap());
        let packet_length = usize::try_from(packet_length).unwrap();
        if packet_length < 6 {
            let error_message = format!(
                "invalid BMP message length: {} (must be >= 6)",
                packet_length
            );
            error!("{}: {}", socket.to_string(), error_message);
            trace!("{}: {:02x?}", socket.to_string(), common_header);
            anyhow::bail!(error_message);
        }

        // Get the message type from the `Message Type` BMP field
        let message_type = common_header[5];
        if message_type > 6 {
            let error_message = format!("not supported BMP message type: {}", message_type);
            error!("{}: {}", socket.to_string(), error_message);
            trace!("{}: {:02x?}", socket.to_string(), common_header);
            anyhow::bail!(error_message);
        }

        // Read the exact number of bytes found in the BMP message
        let mut buffer = vec![0; packet_length];
        match stream.read_exact(&mut buffer).await {
            Ok(_) => {
                trace!(
                    "{}: Read {} bytes: {:02x?}",
                    socket.to_string(),
                    packet_length,
                    buffer
                );
                let mut buffer_bytes = Bytes::from(buffer);

                // Process the BMP message
                process_bmp_message(state.clone(), tx.clone(), socket, &mut buffer_bytes).await;
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    debug!(
                        "{}: connection closed while reading message body",
                        socket.to_string()
                    );
                } else {
                    error!(
                        "{}: failed to read message body; error = {:?}",
                        socket.to_string(),
                        e
                    );
                }
                break;
            }
        }
    }

    debug!("{}: session finished", socket.to_string());
    Ok(())
}
