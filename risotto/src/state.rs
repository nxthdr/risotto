use std::time::Duration;
use tokio::time::sleep;
use tracing::debug;

use risotto_lib::state::AsyncState;
use risotto_lib::state::MemoryStore;

use crate::config::StateConfig;

pub fn dump(state: AsyncState, cfg: StateConfig) {
    let state_lock = state.lock().unwrap();
    let temp_path = format!("{}.tmp", cfg.path);
    let file = std::fs::File::create(&temp_path).unwrap();
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer(&mut writer, &state_lock.store).unwrap();
    std::fs::rename(temp_path, cfg.path.clone()).unwrap();
}

pub fn load(state: AsyncState, cfg: StateConfig) {
    let mut state_lock = state.lock().unwrap();

    let file = match std::fs::File::open(cfg.path.clone()) {
        Ok(file) => file,
        Err(_) => return,
    };

    let reader = std::io::BufReader::new(file);
    let store: MemoryStore = serde_json::from_reader(reader).unwrap();
    state_lock.store = store;
}

pub async fn dump_handler(state: Option<AsyncState>, cfg: StateConfig) {
    loop {
        // TODO do not spawn this task if state is disabled
        sleep(Duration::from_secs(cfg.interval)).await;
        if let Some(ref state) = state {
            debug!("dump handler - dumping state to {}", cfg.path);
            dump(state.clone(), cfg.clone());
        }
    }
}
