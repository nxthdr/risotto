use config::Config;
use core::net::IpAddr;

pub fn is_kafka_enabled(settings: &Config) -> bool {
    settings.get_bool("kafka.enabled").unwrap_or(false)
}

pub fn get_kafka_details(settings: &Config) -> (String, String) {
    // Get Kafka information from settings
    let kafka_addr = settings.get_string("kafka.host").unwrap();
    let kafka_port = settings.get_int("kafka.port").unwrap();
    let kafka_host = host(kafka_addr, kafka_port, true);
    let kafka_topic = settings.get_string("kafka.topic").unwrap();
    return (kafka_host, kafka_topic);
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
