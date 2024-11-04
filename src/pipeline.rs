use std::time::Duration;

use config::Config;
use kafka::error::Error as KafkaError;
use kafka::producer::{Producer, Record, RequiredAcks};
use log::error;

use crate::settings::{get_kafka_details, is_kafka_enabled};

pub fn send_to_kafka(settings: &Config, data: &[u8]) {
    if !is_kafka_enabled(settings) {
        return;
    }

    let (broker, topic) = get_kafka_details(settings);

    if let Err(e) = produce_message(data, &topic, vec![broker.to_owned()]) {
        error!("failed producing messages: {}", e);
    }
}

fn produce_message<'a, 'b>(
    data: &'a [u8],
    topic: &'b str,
    brokers: Vec<String>,
) -> Result<(), KafkaError> {
    let mut producer = Producer::from_hosts(brokers)
        .with_ack_timeout(Duration::from_secs(1))
        .with_required_acks(RequiredAcks::One)
        .create()?;

    producer.send(&Record::from_value(topic, data))?;
    Ok(())
}
