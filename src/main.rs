mod api;
mod bmp;
mod db;
mod pipeline;
mod router;
mod settings;
mod update;

use chrono::Local;
use clap::Parser;
use config::Config;
use env_logger::Builder;
use log::debug;
use std::error::Error;
use std::io::Write;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::try_join;
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

fn load_settings(config_path: &str) -> Config {
    Config::builder()
        .add_source(config::File::with_name(config_path))
        .add_source(config::Environment::with_prefix("RISOTTO"))
        .build()
        .unwrap()
}

async fn api_handler(settings: Config, db: DB) {
    let address = settings.get_string("api.address").unwrap();
    let port = settings.get_int("api.port").unwrap();
    let host = settings::host(address, port, false);

    debug!("binding API listener to {}", host);

    let api_listener = TcpListener::bind(host).await.unwrap();
    let app = api::app(db.clone());

    axum::serve(api_listener, app).await.unwrap();
}

async fn bmp_handler(settings: Config, db: DB) {
    let address = settings.get_string("bmp.address").unwrap();
    let port = settings.get_int("bmp.port").unwrap();
    let host = settings::host(address, port, false);

    debug!("binding BMP listener to {}", host);

    let bmp_listener = TcpListener::bind(host).await.unwrap();
    loop {
        let (mut bmp_socket, _) = bmp_listener.accept().await.unwrap();
        let bmp_db = db.clone();
        let bmp_settings = settings.clone();
        tokio::spawn(async move {
            loop {
                bmp::handle(&mut bmp_socket, bmp_db.clone(), bmp_settings.clone()).await
            }
        });
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = CLI::parse();

    let settings = load_settings(&cli.config);
    let db = db::new_db().await?;
    let shutdown: Shutdown = Shutdown::default();

    set_logging(&cli);

    let api_handler = shutdown.spawn_task(api_handler(settings.clone(), db.clone()));
    let bmp_handler = shutdown.spawn_task(bmp_handler(settings.clone(), db.clone()));

    shutdown.shutdown_with_limit(Duration::from_secs(1)).await?;
    try_join!(api_handler, bmp_handler)?;

    Ok(())
}
