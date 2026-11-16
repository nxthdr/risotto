use anyhow::Result;
use bytes::Bytes;
use metrics::{counter, gauge};
use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc::Sender;
use tokio_graceful::ShutdownGuard;
use tracing::{debug, error, trace};

use risotto_lib::event::{Event, EventBody, SessionEnd};
use risotto_lib::BmpSession;

/// Handle a BMP connection until it ends or the collector shuts down, then emit
/// a synthetic Termination event if the router did not send one.
pub async fn handle(stream: &mut TcpStream, tx: Sender<Event>, guard: ShutdownGuard) -> Result<()> {
    let socket = stream.peer_addr()?;
    debug!("{}: session established", socket);

    let mut session = BmpSession::new(socket);
    let mut peers = HashSet::new();

    let (result, end) = tokio::select! {
        result = read_messages(stream, socket, &mut session, &mut peers, &tx) => {
            (result, SessionEnd::ConnectionClosed)
        }
        _ = guard.cancelled() => (Ok(()), SessionEnd::CollectorShutdown),
    };

    if let Some(event) = session.close(end) {
        debug!(
            "{}: session ended without Termination message ({})",
            socket,
            end.as_str()
        );
        emit(event, socket, &mut peers, &tx).await?;
    }

    debug!("{}: session finished", socket);
    result
}

async fn read_messages(
    stream: &mut TcpStream,
    socket: SocketAddr,
    session: &mut BmpSession,
    peers: &mut HashSet<IpAddr>,
    tx: &Sender<Event>,
) -> Result<()> {
    loop {
        // Read the common header: version (1), message length (4), message type (1)
        let mut common_header = [0; 6];
        let n_bytes_peeked = stream.peek(&mut common_header).await?;

        if n_bytes_peeked == 0 {
            debug!("{}: connection closed by peer", socket);
            return Ok(());
        }

        if n_bytes_peeked != 6 {
            trace!(
                "{}: incomplete peek, expected 6 bytes, got {}",
                socket,
                n_bytes_peeked
            );
            continue;
        }

        let message_version = common_header[0];
        if message_version != 3 {
            trace!("{}: {:02x?}", socket, common_header);
            anyhow::bail!("{}: unsupported BMP version {}", socket, message_version);
        }

        let packet_length = u32::from_be_bytes(common_header[1..5].try_into().unwrap()) as usize;
        if packet_length < 6 {
            anyhow::bail!(
                "{}: invalid BMP message length {} (must be >= 6)",
                socket,
                packet_length
            );
        }

        let message_type = common_header[5];
        if message_type > 6 {
            anyhow::bail!("{}: unsupported BMP message type {}", socket, message_type);
        }

        let mut buffer = vec![0; packet_length];
        stream
            .read_exact(&mut buffer)
            .await
            .map_err(|e| anyhow::anyhow!("{}: failed to read message body: {}", socket, e))?;
        trace!("{}: read {} bytes: {:02x?}", socket, packet_length, buffer);

        let events = match session.decode(&mut Bytes::from(buffer)) {
            Ok(events) => events,
            Err(e) => {
                // A single undecodable message should not tear down the session
                error!("{}: {}", socket, e);
                counter!("risotto_bmp_decode_errors_total", "router" => socket.ip().to_string())
                    .increment(1);
                continue;
            }
        };

        counter!(
            "risotto_bmp_messages_total",
            "router" => socket.ip().to_string(),
            "type" => message_type_name(message_type),
        )
        .increment(1);

        for event in events {
            emit(event, socket, peers, tx).await?;
        }
    }
}

/// Update metrics for an event and send it to the producer.
async fn emit(
    event: Event,
    socket: SocketAddr,
    peers: &mut HashSet<IpAddr>,
    tx: &Sender<Event>,
) -> Result<()> {
    let router = socket.ip().to_string();
    match &event.body {
        EventBody::RouteMonitoring(header, _) => {
            counter!("risotto_rx_updates_total", "router" => router, "peer" => header.peer_addr.to_string())
                .increment(1);
        }
        EventBody::PeerUp(header, _) => {
            debug!("{}: peer up {}", socket, header.peer_addr);
            peers.insert(header.peer_addr);
            gauge!("risotto_peer_established", "router" => router, "peer" => header.peer_addr.to_string())
                .set(1);
        }
        EventBody::PeerDown(header, body) => {
            debug!(
                "{}: peer down {} (reason {})",
                socket, header.peer_addr, body.reason
            );
            peers.remove(&header.peer_addr);
            gauge!("risotto_peer_established", "router" => router, "peer" => header.peer_addr.to_string())
                .set(0);
        }
        EventBody::Termination(_) => {
            // The peers of this session are no longer observed (they are not known to be down)
            for peer in peers.drain() {
                gauge!("risotto_peer_established", "router" => router.clone(), "peer" => peer.to_string())
                    .set(0);
            }
        }
        _ => {}
    }

    trace!("{:?}", event);
    tx.send(event).await?;
    Ok(())
}

fn message_type_name(message_type: u8) -> &'static str {
    match message_type {
        0 => "route_monitoring",
        1 => "stats_report",
        2 => "peer_down_notification",
        3 => "peer_up_notification",
        4 => "initiation",
        5 => "termination",
        6 => "route_mirroring",
        _ => "unknown",
    }
}
