mod api;
mod bmp;
mod producer;
mod settings;
mod state;
mod update;

use chrono::Local;
use clap::Parser;
use clap_verbosity_flag::{InfoLevel, Verbosity};
use config::Config;
use env_logger::Builder;
use log::{debug, info};
use std::error::Error;
use std::io::Write;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_graceful::Shutdown;

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

fn load_settings(config_path: &str) -> Arc<Config> {
    let cfg = Config::builder()
        .add_source(config::File::with_name(config_path))
        .add_source(config::Environment::with_prefix("RISOTTO"))
        .build()
        .unwrap();
    Arc::new(cfg)
}

async fn api_handler(state: AsyncState, cfg: Arc<Config>) {
    let api_config = settings::get_api_config(&cfg).unwrap();

    debug!("api - binding listener to {}", api_config.host);
    let api_listener = TcpListener::bind(api_config.host).await.unwrap();

    let app = api::app(state.clone());
    axum::serve(api_listener, app).await.unwrap();
}

async fn bmp_handler(state: AsyncState, cfg: Arc<Config>, tx: Sender<Vec<u8>>) {
    let bmp_config = settings::get_bmp_config(&cfg).unwrap();

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

async fn producer_handler(cfg: Arc<Config>, rx: Receiver<Vec<u8>>) {
    let cfg = settings::get_kafka_config(&cfg).unwrap();

    producer::handle(&cfg, rx).await;
}

async fn state_handler(state: AsyncState, cfg: Arc<Config>) {
    let cfg = settings::get_state_config(&cfg).unwrap();

    state::dump_handler(state.clone(), cfg.clone()).await;
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    let cfg = load_settings(&cli.config);
    let state_config = settings::get_state_config(&cfg).unwrap();
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
