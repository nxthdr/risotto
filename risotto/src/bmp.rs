use anyhow::Result;
use bytes::Bytes;

use std::sync::mpsc::Sender;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tracing::{debug, error, trace};

use risotto_lib::process_bmp_message;
use risotto_lib::state::AsyncState;
use risotto_lib::state_store::store::StateStore;
use risotto_lib::statistics::AsyncStatistics;
use risotto_lib::update::Update;

pub async fn handle<T: StateStore>(
    stream: &mut TcpStream,
    state: Option<AsyncState<T>>,
    statistics: AsyncStatistics,
    tx: Sender<Update>,
) -> Result<()> {
    // Get router IP information
    let socket_info = stream.peer_addr().unwrap();
    let router_addr = socket_info.ip();
    let router_port = socket_info.port();

    debug!("[{}]:{} - session established", router_addr, router_port);

    loop {
        // Wait for the stream to be readable
        stream.readable().await?;

        // Get minimal packet length to get how many bytes to remove from the socket
        let mut common_header = [0; 6];
        let n_bytes_peeked = stream.peek(&mut common_header).await?;
        if n_bytes_peeked != 6 {
            trace!("[{}]:{} - incomplete peek", router_addr, router_port);
            trace!("[{}]:{} - {:02x?}", router_addr, router_port, common_header);
            continue;
        }

        // Get the message version from the `Version` BMP field
        let message_version = u8::from_be(common_header[0]);
        if message_version != 3 {
            let error_message = "not supported BMP message version";
            error!("[{}]:{} - {}", router_addr, router_port, error_message);
            trace!("[{}]:{} - {:02x?}", router_addr, router_port, common_header);
            anyhow::bail!(error_message);
        }

        // Get the message length from the `Message Length` BMP field
        let packet_length = u32::from_be_bytes(common_header[1..5].try_into().unwrap());
        let packet_length = usize::try_from(packet_length).unwrap();
        if packet_length == 0 {
            let error_message = "invalid BMP message length";
            error!("[{}]:{} - {}", router_addr, router_port, error_message);
            trace!("[{}]:{} - {:02x?}", router_addr, router_port, common_header);
            anyhow::bail!(error_message);
        }

        // Get the message type from the `Message Type` BMP field
        let message_type = common_header[5];
        if message_type > 6 {
            let error_message = "not supported BMP message type";
            error!("[{}]:{} - {}", router_addr, router_port, error_message);
            trace!("[{}]:{} - {:02x?}", router_addr, router_port, common_header);
            anyhow::bail!(error_message);
        }

        // Exactly read the number of bytes found in the BMP message
        let mut buffer = vec![0; packet_length];
        stream.read_exact(&mut buffer).await?;
        trace!("[{}]:{} - {:02x?}", router_addr, router_port, buffer);
        let mut buffer = Bytes::from(buffer);

        // Process the BMP message
        process_bmp_message(
            state.clone(),
            statistics.clone(),
            tx.clone(),
            router_addr,
            router_port,
            &mut buffer,
        )
        .await;
    }
}
