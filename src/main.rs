mod api;
mod bmp;
mod config;
mod producer;
mod state;
mod update;

use chrono::Local;
use clap::Parser;
use clap_verbosity_flag::{InfoLevel, Verbosity};
use config::AppConfig;
use env_logger::Builder;
use log::{debug, error, info};
use std::error::Error;
use std::io::Write;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_graceful::Shutdown;

use crate::config::app_config;
use crate::state::AsyncState;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(short, long)]
    config: String,

    #[command(flatten)]
    verbose: Verbosity<InfoLevel>,
}

fn set_logging(cli: &Cli) {
    Builder::new()
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] - {}",
                Local::now().format("%Y-%m-%dT%H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .filter_module("risotto", cli.verbose.log_level_filter())
        .init();
}

async fn api_handler(state: AsyncState, cfg: Arc<AppConfig>) {
    let api_config = cfg.api.clone();
    debug!("api - binding listener to {}", api_config.host);
    let api_listener = TcpListener::bind(api_config.host).await.unwrap();

    let app = api::app(state.clone());
    axum::serve(api_listener, app).await.unwrap();
}

async fn bmp_handler(state: AsyncState, cfg: Arc<AppConfig>, tx: Sender<String>) {
    let bmp_config = cfg.bmp.clone();

    debug!("bmp - binding listener to {}", bmp_config.host);
    let bmp_listener = TcpListener::bind(bmp_config.host).await.unwrap();

    loop {
        let (mut bmp_socket, _) = bmp_listener.accept().await.unwrap();
        let bmp_state = state.clone();
        let tx = tx.clone();

        // Spawn a new task for each BMP connection
        tokio::spawn(async move {
            bmp::handle(&mut bmp_socket, bmp_state.clone(), tx).await;
        });
    }
}

async fn producer_handler(cfg: Arc<AppConfig>, rx: Receiver<String>) {
    let kafka_config = cfg.kafka.clone();

    match producer::handle(&kafka_config, rx).await {
        Ok(_) => {}
        Err(e) => error!("producer - {}", e),
    }
}

async fn state_handler(state: AsyncState, cfg: Arc<AppConfig>) {
    let cfg = cfg.state.clone();

    state::dump_handler(state.clone(), cfg.clone()).await;
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    let cfg = Arc::new(app_config(&cli.config));
    let state_config = cfg.state.clone();
    let state = state::new_state(&state_config);
    let shutdown: Shutdown = Shutdown::default();

    set_logging(&cli);

    // Load the state if enabled
    if state_config.enable {
        state::load(state.clone());
    }

    // MPSC channel to communicate between BMP tasks and producer task
    let (tx, rx) = channel();

    let api_task = shutdown.spawn_task(api_handler(state.clone(), cfg.clone()));
    let bmp_task = shutdown.spawn_task(bmp_handler(state.clone(), cfg.clone(), tx.clone()));
    let producer_task = shutdown.spawn_task(producer_handler(cfg.clone(), rx));
    let state_task = shutdown.spawn_task(state_handler(state.clone(), cfg.clone()));

    tokio::select! {
        _ = shutdown.shutdown_with_limit(Duration::from_secs(1)) => {
            info!("shutdown - gracefully after shutdown signal received");
        }
        _ = api_task => {
            info!("api - handler shutdown");
        }
        _ = bmp_task => {
            info!("bmp - handler shutdown");
        }
        _ = producer_task => {
            info!("producer - handler shutdown");
        }
        _ = state_task => {
            info!("state - handler shutdown");
        }
    }

    Ok(())
}
