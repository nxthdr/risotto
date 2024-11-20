use std::time::Duration;

use crate::settings::KafkaConfig;
use kafka::client::{Compression, DEFAULT_CONNECTION_IDLE_TIMEOUT_MILLIS};
use kafka::producer::{AsBytes, Producer, Record, RequiredAcks, DEFAULT_ACK_TIMEOUT_MILLIS};
use log::error;
use std::error::Error;
use std::io::BufRead;
use std::io::Cursor;
use std::ops::{Deref, DerefMut};
use std::sync::mpsc::{Receiver, TryRecvError};

struct Trimmed(String);

impl AsBytes for Trimmed {
    fn as_bytes(&self) -> &[u8] {
        self.0.trim().as_bytes()
    }
}

impl Deref for Trimmed {
    type Target = String;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Trimmed {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

fn produce_impl(
    producer: &mut Producer,
    cfg: &KafkaConfig,
    data: &mut dyn BufRead,
) -> Result<usize, Box<dyn Error>> {
    // assert!(cfg.batch_max_size > 1);

    // ~ a buffer of prepared records to be send in a batch to Kafka
    // ~ in the loop following, we'll only modify the 'value' of the
    // cached records
    let mut rec_stash: Vec<Record<'_, (), Trimmed>> = (0..cfg.batch_max_size)
        .map(|_| Record::from_value(&cfg.topic, Trimmed(String::new())))
        .collect();

    // ~ points to the next free slot in `rec_stash`.  if it reaches
    // `rec_stash.len()` we'll send `rec_stash` to kafka
    let mut next_rec = 0;
    let mut n_rec = 0;
    loop {
        // ~ send out a batch if it's ready
        if next_rec == rec_stash.len() {
            send_batch(producer, &rec_stash)?;
            next_rec = 0;
        }
        let rec = &mut rec_stash[next_rec];
        rec.value.clear();
        if data.read_line(&mut rec.value)? == 0 {
            break; // ~ EOF reached
        }
        if rec.value.trim().is_empty() {
            continue; // ~ skip empty lines
        }
        // ~ ok, we got a line. read the next one in a new buffer
        next_rec += 1;
        n_rec += 1;
    }
    // ~ flush pending messages - if any
    if next_rec > 0 {
        send_batch(producer, &rec_stash[..next_rec])?;
    }
    Ok(n_rec)
}

fn send_batch(
    producer: &mut Producer,
    batch: &[Record<'_, (), Trimmed>],
) -> Result<(), Box<dyn Error>> {
    let rs = producer.send_all(batch)?;

    for r in rs {
        for tpc in r.partition_confirms {
            if let Err(code) = tpc.offset {
                return Err(Box::new(kafka::error::Error::Kafka(code)));
            }
        }
    }

    Ok(())
}

pub async fn handle(cfg: &KafkaConfig, rx: Receiver<Vec<u8>>) {
    // TODO: Allow multiple brokers via the config file
    let mut client = kafka::client::KafkaClient::new(vec![cfg.host.to_owned()]);

    // Wait until the metadata we succeed to reach the Kafka brokers
    loop {
        match client.load_metadata(&[cfg.topic.to_owned()]) {
            Ok(_) => {
                log::debug!("producer - metadata loaded");
                break;
            }
            Err(_) => {
                error!("producer - failed to load metadata: retrying in 5 seconds");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }

    // TODO: Allow compression setting via the config file
    // TODO: Allow timeouts settins via the config file
    let mut producer = Producer::from_client(client)
        .with_ack_timeout(Duration::from_millis(DEFAULT_ACK_TIMEOUT_MILLIS))
        .with_connection_idle_timeout(Duration::from_millis(
            DEFAULT_CONNECTION_IDLE_TIMEOUT_MILLIS,
        ))
        .with_required_acks(RequiredAcks::One)
        .with_compression(Compression::NONE)
        .create()
        .unwrap();

    loop {
        // Wait the batch wait time to collect messages
        tokio::time::sleep(Duration::from_secs(cfg.batch_interval)).await;
        let mut data = Vec::new();
        loop {
            // Collect all of the messages from BMP handler
            match rx.try_recv() {
                Ok(d) => data.extend(d),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    log::error!("producer - BMP handler disconnected");
                    return;
                }
            }
        }

        // If no data was collected within the batch waiting time,
        // continue to the next iteration
        if data.is_empty() {
            log::debug!("producer - produced 0 messages");
            continue;
        }

        // Send the collected messages to Kafka in batches
        let mut data = Cursor::new(data);
        match produce_impl(&mut producer, &cfg, &mut data) {
            Ok(n) => {
                log::info!("producer - produced {} messages", n)
            }
            Err(e) => {
                error!("producer - failed producing messages: {}", e);
            }
        };
    }
}
