use anyhow::Result;
use chrono::Utc;
use core::net::IpAddr;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, trace};

use crate::state_store::store::StateStore;
use crate::update::{Update, UpdateMetadata};

pub type AsyncState<T> = Arc<Mutex<State<T>>>;
pub type RouterPeerUpdate = (IpAddr, IpAddr, TimedPrefix);

pub fn new_state<T: StateStore + Send>(store: T) -> AsyncState<T> {
    Arc::new(Mutex::new(State::new(store)))
}

pub struct State<T: StateStore> {
    pub store: T,
}

impl<T: StateStore> State<T> {
    pub fn new(store: T) -> State<T> {
        State { store }
    }

    // Get all the updates from the state
    pub fn get_all(&self) -> Result<Vec<RouterPeerUpdate>> {
        Ok(self.store.get_all())
    }

    // Get the updates for a specific router and peer
    pub fn get_updates_by_peer(
        &self,
        router_addr: &IpAddr,
        peer_addr: &IpAddr,
    ) -> Result<Vec<TimedPrefix>> {
        Ok(self.store.get_updates_by_peer(router_addr, peer_addr))
    }

    // Remove all updates for a specific router and peer
    pub fn remove_updates(&mut self, router_addr: &IpAddr, peer_addr: &IpAddr) -> Result<()> {
        self.store.remove_peer(router_addr, peer_addr);
        Ok(())
    }

    // Update the state with a new update
    pub fn update(
        &mut self,
        router_addr: &IpAddr,
        peer_addr: &IpAddr,
        update: &Update,
    ) -> Result<bool, Box<dyn Error>> {
        Ok(self.store.update(router_addr, &peer_addr, update))
    }
}

#[derive(Serialize, Deserialize, Eq, Clone)]
pub struct TimedPrefix {
    pub prefix_addr: IpAddr,
    pub prefix_len: u8,
    pub is_post_policy: bool,
    pub is_adj_rib_out: bool,
    pub timestamp: i64,
}

impl PartialEq for TimedPrefix {
    fn eq(&self, other: &Self) -> bool {
        self.prefix_addr == other.prefix_addr
            && self.prefix_len == other.prefix_len
            && self.is_post_policy == other.is_post_policy
            && self.is_adj_rib_out == other.is_adj_rib_out
    }
}

impl Hash for TimedPrefix {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.prefix_addr.hash(state);
        self.prefix_len.hash(state);
        self.is_post_policy.hash(state);
        self.is_adj_rib_out.hash(state);
    }
}

pub fn synthesize_withdraw_update(prefix: TimedPrefix, metadata: UpdateMetadata) -> Update {
    Update {
        time_received_ns: Utc::now(),
        router_addr: metadata.router_addr,
        router_port: metadata.router_port,
        peer_addr: metadata.peer_addr,
        peer_bgp_id: metadata.peer_bgp_id,
        peer_asn: metadata.peer_asn,
        prefix_addr: prefix.prefix_addr,
        prefix_len: prefix.prefix_len,
        announced: false,
        origin: "INCOMPLETE".to_string(),
        path: vec![],
        communities: vec![],
        is_post_policy: prefix.is_post_policy,
        is_adj_rib_out: prefix.is_adj_rib_out,
        synthetic: true,
    }
}

pub async fn peer_up_withdraws_handler<T: StateStore>(
    state: AsyncState<T>,
    tx: Sender<Update>,
    metadata: UpdateMetadata,
    sleep_time: u64,
) {
    let startup = chrono::Utc::now();
    sleep(Duration::from_secs(sleep_time)).await;

    info!(
        "[{}]:{} - {} - removing updates older than {} after waited {} seconds",
        metadata.router_addr, metadata.router_port, metadata.peer_addr, startup, sleep_time
    );

    let state_lock = state.lock().unwrap();
    let timed_prefixes = state_lock
        .store
        .get_updates_by_peer(&metadata.router_addr, &metadata.peer_addr);

    drop(state_lock);

    let mut synthetic_updates = Vec::new();
    for timed_prefix in timed_prefixes {
        if timed_prefix.timestamp < startup.timestamp_millis() {
            // This update has not been re-announced after startup
            // Emit a synthetic withdraw update
            synthetic_updates.push(synthesize_withdraw_update(timed_prefix, metadata.clone()));
        }
    }

    info!(
        "[{}]:{} - {} - emitting {} synthetic withdraw updates",
        metadata.router_addr,
        metadata.router_port,
        metadata.peer_addr,
        synthetic_updates.len()
    );

    let mut state_lock = state.lock().unwrap();
    for update in &mut synthetic_updates {
        trace!("{:?}", update);

        // Sent to the event pipeline
        tx.send(update.clone()).unwrap();

        // Remove the update from the state
        state_lock
            .store
            .update(&update.router_addr, &metadata.peer_addr, update);
    }
}
