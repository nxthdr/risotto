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
use log::LevelFilter;
use std::error::Error;
use std::io::Write;
use tokio::net::TcpListener;
use tokio::try_join;

use crate::db::DB;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct CLI {
    #[arg(short, long)]
    config: String,

    #[arg(short, long)]
    debug: bool,
}

fn set_logging(cli: &CLI) {
    let level_filter = if cli.debug {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };

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
        .filter(None, level_filter)
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

    debug!("Binding API listener to {}", host);

    let api_listener = TcpListener::bind(host).await.unwrap();
    let app = api::app(db.clone());

    axum::serve(api_listener, app).await.unwrap();
}

async fn bmp_handler(settings: Config, db: DB) {
    let address = settings.get_string("bmp.address").unwrap();
    let port = settings.get_int("bmp.port").unwrap();
    let host = settings::host(address, port, false);

    debug!("Binding BMP listener to {}", host);

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

    set_logging(&cli);

    let api_handler = tokio::spawn(api_handler(settings.clone(), db.clone()));
    let bmp_handler = tokio::spawn(bmp_handler(settings.clone(), db.clone()));

    try_join!(api_handler, bmp_handler)?;

    Ok(())
}
