use config::Config;
use core::net::IpAddr;
use std::error::Error;

pub struct KafkaConfig {
    pub enabled: bool,
    pub host: String,
    pub topic: String,
    pub batch_size: i64,
}

pub fn get_kafka_config(settings: &Config) -> Result<KafkaConfig, Box<dyn Error>> {
    // TODO: better error handling
    // Right now the thread will panic if the settings are not found,
    // but not the entire program
    let enabled = settings.get_bool("kafka.enabled").unwrap_or(true);
    let kafka_addr = settings.get_string("kafka.address")?;
    let kafka_port = settings.get_int("kafka.port")?;
    let kafka_host = host(kafka_addr, kafka_port, true);
    let kafka_topic = settings.get_string("kafka.topic")?;

    // This is the number of messages to batch per BMP Route Monitoring message
    // Not configurable, as it can be misleading
    let batch_size = 100;

    Ok(KafkaConfig {
        enabled: enabled,
        host: kafka_host,
        topic: kafka_topic,
        batch_size: batch_size,
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
