use metrics::{counter, Label};
use rdkafka::config::ClientConfig;
use rdkafka::message::OwnedHeaders;
use rdkafka::producer::{FutureProducer, FutureRecord};
use risotto_lib::update::Update;
use std::sync::mpsc::Receiver;
use std::time::Duration;
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

pub async fn handle(config: &KafkaConfig, rx: Receiver<Update>) {
    // Configure Kafka authentication
    let kafka_auth = match config.auth_protocol.as_str() {
        "PLAINTEXT" => KafkaAuth::PlainText,
        "SASL_PLAINTEXT" => KafkaAuth::SasalPlainText(SaslAuth {
            username: config.auth_sasl_username.clone(),
            password: config.auth_sasl_password.clone(),
            mechanism: config.auth_sasl_mechanism.clone(),
        }),
        _ => {
            error!("invalid Kafka producer authentication protocol");
            return;
        }
    };

    if config.enable == false {
        debug!("producer disabled");
        loop {
            rx.recv().unwrap();
        }
    }

    let producer: &FutureProducer = match kafka_auth {
        KafkaAuth::PlainText => &ClientConfig::new()
            .set("bootstrap.servers", config.brokers.clone())
            .set("message.timeout.ms", "5000")
            .create()
            .expect("Producer creation error"),
        KafkaAuth::SasalPlainText(scram_auth) => &ClientConfig::new()
            .set("bootstrap.servers", config.brokers.clone())
            .set("message.timeout.ms", "5000")
            .set("sasl.username", scram_auth.username)
            .set("sasl.password", scram_auth.password)
            .set("sasl.mechanisms", scram_auth.mechanism)
            .set("security.protocol", "SASL_PLAINTEXT")
            .create()
            .expect("Producer creation error"),
    };

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
            Ok((partition, offset)) => {
                counter!(metric_name, vec![Label::new("status", "success")]).increment(1);
                debug!(
                    "successfully sent message to partition {} at offset {}",
                    partition, offset
                );
            }
            Err((error, _)) => {
                counter!(metric_name, vec![Label::new("status", "failed")]).increment(1);
                error!("failed to send message: {}", error);
            }
        }
    }
}
