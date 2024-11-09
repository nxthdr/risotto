mod api;
mod bmp;
mod db;
mod producer;
mod router;
mod settings;
mod update;

use chrono::Local;
use clap::Parser;
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

use crate::db::DB;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct CLI {
    #[arg(short, long)]
    config: String,

    #[command(flatten)]
    verbose: clap_verbosity_flag::Verbosity,
}

fn set_logging(cli: &CLI) {
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
        .filter(Some("bgpkit_parser"), log::LevelFilter::Off)
        .filter(Some("tokio_graceful"), log::LevelFilter::Off)
        .filter_level(cli.verbose.log_level_filter())
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

async fn api_handler(db: DB, cfg: Arc<Config>) {
    let address = cfg.get_string("api.address").unwrap();
    let port = cfg.get_int("api.port").unwrap();
    let host = settings::host(address, port, false);

    debug!("api - binding listener to {}", host);

    let api_listener = TcpListener::bind(host).await.unwrap();
    let app = api::app(db.clone());

    axum::serve(api_listener, app).await.unwrap();
}

async fn bmp_handler(db: DB, cfg: Arc<Config>, tx: Sender<Vec<u8>>) {
    let address = cfg.get_string("bmp.address").unwrap();
    let port = cfg.get_int("bmp.port").unwrap();
    let host = settings::host(address, port, false);

    debug!("bmp - binding listener to {}", host);

    let bmp_listener = TcpListener::bind(host).await.unwrap();
    loop {
        let (mut bmp_socket, _) = bmp_listener.accept().await.unwrap();
        let bmp_db = db.clone();
        let tx = tx.clone();

        // We spawn a new task for each BMP connection
        tokio::spawn(async move {
            bmp::handle(&mut bmp_socket, bmp_db.clone(), tx).await;
        });
    }
}

async fn producer_handler(cfg: Arc<Config>, rx: Receiver<Vec<u8>>) {
    let cfg = settings::get_kafka_config(&cfg).unwrap();

    producer::handle(&cfg, rx).await;
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = CLI::parse();

    let cfg = load_settings(&cli.config);
    let db = db::new_db().await?;
    let shutdown: Shutdown = Shutdown::default();

    set_logging(&cli);

    let (tx, rx) = channel();

    let api_task = shutdown.spawn_task(api_handler(db.clone(), cfg.clone()));
    let bmp_task = shutdown.spawn_task(bmp_handler(db.clone(), cfg.clone(), tx));
    let producer_task = shutdown.spawn_task(producer_handler(cfg.clone(), rx));

    tokio::select! {
        _ = shutdown.shutdown_with_limit(Duration::from_secs(1)) => {
            info!("shutdown - gracefully after shutdown signal received");
        },
        _ = api_task => {
            info!("api - handler shutdown");
        }
        _ = bmp_task => {
            info!("bmp - handler shutdown");
        }
        _ = producer_task => {
            info!("producer - handler shutdown");
        }
    }

    Ok(())
}
