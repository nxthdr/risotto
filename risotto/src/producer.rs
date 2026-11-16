use anyhow::Result;
use metrics::counter;
use rdkafka::config::ClientConfig;
use rdkafka::message::OwnedHeaders;
use rdkafka::producer::{FutureProducer, FutureRecord};
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use tracing::{debug, error, trace};

use risotto_lib::event::Event;

use crate::config::KafkaConfig;
use crate::serializer::serialize_event;

pub async fn handle(config: &KafkaConfig, mut rx: Receiver<Event>) -> Result<()> {
    if config.disable {
        debug!("producer disabled");
        while rx.recv().await.is_some() {
            // Just consume and drop
        }
        return Ok(());
    }

    let producer = new_producer(config)?;

    // Events are serialized and concatenated into Kafka messages of at most
    // `message_max_bytes`, flushed at least every `batch_wait_time` milliseconds.
    let mut batch: Vec<u8> = Vec::new();
    let mut n_events = 0;
    let mut flush_interval = tokio::time::interval(Duration::from_millis(config.batch_wait_time));
    flush_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            event = rx.recv() => {
                let Some(event) = event else {
                    // All senders are gone: flush what is left and stop
                    send(&producer, config, &mut batch, &mut n_events).await;
                    return Ok(());
                };
                trace!("{:?}", event);

                let message = serialize_event(&event);
                if !batch.is_empty() && batch.len() + message.len() > config.message_max_bytes {
                    send(&producer, config, &mut batch, &mut n_events).await;
                }
                batch.extend_from_slice(&message);
                n_events += 1;
            }
            _ = flush_interval.tick() => {
                send(&producer, config, &mut batch, &mut n_events).await;
            }
        }
    }
}

fn new_producer(config: &KafkaConfig) -> Result<FutureProducer> {
    let kafka_brokers = config
        .brokers
        .iter()
        .map(|addr| addr.to_string())
        .collect::<Vec<_>>()
        .join(",");
    debug!(
        "Kafka producer enabled, connecting to brokers: {}",
        kafka_brokers
    );

    let mut client_config = ClientConfig::new();
    client_config
        .set("bootstrap.servers", kafka_brokers)
        .set("message.timeout.ms", config.message_timeout_ms.to_string());

    match config.auth_protocol.as_str() {
        "PLAINTEXT" => {}
        "SASL_PLAINTEXT" => {
            client_config
                .set("sasl.username", &config.auth_sasl_username)
                .set("sasl.password", &config.auth_sasl_password)
                .set("sasl.mechanisms", &config.auth_sasl_mechanism)
                .set("security.protocol", "SASL_PLAINTEXT");
        }
        _ => anyhow::bail!("invalid Kafka producer authentication protocol"),
    }

    Ok(client_config.create()?)
}

async fn send(
    producer: &FutureProducer,
    config: &KafkaConfig,
    batch: &mut Vec<u8>,
    n_events: &mut usize,
) {
    if batch.is_empty() {
        return;
    }

    debug!("sending {} events to Kafka", n_events);
    let delivery_status = producer
        .send(
            FutureRecord::to(config.topic.as_str())
                .payload(batch.as_slice())
                .key("")
                .headers(OwnedHeaders::new()),
            Duration::from_secs(0),
        )
        .await;

    let metric_name = "risotto_kafka_messages_total";
    match delivery_status {
        Ok(delivery) => {
            counter!(metric_name, "status" => "success").increment(1);
            debug!(
                "successfully sent message to partition {} at offset {}",
                delivery.partition, delivery.offset
            );
        }
        Err((error, _)) => {
            counter!(metric_name, "status" => "failure").increment(1);
            error!("failed to send message: {}", error);
        }
    }

    batch.clear();
    *n_events = 0;
}
