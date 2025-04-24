use metrics::counter;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::fs::{rename, File};
use tokio::time::sleep;
use tracing::trace;

use risotto_lib::state::AsyncState;
use risotto_lib::state_store::store::StateStore;

use crate::config::StateConfig;

pub async fn dump<T: StateStore + Serialize>(state: AsyncState<T>, cfg: StateConfig) {
    let state_lock = state.lock().await;
    let temp_path = format!("{}.tmp", cfg.path);
    let file = File::create(&temp_path).await.unwrap();
    let mut writer = std::io::BufWriter::new(file.into_std().await);
    serde_json::to_writer(&mut writer, &state_lock.store).unwrap();
    rename(temp_path, cfg.path.clone()).await.unwrap();
}

pub async fn load<T: StateStore + for<'de> Deserialize<'de>>(
    state: AsyncState<T>,
    cfg: StateConfig,
) {
    let mut state_lock = state.lock().await;

    let file = match File::open(cfg.path.clone()).await {
        Ok(file) => file,
        Err(_) => return,
    };

    let reader = std::io::BufReader::new(file.into_std().await);
    let store = serde_json::from_reader(reader).unwrap();
    state_lock.store = store;
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
            dump(state.clone(), cfg.clone()).await;
            counter!("risotto_state_dump_total").increment(1);
        }
    }
}
