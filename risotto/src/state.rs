use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;
use tracing::debug;

use risotto_lib::state::AsyncState;
use risotto_lib::state_store::store::StateStore;

use crate::config::StateConfig;
pub fn dump<T: StateStore + Serialize>(state: AsyncState<T>, cfg: StateConfig) {
    let state_lock = state.lock().unwrap();
    let temp_path = format!("{}.tmp", cfg.path);
    let file = std::fs::File::create(&temp_path).unwrap();
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer(&mut writer, &state_lock.store).unwrap();
    std::fs::rename(temp_path, cfg.path.clone()).unwrap();
}

pub fn load<T: StateStore + for<'de> Deserialize<'de>>(state: AsyncState<T>, cfg: StateConfig) {
    let mut state_lock = state.lock().unwrap();

    let file = match std::fs::File::open(cfg.path.clone()) {
        Ok(file) => file,
        Err(_) => return,
    };

    let reader = std::io::BufReader::new(file);
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
            debug!("dump handler - dumping state to {}", cfg.path);
            dump(state.clone(), cfg.clone());
        }
    }
}
