use anyhow::Result;
use chrono::Utc;
use metrics::{counter, gauge, histogram};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::fs::{rename, File};
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;
use tracing::{debug, error, info, trace, warn};

use risotto_lib::state::AsyncState;
use risotto_lib::state_store::store::StateStore;

use crate::config::CurationConfig;

pub async fn dump<T: StateStore + Serialize>(state: AsyncState<T>, cfg: CurationConfig) -> Result<()> {
    let state_lock = state.lock().await;
    let temp_path = format!("{}.tmp", cfg.state_path);

    match bincode::serde::encode_to_vec(&state_lock.store, bincode::config::legacy()) {
        Ok(encoded) => {
            match File::create(&temp_path).await {
                Ok(mut file) => {
                    if let Err(e) = file.write_all(&encoded).await {
                        let _ = tokio::fs::remove_file(&temp_path).await;
                        anyhow::bail!(
                            "Failed to write serialized state to temporary file {}: {}",
                            temp_path,
                            e
                        );
                    }

                    if let Err(e) = file.sync_all().await {
                        let _ = tokio::fs::remove_file(&temp_path).await;
                        anyhow::bail!("Failed to sync temporary state file {}: {}", temp_path, e);
                    }
                }
                Err(e) => {
                    anyhow::bail!("Failed to create temporary state file {}: {}", temp_path, e);
                }
            }

            if let Err(e) = rename(&temp_path, cfg.state_path.clone()).await {
                let _ = tokio::fs::remove_file(&temp_path).await;
                anyhow::bail!("Failed to rename temporary state file: {}", e);
            } else {
                trace!("Successfully dumped state to {}", cfg.state_path);
                Ok(())
            }
        }
        Err(e) => {
            anyhow::bail!("Failed to serialize state using bincode: {}", e);
        }
    }
}

pub async fn load<T: StateStore + for<'de> Deserialize<'de>>(
    state: AsyncState<T>,
    cfg: CurationConfig,
) {
    debug!("Attempting to load curation state from {}", cfg.state_path);
    match tokio::fs::read(&cfg.state_path).await {
        Ok(encoded) => {
            match bincode::serde::decode_from_slice::<T, _>(&encoded, bincode::config::legacy()) {
                Ok((store, _)) => {
                    debug!("Successfully deserialized state from {}", cfg.state_path);
                    debug!("Initializing state metrics from loaded data...");

                    let peers = store.get_peers();
                    for (router_addr, peer_addr) in peers {
                        let updates = store.get_updates_by_peer(&router_addr, &peer_addr);
                        gauge!("risotto_state_updates", "router" => router_addr.to_string(), "peer" => peer_addr.to_string()).set(updates.len() as f64);
                    }

                    let mut state_lock = state.lock().await;
                    state_lock.store = store;
                    info!("Curation state loaded and metrics initialized successfully.");
                }
                Err(e) => {
                    error!(
                        "Failed to deserialize curation state from {} using bincode: {}",
                        cfg.state_path, e
                    );
                    let backup_path = format!("{}.corrupted-{}", cfg.state_path, Utc::now().timestamp());
                    warn!("Renaming corrupted state file to {}", backup_path);
                    let _ = rename(&cfg.state_path, backup_path).await;
                }
            }
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                info!(
                    "Curation state file {} not found. Starting with empty state.",
                    cfg.state_path
                );
            } else {
                error!("Failed to read curation state file {}: {}", cfg.state_path, e);
            }
        }
    }
}

pub async fn dump_handler<T: StateStore + Serialize>(
    state: AsyncState<T>,
    cfg: CurationConfig,
) -> Result<()> {
    loop {
        sleep(Duration::from_secs(cfg.state_save_interval)).await;
        trace!("dumping curation state to {}", cfg.state_path);
        let start = std::time::Instant::now();
        dump(state.clone(), cfg.clone()).await?;
        let duration = start.elapsed();
        trace!("State dump finished in {:?}", duration);
        counter!("risotto_state_dump_total").increment(1);
        histogram!("risotto_state_dump_duration_seconds").record(duration.as_secs_f64());
    }
}
