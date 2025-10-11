use anyhow::Result;
use chrono::Utc;
use core::net::IpAddr;
use metrics::{counter, gauge};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{debug, trace};

use crate::state_store::store::StateStore;
use crate::update::{map_to_ipv6, Update, UpdateMetadata};

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

    pub fn get_updates_by_peer(
        &self,
        router_addr: &IpAddr,
        peer_addr: &IpAddr,
    ) -> Result<Vec<TimedPrefix>> {
        Ok(self.store.get_updates_by_peer(router_addr, peer_addr))
    }

    pub fn add_peer(&mut self, router_addr: &IpAddr, peer_addr: &IpAddr) -> Result<()> {
        self.store.add_peer(router_addr, peer_addr);
        Ok(())
    }

    pub fn remove_peer(&mut self, router_addr: &IpAddr, peer_addr: &IpAddr) -> Result<()> {
        self.store.remove_peer(router_addr, peer_addr);
        gauge!(
            "risotto_state_updates",
            "router" => router_addr.to_string(),
            "peer" => peer_addr.to_string()
        )
        .set(0);
        Ok(())
    }

    pub fn update(
        &mut self,
        router_addr: &IpAddr,
        peer_addr: &IpAddr,
        update: &Update,
    ) -> Result<bool> {
        let emit = self.store.update(router_addr, &peer_addr, update);
        if emit {
            let delta = if update.announced { 1.0 } else { -1.0 };
            gauge!(
                "risotto_state_updates",
                "router" => router_addr.to_string(),
                "peer" => peer_addr.to_string()
            )
            .increment(delta);
        }
        Ok(emit)
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
        time_bmp_header_ns: Utc::now(),
        router_addr: map_to_ipv6(metadata.router_socket.ip()),
        router_port: metadata.router_socket.port(),
        peer_addr: map_to_ipv6(metadata.peer_addr),
        peer_bgp_id: metadata.peer_bgp_id,
        peer_asn: metadata.peer_asn,
        prefix_addr: map_to_ipv6(prefix.prefix_addr),
        prefix_len: prefix.prefix_len,
        is_post_policy: prefix.is_post_policy,
        is_adj_rib_out: prefix.is_adj_rib_out,
        announced: false,
        synthetic: true,
        
        // BGP Attributes - all empty/default for synthetic withdraws
        origin: "INCOMPLETE".to_string(),
        as_path: vec![],
        next_hop: None,
        multi_exit_discriminator: None,
        local_preference: None,
        only_to_customer: None,
        atomic_aggregate: false,
        aggregator_asn: None,
        aggregator_bgp_id: None,
        communities: vec![],
        extended_communities: vec![],
        large_communities: vec![],
        originator_id: None,
        cluster_list: vec![],
        mp_reach_afi: None,
        mp_reach_safi: None,
        mp_unreach_afi: None,
        mp_unreach_safi: None,
    }
}

pub async fn peer_up_withdraws_handler<T: StateStore>(
    state: AsyncState<T>,
    tx: Sender<Update>,
    metadata: UpdateMetadata,
    sleep_time: u64,
) -> Result<()> {
    let startup = chrono::Utc::now();
    sleep(Duration::from_secs(sleep_time)).await;

    debug!(
        "[{} - {} - removing updates older than {} after waited {} seconds",
        metadata.router_socket, metadata.peer_addr, startup, sleep_time
    );

    let state_lock = state.lock().await;
    let timed_prefixes = state_lock
        .store
        .get_updates_by_peer(&metadata.router_socket.ip(), &metadata.peer_addr);

    drop(state_lock);

    let mut synthetic_updates = Vec::new();
    for timed_prefix in timed_prefixes {
        if timed_prefix.timestamp < startup.timestamp_millis() {
            // This update has not been re-announced after startup
            // Emit a synthetic withdraw update
            synthetic_updates.push(synthesize_withdraw_update(timed_prefix, metadata.clone()));
        }
    }

    debug!(
        "[{} - {} - emitting {} synthetic withdraw updates",
        metadata.router_socket,
        metadata.peer_addr,
        synthetic_updates.len()
    );

    counter!(
        "risotto_tx_updates_total",
        "router" => metadata.router_socket.ip().to_string(),
        "peer" => metadata.peer_addr.to_string(),
    )
    .increment(synthetic_updates.len() as u64);

    let mut state_lock = state.lock().await;
    for update in &mut synthetic_updates {
        trace!("{:?}", update);

        // Sent to the event pipeline
        tx.send(update.clone()).await?;

        // Remove the update from the state
        state_lock
            .store
            .update(&update.router_addr, &metadata.peer_addr, update);
    }

    Ok(())
}

/// Process pre-parsed updates through the state machine
/// This is the core processing logic used by both collectors and curators
pub async fn process_updates<T: StateStore>(
    state: Option<AsyncState<T>>,
    tx: Sender<Update>,
    updates: Vec<Update>,
) -> Result<()> {
    if updates.is_empty() {
        return Ok(());
    }

    match state {
        Some(state) => {
            // Stateful mode: deduplicate updates
            let mut state_lock = state.lock().await;

            for update in updates {
                debug!("Processing update: {:?}", update);
                let should_emit =
                    state_lock.update(&update.router_addr, &update.peer_addr, &update)?;

                if should_emit {
                    trace!("Emitting update: {:?}", update);
                    tx.send(update.clone()).await?;

                    counter!(
                        "risotto_tx_updates_total",
                        "router" => update.router_addr.to_string(),
                        "peer" => update.peer_addr.to_string(),
                    )
                    .increment(1);
                }
            }
        }
        None => {
            // Stateless mode: forward all updates as-is
            for update in updates {
                trace!("Forwarding update: {:?}", update);

                counter!(
                    "risotto_tx_updates_total",
                    "router" => update.router_addr.to_string(),
                    "peer" => update.peer_addr.to_string(),
                )
                .increment(1);

                tx.send(update).await?;
            }
        }
    }

    Ok(())
}

/// Process a single pre-parsed update through the state machine
/// Convenience wrapper for single update processing
pub async fn process_update<T: StateStore>(
    state: Option<AsyncState<T>>,
    tx: Sender<Update>,
    update: Update,
) -> Result<()> {
    process_updates(state, tx, vec![update]).await
}
