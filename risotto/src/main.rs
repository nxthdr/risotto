mod bmp;
mod config;
mod message_capnp;
mod producer;
mod serializer;

use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio_graceful::{Shutdown, ShutdownGuard};
use tracing::{debug, error, info, trace};

use risotto_lib::event::Event;

use crate::config::{configure, AppConfig};

/// Maximum time to close the BMP sessions and flush the events on shutdown.
const SHUTDOWN_LIMIT: Duration = Duration::from_secs(8);

async fn bmp_handler(listener: TcpListener, tx: Sender<Event>, guard: ShutdownGuard) {
    loop {
        let (mut bmp_stream, _) = tokio::select! {
            connection = listener.accept() => match connection {
                Ok(connection) => connection,
                Err(err) => {
                    error!("Error accepting BMP connection: {}", err);
                    continue;
                }
            },
            // Stop accepting connections; open sessions close themselves
            _ = guard.cancelled() => return,
        };
        let tx = tx.clone();

        // Each connection holds a shutdown guard, so that the collector waits for
        // its synthetic Termination to be emitted before exiting
        guard.spawn_task_fn(move |guard| async move {
            if let Err(err) = bmp::handle(&mut bmp_stream, tx, guard).await {
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

    // Shut down gracefully on SIGINT and SIGTERM
    let shutdown: Shutdown = Shutdown::default();

    // Initialize MPSC channel to communicate between BMP tasks and producer task
    let (tx, rx) = channel(cfg.kafka.mpsc_buffer_size);

    // The producer runs until every sender is dropped, i.e. until every BMP
    // session has been closed, then flushes the remaining events
    let mut producer_task = shutdown.spawn_task(producer_handler(cfg.clone(), rx));

    debug!("binding bmp listener to {}", cfg.bmp.host);
    let listener = TcpListener::bind(cfg.bmp.host).await?;

    // Sessions established before this point (by a previous run) are over
    tx.send(risotto_lib::collector_start(Utc::now())).await?;

    shutdown.spawn_task_fn(move |guard| bmp_handler(listener, tx, guard));

    tokio::select! {
        result = shutdown.shutdown_with_limit(SHUTDOWN_LIMIT) => match result {
            Ok(elapsed) => info!("shutdown completed in {:?}", elapsed),
            Err(err) => error!("shutdown did not complete: {}", err),
        },
        // The producer stopped on its own (e.g. invalid Kafka configuration)
        _ = &mut producer_task => {}
    }

    Ok(())
}
