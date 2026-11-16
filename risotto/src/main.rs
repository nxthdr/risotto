mod bmp;
mod config;
mod message_capnp;
mod producer;
mod serializer;

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio_graceful::Shutdown;
use tracing::{debug, error, trace};

use risotto_lib::event::Event;

use crate::config::{configure, AppConfig};

async fn bmp_handler(cfg: Arc<AppConfig>, tx: Sender<Event>) {
    let bmp_config = cfg.bmp.clone();

    debug!("binding bmp listener to {}", bmp_config.host);
    let bmp_listener = TcpListener::bind(bmp_config.host).await.unwrap();

    loop {
        let (mut bmp_stream, _) = match bmp_listener.accept().await {
            Ok(connection) => connection,
            Err(err) => {
                error!("Error accepting BMP connection: {}", err);
                continue;
            }
        };
        let tx = tx.clone();

        // Spawn a new task for the BMP connection with TCP stream
        tokio::spawn(async move {
            if let Err(err) = bmp::handle(&mut bmp_stream, tx).await {
                error!("Error handling BMP connection: {}", err);
            }
        });
    }
}

async fn producer_handler(cfg: Arc<AppConfig>, rx: Receiver<Event>) {
    let kafka_config = cfg.kafka.clone();
    if let Err(err) = producer::handle(&kafka_config, rx).await {
        error!("Error handling Kafka producer: {}", err);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Arc::new(configure().await?);
    trace!("{:?}", cfg);

    let shutdown: Shutdown = Shutdown::default();

    // Initialize MPSC channel to communicate between BMP tasks and producer task
    let (tx, rx) = channel(cfg.kafka.mpsc_buffer_size);

    // Initialize tasks
    let bmp_task = shutdown.spawn_task(bmp_handler(cfg.clone(), tx));
    let producer_task = shutdown.spawn_task(producer_handler(cfg.clone(), rx));
    tokio::select! {
        biased;
        _ = shutdown.shutdown_with_limit(Duration::from_secs(1)) => {}
        _ = bmp_task => {}
        _ = producer_task => {}
    }

    Ok(())
}
