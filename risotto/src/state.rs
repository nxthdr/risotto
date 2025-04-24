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

use crate::config::StateConfig;

pub async fn dump<T: StateStore + Serialize>(state: AsyncState<T>, cfg: StateConfig) {
    let state_lock = state.lock().await;
    let temp_path = format!("{}.tmp", cfg.path);

    match bincode::serde::encode_to_vec(&state_lock.store, bincode::config::legacy()) {
        Ok(encoded) => {
            match File::create(&temp_path).await {
                Ok(mut file) => {
                    if let Err(e) = file.write_all(&encoded).await {
                        error!(
                            "Failed to write serialized state to temporary file {}: {}",
                            temp_path, e
                        );
                        let _ = tokio::fs::remove_file(&temp_path).await;
                        return;
                    }

                    if let Err(e) = file.sync_all().await {
                        error!("Failed to sync temporary state file {}: {}", temp_path, e);
                        let _ = tokio::fs::remove_file(&temp_path).await;
                        return;
                    }
                }
                Err(e) => {
                    error!("Failed to create temporary state file {}: {}", temp_path, e);
                    return;
                }
            }

            if let Err(e) = rename(&temp_path, cfg.path.clone()).await {
                error!(
                    "Failed to rename temporary state file {} to {}: {}",
                    temp_path, cfg.path, e
                );
                let _ = tokio::fs::remove_file(&temp_path).await;
            } else {
                trace!("Successfully dumped state to {}", cfg.path);
            }
        }
        Err(e) => {
            error!("Failed to serialize state using bincode: {}", e);
        }
    }
}

pub async fn load<T: StateStore + for<'de> Deserialize<'de>>(
    state: AsyncState<T>,
    cfg: StateConfig,
) {
    debug!("Attempting to load state from {}", cfg.path);
    match tokio::fs::read(&cfg.path).await {
        Ok(encoded) => {
            match bincode::serde::decode_from_slice::<T, _>(&encoded, bincode::config::legacy()) {
                Ok((store, _)) => {
                    debug!("Successfully deserialized state from {}", cfg.path);
                    debug!("Initializing state metrics from loaded data...");

                    let peers = store.get_peers();
                    for (router_addr, peer_addr) in peers {
                        let updates = store.get_updates_by_peer(&router_addr, &peer_addr);
                        gauge!("risotto_state_updates", "router" => router_addr.to_string(), "peer" => peer_addr.to_string()).set(updates.len() as f64);
                    }

                    let mut state_lock = state.lock().await;
                    state_lock.store = store;
                    info!("State loaded and metrics initialized successfully.");
                }
                Err(e) => {
                    error!(
                        "Failed to deserialize state from {} using bincode: {}",
                        cfg.path, e
                    );
                    let backup_path = format!("{}.corrupted-{}", cfg.path, Utc::now().timestamp());
                    warn!("Renaming corrupted state file to {}", backup_path);
                    let _ = rename(&cfg.path, backup_path).await;
                }
            }
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                info!(
                    "State file {} not found. Starting with empty state.",
                    cfg.path
                );
            } else {
                error!("Failed to read state file {}: {}", cfg.path, e);
            }
        }
    }
}

pub async fn dump_handler<T: StateStore + Serialize>(
    state: Option<AsyncState<T>>,
    cfg: StateConfig,
) {
    loop {
        // TODO do not spawn this task if state is disabled
        sleep(Duration::from_secs(cfg.interval)).await;
        if let Some(ref state) = state {
            trace!("dumping state to {}", cfg.path);
            let start = std::time::Instant::now();
            dump(state.clone(), cfg.clone()).await;
            let duration = start.elapsed();
            trace!("State dump finished in {:?}", duration);
            counter!("risotto_state_dump_total").increment(1);
            histogram!("risotto_state_dump_duration_seconds").record(duration.as_secs_f64());
        }
    }
}
