use config::Config;
use core::net::IpAddr;
use std::error::Error;

pub struct KafkaConfig {
    pub host: String,
    pub topic: String,
    pub batch_max_size: u64,
    pub batch_interval: u64,
}

pub fn get_kafka_config(settings: &Config) -> Result<KafkaConfig, Box<dyn Error>> {
    // TODO: better error handling
    // Right now the thread will panic if the settings are not found,
    // but not the entire program
    let kafka_addr = settings.get_string("kafka.address")?;
    let kafka_port = settings.get_int("kafka.port")?;
    let host = host(kafka_addr, kafka_port, true);

    let topic = settings.get_string("kafka.topic")?;
    let batch_max_size = settings.get_int("kafka.batch_max_size").unwrap_or(100) as u64;
    let batch_interval = settings.get_int("kafka.batch_interval").unwrap_or(1) as u64;

    Ok(KafkaConfig {
        host,
        topic,
        batch_max_size,
        batch_interval,
    })
}

pub struct StateConfig {
    pub host: String,
}

pub fn get_state_config(settings: &Config) -> Result<StateConfig, Box<dyn Error>> {
    let redis_addr = settings.get_string("state.address")?;
    let redis_port = settings.get_int("state.port")?;
    let host = host(redis_addr, redis_port, true);

    Ok(StateConfig { host })
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
    return format!("{}:{}", host, port);
}
