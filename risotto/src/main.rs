mod bmp;
mod config;
mod formatter;
mod producer;
mod state;
mod update_capnp;

use anyhow::Result;
use clap::Parser;
use clap_verbosity_flag::{InfoLevel, Verbosity};
use config::{APIConfig, AppConfig};
use metrics_exporter_prometheus::PrometheusBuilder;
use std::net::SocketAddr;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_graceful::Shutdown;
use tracing::debug;

use risotto_lib::state::new_state;
use risotto_lib::state::AsyncState;
use risotto_lib::state_store::memory::MemoryStore;
use risotto_lib::state_store::store::StateStore;
use risotto_lib::update::Update;

use crate::config::app_config;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(short, long)]
    config: String,

    #[command(flatten)]
    verbose: Verbosity<InfoLevel>,
}

fn set_tracing(cli: &Cli) -> Result<()> {
    let subscriber = tracing_subscriber::fmt()
        .compact()
        .with_file(true)
        .with_line_number(true)
        .with_max_level(cli.verbose)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    Ok(())
}

fn set_prometheus(config: &APIConfig) {
    let socket = config.host.parse::<SocketAddr>().unwrap();
    let prom_builder = PrometheusBuilder::new();
    prom_builder
        .with_http_listener(socket)
        .install()
        .expect("Failed to install Prometheus metrics exporter");

    // Producer metrics
    metrics::describe_counter!(
        "risotto_kafka_messages_total",
        "Total number of Kafka messages produced"
    );

    // State metrics
    metrics::describe_counter!("risotto_state_dump_total", "Total number of state dumps");
    metrics::describe_gauge!("risotto_peer_established", "Peer established for a router");
    metrics::describe_gauge!(
        "risotto_state_updates",
        "Number of BGP updates in the state"
    );

    // Statistics metrics
    metrics::describe_counter!(
        "risotto_bmp_messages_total",
        "Total number of BMP messages received"
    );
    metrics::describe_counter!(
        "risotto_rx_updates_total",
        "Total number of BGP updates received"
    );
    metrics::describe_counter!(
        "risotto_tx_updates_total",
        "Total number of BGP updates sent"
    );
}

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

        // Spawn a new task for the BMP connection with each router
        tokio::spawn(async move {
            let _ = bmp::handle(&mut bmp_stream, bmp_state, tx).await;
            drop(bmp_stream);
        });
    }
}

async fn producer_handler(cfg: Arc<AppConfig>, rx: Receiver<Update>) {
    let kafka_config = cfg.kafka.clone();

    producer::handle(&kafka_config, rx).await;
}

async fn state_handler<T: StateStore + serde::Serialize>(
    state: Option<AsyncState<T>>,
    cfg: Arc<AppConfig>,
) {
    let state_config = cfg.state.clone();
    state::dump_handler(state.clone(), state_config).await;
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    set_tracing(&cli)?;

    let cfg = Arc::new(app_config(&cli.config));
    set_prometheus(&cfg.api.clone());

    let state_config = cfg.state.clone();
    let shutdown: Shutdown = Shutdown::default();

    // Initialize state if enabled
    let state = match state_config.enable {
        true => {
            debug!("state is enabled");
            let store = MemoryStore::new();
            let state = new_state(store);
            state::load(state.clone(), state_config.clone()).await;
            Some(state)
        }
        false => {
            debug!("state is disabled");
            None
        }
    };

    // MPSC channel to communicate between BMP tasks and producer task
    let (tx, rx) = channel();

    let bmp_task = shutdown.spawn_task(bmp_handler(state.clone(), cfg.clone(), tx.clone()));
    let producer_task = shutdown.spawn_task(producer_handler(cfg.clone(), rx));
    let state_task = shutdown.spawn_task(state_handler(state.clone(), cfg.clone()));

    tokio::select! {
        _ = shutdown.shutdown_with_limit(Duration::from_secs(1)) => {}
        _ = bmp_task => {}
        _ = producer_task => {}
        _ = state_task => {}
    }

    Ok(())
}
