use anyhow::Result;
use metrics::counter;
use rdkafka::config::ClientConfig;
use rdkafka::message::OwnedHeaders;
use rdkafka::producer::{FutureProducer, FutureRecord};
use risotto_lib::update::Update;
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use tracing::{debug, error, trace};

use crate::config::KafkaConfig;
use crate::formatter::serialize_update;

#[derive(Clone)]
pub struct SaslAuth {
    pub username: String,
    pub password: String,
    pub mechanism: String,
}

#[derive(Clone)]
pub enum KafkaAuth {
    SasalPlainText(SaslAuth),
    PlainText,
}

pub async fn handle(config: &KafkaConfig, mut rx: Receiver<Update>) -> Result<()> {
    // Configure Kafka authentication
    let kafka_auth = match config.auth_protocol.as_str() {
        "PLAINTEXT" => KafkaAuth::PlainText,
        "SASL_PLAINTEXT" => KafkaAuth::SasalPlainText(SaslAuth {
            username: config.auth_sasl_username.clone(),
            password: config.auth_sasl_password.clone(),
            mechanism: config.auth_sasl_mechanism.clone(),
        }),
        _ => {
            anyhow::bail!("invalid Kafka producer authentication protocol");
        }
    };

    if config.disable {
        debug!("producer disabled");
        while let Some(_update) = rx.recv().await {
            // Just consume and drop
        }
    }

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

    // Configure Kafka producer
    let mut client_config = ClientConfig::new();
    client_config
        .set("bootstrap.servers", kafka_brokers)
        .set("message.timeout.ms", config.message_timeout_ms.to_string());

    if let KafkaAuth::SasalPlainText(auth) = kafka_auth {
        client_config = client_config
            .set("sasl.username", auth.username)
            .set("sasl.password", auth.password)
            .set("sasl.mechanisms", auth.mechanism)
            .set("security.protocol", "SASL_PLAINTEXT")
            .to_owned();
    }

    let producer: FutureProducer = client_config
        .create()
        .expect("Failed to create Kafka producer");

    // Send to Kafka
    let mut additional_message: Option<Vec<u8>> = None;
    loop {
        let start_time = std::time::Instant::now();
        let mut final_message = Vec::new();
        let mut n_messages = 0;

        // Send the additional message first
        if let Some(message) = additional_message {
            final_message.extend_from_slice(&message);
            n_messages += 1;
            additional_message = None;
        }

        loop {
            if std::time::Instant::now().duration_since(start_time)
                > std::time::Duration::from_millis(config.batch_wait_time)
            {
                break;
            }

            let message = rx.try_recv();
            if message.is_err() {
                tokio::time::sleep(Duration::from_millis(config.batch_wait_interval)).await;
                continue;
            }

            let message = message.unwrap();
            trace!("{:?}", message);

            // Serialize the update
            let message = serialize_update(&message);

            // Max message size is 1048576 bytes (including headers)
            if final_message.len() + message.len() > config.message_max_bytes {
                additional_message = Some(message);
                break;
            }

            final_message.extend_from_slice(&message);
            n_messages += 1;
        }

        if final_message.is_empty() {
            continue;
        }

        debug!("sending {} updates to Kafka", n_messages);
        let delivery_status = producer
            .send(
                FutureRecord::to(config.topic.as_str())
                    .payload(&final_message)
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
    }
}
