use config::Config;
use core::net::IpAddr;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub api: APIConfig,
    pub bmp: BMPConfig,
    pub kafka: KafkaConfig,
    pub state: StateConfig,
}

fn load_config(config_path: &str) -> Config {
    let cfg = Config::builder()
        .add_source(config::File::with_name(config_path))
        .add_source(config::Environment::with_prefix("RISOTTO"))
        .build()
        .unwrap();
    cfg
}

pub fn app_config(config_path: &str) -> AppConfig {
    let config = load_config(config_path);
    let api = get_api_config(&config);
    let bmp = get_bmp_config(&config);
    let kafka = get_kafka_config(&config);
    let state = get_state_config(&config);
    AppConfig {
        api,
        bmp,
        kafka,
        state,
    }
}

#[derive(Debug, Clone)]
pub struct APIConfig {
    pub host: String,
}

pub fn get_api_config(config: &Config) -> APIConfig {
    let api_addr = config
        .get_string("api.address")
        .unwrap_or("localhost".to_string());
    let api_port = config.get_int("api.port").unwrap_or(3000);

    APIConfig {
        host: host(api_addr, api_port, false),
    }
}

#[derive(Debug, Clone)]
pub struct BMPConfig {
    pub host: String,
}

pub fn get_bmp_config(config: &Config) -> BMPConfig {
    let bmp_addr = config
        .get_string("bmp.address")
        .unwrap_or("localhost".to_string());
    let bmp_port = config.get_int("bmp.port").unwrap_or(4000);

    BMPConfig {
        host: host(bmp_addr, bmp_port, true),
    }
}

#[derive(Debug, Clone)]
pub struct KafkaConfig {
    /// Kafka brokers
    /// Default: localhost:9092
    pub brokers: String,

    /// Kafka Authentication Protocol
    /// Default: PLAINTEXT
    pub auth_protocol: String,

    /// Kafka Authentication SASL Username
    /// Default: saimiris
    pub auth_sasl_username: String,

    /// Kafka Authentication SASL Password
    /// Default: saimiris
    pub auth_sasl_password: String,

    /// Kafka Authentication SASL Mechanism
    /// Default: SCRAM-SHA-512
    pub auth_sasl_mechanism: String,

    /// Enable Kafka producer
    /// Default: true
    pub enable: bool,

    /// Kafka producer topic
    /// Default: saimiris-results
    pub topic: String,

    /// Kafka message max bytes
    /// Default: 1048576
    pub message_max_bytes: usize,

    /// Kafka producer batch wait time
    /// Default: 1000
    pub batch_wait_time: u64,

    /// Kafka producer batch wait interval
    /// Default: 100
    pub batch_wait_interval: u64,
}

pub fn get_kafka_config(config: &Config) -> KafkaConfig {
    KafkaConfig {
        brokers: config
            .get_string("kafka.brokers")
            .unwrap_or("localhost:9092".to_string()),
        auth_protocol: config
            .get_string("kafka.auth_protocol")
            .unwrap_or("PLAINTEXT".to_string()),
        auth_sasl_username: config
            .get_string("kafka.auth_sasl_username")
            .unwrap_or("risotto".to_string()),
        auth_sasl_password: config
            .get_string("kafka.auth_sasl_password")
            .unwrap_or("risotto".to_string()),
        auth_sasl_mechanism: config
            .get_string("kafka.auth_sasl_mechanism")
            .unwrap_or("SCRAM-SHA-512".to_string()),
        enable: config.get_bool("kafka.enable").unwrap_or(true),
        topic: config
            .get_string("kafka.topic")
            .unwrap_or("risotto-updates".to_string()),
        message_max_bytes: config.get_int("kafka.message_max_bytes").unwrap_or(990000) as usize,
        batch_wait_time: config.get_int("kafka.batch_wait_time").unwrap_or(1000) as u64,
        batch_wait_interval: config.get_int("kafka.batch_wait_interval").unwrap_or(100) as u64,
    }
}

#[derive(Debug, Clone)]
pub struct StateConfig {
    pub enable: bool,
    pub path: String,
    pub interval: u64,
}

pub fn get_state_config(config: &Config) -> StateConfig {
    StateConfig {
        enable: config.get_bool("state.enable").unwrap_or(true),
        path: config
            .get_string("state.path")
            .unwrap_or("state.json".to_string()),
        interval: config.get_int("state.save_interval").unwrap_or(10) as u64,
    }
}

pub fn host(address: String, port: i64, accept_fqdn: bool) -> String {
    let host = match address.parse::<IpAddr>() {
        Ok(ip) => {
            if ip.is_ipv4() {
                address
            } else {
                format!("[{}]", ip)
            }
        }
        Err(_) => {
            if accept_fqdn {
                address
            } else {
                panic!("FQDN non supported")
            }
        }
    };
    format!("{}:{}", host, port)
}
