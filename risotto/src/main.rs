mod bmp;
mod config;
mod formatter;
mod producer;
mod state;
mod update_capnp;

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio_graceful::Shutdown;
use tracing::{debug, error, trace};

use risotto_lib::state::{new_state, AsyncState};
use risotto_lib::state_store::memory::MemoryStore;
use risotto_lib::state_store::store::StateStore;
use risotto_lib::update::Update;

use crate::config::{configure, AppConfig};

async fn bmp_handler<T: StateStore>(
    state: Option<AsyncState<T>>,
    cfg: Arc<AppConfig>,
    tx: Sender<Update>,
) {
    let bmp_config = cfg.bmp.clone();

    debug!("binding bmp listener to {}", bmp_config.host);
    let bmp_listener = TcpListener::bind(bmp_config.host).await.unwrap();

    loop {
        let (mut bmp_stream, _) = bmp_listener.accept().await.unwrap();
        let bmp_state = state.clone();
        let tx = tx.clone();

        // Spawn a new task for the BMP connection with TCP stream
        tokio::spawn(async move {
            if let Err(err) = bmp::handle(&mut bmp_stream, bmp_state, tx).await {
                error!("Error handling BMP connection: {}", err);
            }

            drop(bmp_stream);
        });
    }
}

async fn producer_handler(cfg: Arc<AppConfig>, rx: Receiver<Update>) {
    let kafka_config = cfg.kafka.clone();
    if let Err(err) = producer::handle(&kafka_config, rx).await {
        error!("Error handling Kafka producer: {}", err);
    }
}

async fn state_handler<T: StateStore + serde::Serialize>(
    state: AsyncState<T>,
    cfg: Arc<AppConfig>,
) {
    let state_config = cfg.state.clone();
    if let Err(err) = state::dump_handler(state.clone(), state_config).await {
        error!("Error dumping state: {}", err);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Arc::new(configure().await?);
    trace!("{:?}", cfg);

    let state_config = cfg.state.clone();
    let shutdown: Shutdown = Shutdown::default();

    // Initialize state if enabled
    let state = match state_config.disable {
        true => {
            debug!("state is disabled");
            None
        }
        false => {
            debug!("state is enabled");
            let store = MemoryStore::new();
            let state = new_state(store);
            state::load(state.clone(), state_config.clone()).await;
            shutdown.spawn_task(state_handler(state.clone(), cfg.clone()));
            Some(state)
        }
    };

    // Initialize MPSC channel to communicate between BMP tasks and producer task
    let (tx, rx) = channel(cfg.kafka.mpsc_buffer_size);

    // Initialize tasks
    let bmp_task = shutdown.spawn_task(bmp_handler(state.clone(), cfg.clone(), tx.clone()));
    let producer_task = shutdown.spawn_task(producer_handler(cfg.clone(), rx));
    tokio::select! {
        biased;
        _ = shutdown.shutdown_with_limit(Duration::from_secs(1)) => {}
        _ = bmp_task => {}
        _ = producer_task => {}
    }

    Ok(())
}
