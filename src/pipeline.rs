use std::time::Duration;

use kafka::error::Error as KafkaError;
use kafka::producer::{Producer, Record, RequiredAcks};
use log::error;

pub fn send_to_kafka(broker: &str, topic: &str, data: &[u8]) {
    if let Err(e) = produce_message(data, topic, vec![broker.to_owned()]) {
        error!("Failed producing messages: {}", e);
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
