use anyhow::Result;
use clap::Parser;
use clap_verbosity_flag::{InfoLevel, Verbosity};
use futures::future::try_join_all;
use metrics_exporter_prometheus::PrometheusBuilder;
use std::net::SocketAddr;
use tokio::net::lookup_host;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bmp: BMPConfig,
    pub kafka: KafkaConfig,
    pub state: StateConfig,
}

#[derive(Debug, Clone)]
pub struct BMPConfig {
    pub host: SocketAddr,
}

#[derive(Debug, Clone)]
pub struct KafkaConfig {
    pub disable: bool,
    pub brokers: Vec<SocketAddr>,
    pub topic: String,
    pub auth_protocol: String,
    pub auth_sasl_username: String,
    pub auth_sasl_password: String,
    pub auth_sasl_mechanism: String,
    pub message_max_bytes: usize,
    pub message_timeout_ms: usize,
    pub batch_wait_time: u64,
    pub batch_wait_interval: u64,
    pub mpsc_buffer_size: usize,
}

#[derive(Debug, Clone)]
pub struct StateConfig {
    pub disable: bool,
    pub path: String,
    pub interval: u64,
}

#[derive(Parser, Debug)]
#[command(author, version, about = "Risotto BGP Monitoring Protocol (BMP) collector", long_about = None)]
pub struct Cli {
    /// BMP listener address (IP or FQDN)
    #[arg(long, default_value = "0.0.0.0:4000")]
    pub bmp_address: String,

    /// Kafka brokers (comma-separated list of address:port)
    #[arg(long, value_delimiter(','), default_value = "localhost:9092")]
    pub kafka_brokers: Vec<String>,

    /// Disable Kafka producer
    #[arg(long)]
    pub kafka_disable: bool,

    /// Kafka producer topic
    #[arg(long, default_value = "risotto-updates")]
    pub kafka_topic: String,

    /// Kafka Authentication Protocol (e.g., PLAINTEXT, SASL_PLAINTEXT)
    #[arg(long, default_value = "PLAINTEXT")]
    pub kafka_auth_protocol: String,

    /// Kafka Authentication SASL Username
    #[arg(long, default_value = "risotto")]
    pub kafka_auth_sasl_username: String,

    /// Kafka Authentication SASL Password
    #[arg(long, default_value = "risotto")]
    pub kafka_auth_sasl_password: String,

    /// Kafka Authentication SASL Mechanism (e.g., PLAIN, SCRAM-SHA-512)
    #[arg(long, default_value = "SCRAM-SHA-512")]
    pub kafka_auth_sasl_mechanism: String,

    /// Kafka message max bytes
    #[arg(long, default_value_t = 990000)]
    pub kafka_message_max_bytes: usize,

    /// Kafka producer batch size (bytes)
    #[arg(long, default_value_t = 500000)]
    pub kafka_message_timeout_ms: usize,

    /// Kafka producer batch wait time (ms)
    #[arg(long, default_value_t = 1000)]
    pub kafka_batch_wait_time: u64,

    /// Kafka producer batch wait interval (ms)
    #[arg(long, default_value_t = 100)]
    pub kafka_batch_wait_interval: u64,

    /// Kafka MPSC bufer size
    #[arg(long, default_value_t = 100000)]
    pub kafka_mpsc_buffer_size: usize,

    /// Metrics listener address (IP or FQDN) for Prometheus endpoint
    #[arg(long, default_value = "0.0.0.0:8080")]
    pub metrics_address: String,

    /// Disable state saving
    #[arg(long)]
    pub state_disable: bool,

    /// Path to save state file
    #[arg(long, default_value = "state.bin")]
    pub state_path: String,

    /// Interval (in seconds) to save state
    #[arg(long, default_value_t = 10)]
    pub state_interval: u64,

    /// Set the verbosity level
    #[command(flatten)]
    pub verbose: Verbosity<InfoLevel>,
}

pub async fn resolve_address(address: String) -> Result<SocketAddr> {
    match lookup_host(&address).await?.next() {
        Some(addr) => Ok(addr),
        None => anyhow::bail!("Failed to resolve address: {}", address),
    }
}

fn set_logging(cli: &Cli) -> Result<()> {
    let subscriber = tracing_subscriber::fmt()
        .compact()
        .with_file(true)
        .with_line_number(true)
        .with_max_level(cli.verbose)
        .with_target(true)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;
    Ok(())
}

fn set_metrics(metrics_address: SocketAddr) {
    let prom_builder = PrometheusBuilder::new();
    prom_builder
        .with_http_listener(metrics_address)
        .install()
        .expect("Failed to install Prometheus metrics exporter");

    // Producer metrics
    metrics::describe_counter!(
        "risotto_kafka_messages_total",
        "Total number of Kafka messages produced"
    );

    // State metrics
    metrics::describe_counter!("risotto_state_dump_total", "Total number of state dumps");
    metrics::describe_counter!(
        "risotto_state_dump_duration_seconds",
        "Duration of state dump in seconds"
    );
    metrics::describe_gauge!("risotto_peer_established", "Peer established for a router");
    metrics::describe_gauge!("risotto_state_updates", "Number of updates in the state");

    // Statistics metrics
    metrics::describe_counter!(
        "risotto_bmp_messages_total",
        "Total number of BMP messages received"
    );
    metrics::describe_counter!(
        "risotto_rx_updates_total",
        "Total number of updates received"
    );
    metrics::describe_counter!(
        "risotto_tx_updates_total",
        "Total number of updates transmitted"
    );
}

pub async fn configure() -> Result<AppConfig> {
    let cli = Cli::parse();

    // Set up tracing
    set_logging(&cli).map_err(|e| anyhow::anyhow!("Failed to set up logging: {}", e))?;

    // Resolve addresses
    let (bmp_addr, metrics_addr) = tokio::try_join!(
        resolve_address(cli.bmp_address),
        resolve_address(cli.metrics_address)
    )
    .map_err(|e| anyhow::anyhow!("Failed during initial address resolution: {}", e))?;

    let resolved_kafka_brokers = if !cli.kafka_disable {
        try_join_all(
            cli.kafka_brokers
                .iter()
                .map(|broker| resolve_address(broker.clone())),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to resolve Kafka broker: {}", e))?
    } else {
        Vec::new()
    };

    // Set up metrics
    set_metrics(metrics_addr);

    Ok(AppConfig {
        bmp: BMPConfig { host: bmp_addr },
        kafka: KafkaConfig {
            disable: cli.kafka_disable,
            brokers: resolved_kafka_brokers,
            topic: cli.kafka_topic,
            auth_protocol: cli.kafka_auth_protocol,
            auth_sasl_username: cli.kafka_auth_sasl_username,
            auth_sasl_password: cli.kafka_auth_sasl_password,
            auth_sasl_mechanism: cli.kafka_auth_sasl_mechanism,
            message_max_bytes: cli.kafka_message_max_bytes,
            message_timeout_ms: cli.kafka_message_timeout_ms,
            batch_wait_time: cli.kafka_batch_wait_time,
            batch_wait_interval: cli.kafka_batch_wait_interval,
            mpsc_buffer_size: cli.kafka_mpsc_buffer_size,
        },
        state: StateConfig {
            disable: cli.state_disable,
            path: cli.state_path,
            interval: cli.state_interval,
        },
    })
}
