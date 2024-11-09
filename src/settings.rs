use config::Config;
use core::net::IpAddr;
use std::error::Error;

pub struct KafkaConfig {
    pub host: String,
    pub topic: String,
    pub batch_size: u64,
    pub batch_wait: u64,
}

pub fn get_kafka_config(settings: &Config) -> Result<KafkaConfig, Box<dyn Error>> {
    // TODO: better error handling
    // Right now the thread will panic if the settings are not found,
    // but not the entire program
    let kafka_addr = settings.get_string("kafka.address")?;
    let kafka_port = settings.get_int("kafka.port")?;
    let host = host(kafka_addr, kafka_port, true);

    let topic = settings.get_string("kafka.topic")?;
    let batch_size = settings.get_int("kafka.batch_size").unwrap_or(1) as u64;
    let batch_wait = settings.get_int("kafka.batch_wait").unwrap_or(1) as u64;

    Ok(KafkaConfig {
        host,
        topic,
        batch_size,
        batch_wait,
    })
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
