use anyhow::Result;
use log::{debug, info, trace, warn};
use rdkafka::config::ClientConfig;
use rdkafka::message::OwnedHeaders;
use rdkafka::producer::{FutureProducer, FutureRecord};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crate::config::KafkaConfig;

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

pub async fn handle(config: &KafkaConfig, rx: Receiver<String>) -> Result<()> {
    // Configure Kafka authentication
    let kafka_auth = match config.auth_protocol.as_str() {
        "PLAINTEXT" => KafkaAuth::PlainText,
        "SASL_PLAINTEXT" => KafkaAuth::SasalPlainText(SaslAuth {
            username: config.auth_sasl_username.clone(),
            password: config.auth_sasl_password.clone(),
            mechanism: config.auth_sasl_mechanism.clone(),
        }),
        _ => {
            return Err(anyhow::anyhow!(
                "Invalid Kafka producer authentication protocol"
            ))
        }
    };

    if config.enable == false {
        warn!("producer - disabled");
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
    let mut additional_message: Option<String> = None;
    loop {
        let start_time = std::time::Instant::now();
        let mut final_message = String::new();
        let mut n_messages = 0;

        // Send the additional message first
        if let Some(message) = additional_message {
            final_message.push_str(&message);
            final_message.push_str("\n");
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
            trace!("producer - received - {}", message);

            if message.len() + message.len() + 1 > config.message_max_bytes {
                additional_message = Some(message);
                break;
            }

            final_message.push_str(&message);
            final_message.push_str("\n");
            n_messages += 1;
        }

        if final_message.is_empty() {
            continue;
        }

        // Remove the last newline character
        final_message.pop();

        debug!("producer - {}", final_message);
        info!("producer - sending {} updates to Kafka", n_messages);

        let delivery_status = producer
            .send(
                FutureRecord::to(config.topic.as_str())
                    .payload(&format!("{}", final_message))
                    .key("")
                    .headers(OwnedHeaders::new()),
                Duration::from_secs(0),
            )
            .await;

        info!("producer - {:?}", delivery_status);
    }
}
