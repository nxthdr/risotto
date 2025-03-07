use bgpkit_parser::models::{NetworkPrefix, Origin, Peer as BGPkitPeer};
use chrono::Utc;
use core::net::IpAddr;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::StateConfig;
use crate::update::{format_update, Update};

pub type AsyncState = Arc<Mutex<State>>;

type RouterPeerUpdate = (IpAddr, IpAddr, TimedPrefix);

pub fn new_state(state_config: &StateConfig) -> AsyncState {
    Arc::new(Mutex::new(State::new(state_config)))
}
pub fn dump(state: AsyncState) {
    let state = state.lock().unwrap();
    let temp_path = format!("{}.tmp", state.config.path);
    let file = std::fs::File::create(&temp_path).unwrap();
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer(&mut writer, &state.store).unwrap();
    std::fs::rename(temp_path, state.config.path.clone()).unwrap();
}

pub fn load(state: AsyncState) {
    let mut state = state.lock().unwrap();

    let file = match std::fs::File::open(state.config.path.clone()) {
        Ok(file) => file,
        Err(_) => return,
    };

    let reader = std::io::BufReader::new(file);
    let store: MemoryStore = serde_json::from_reader(reader).unwrap();
    state.store = store;
}

pub struct State {
    store: MemoryStore,
    config: StateConfig,
}

impl State {
    pub fn new(state_config: &StateConfig) -> State {
        State {
            store: MemoryStore::new(),
            config: state_config.clone(),
        }
    }

    // Get all the updates from the state
    pub fn get_all(&self) -> Result<Vec<RouterPeerUpdate>, Box<dyn Error>> {
        Ok(self.store.get_all())
    }

    // Get the updates for a specific router and peer
    pub fn get_updates_by_peer(
        &self,
        router_addr: &IpAddr,
        peer: &BGPkitPeer,
    ) -> Result<Vec<TimedPrefix>, Box<dyn Error>> {
        Ok(self.store.get_updates_by_peer(router_addr, peer))
    }

    // Remove all updates for a specific router and peer
    pub fn remove_updates(
        &mut self,
        router_addr: &IpAddr,
        peer: &BGPkitPeer,
    ) -> Result<(), Box<dyn Error>> {
        if !self.config.enable {
            return Ok(());
        }

        self.store.remove_peer(router_addr, peer);
        Ok(())
    }

    // Update the state with a new update
    pub fn update(
        &mut self,
        router_addr: &IpAddr,
        peer: &BGPkitPeer,
        update: &Update,
    ) -> Result<bool, Box<dyn Error>> {
        if !self.config.enable {
            // If the state is disabled, all updates are emited
            return Ok(true);
        }
        let emit = self.store.update(router_addr, peer, update);
        Ok(emit)
    }
}

#[derive(Serialize, Deserialize, Eq, Clone)]
pub struct TimedPrefix {
    pub prefix: NetworkPrefix,
    pub is_post_policy: bool,
    pub is_adj_rib_out: bool,
    pub timestamp: i64,
}

impl PartialEq for TimedPrefix {
    fn eq(&self, other: &Self) -> bool {
        self.prefix == other.prefix
            && self.is_post_policy == other.is_post_policy
            && self.is_adj_rib_out == other.is_adj_rib_out
    }
}

impl Hash for TimedPrefix {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.prefix.hash(state);
        self.is_post_policy.hash(state);
        self.is_adj_rib_out.hash(state);
    }
}

#[derive(Serialize, Deserialize)]
struct MemoryStore {
    routers: HashMap<IpAddr, Router>,
}

impl MemoryStore {
    fn new() -> MemoryStore {
        MemoryStore {
            routers: HashMap::new(),
        }
    }

    fn _get_router(&mut self, router_addr: &IpAddr) -> &mut Router {
        let router = self.routers.entry(*router_addr).or_insert(Router::new());
        router
    }

    fn get_all(&self) -> Vec<RouterPeerUpdate> {
        let mut res = Vec::new();
        for (router_addr, router) in &self.routers {
            for (peer_addr, peer) in &router.peers {
                for update in &peer.updates {
                    res.push((*router_addr, *peer_addr, update.clone()));
                }
            }
        }
        res
    }

    fn get_peer(&self, router_addr: &IpAddr, peer_addr: &IpAddr) -> Option<Peer> {
        let router_binding = Router::new();
        let router = self.routers.get(router_addr).unwrap_or(&router_binding);
        router.peers.get(peer_addr).cloned()
    }

    fn get_updates_by_peer(&self, router_addr: &IpAddr, peer: &BGPkitPeer) -> Vec<TimedPrefix> {
        let router_binding = Router::new();
        let router = self.routers.get(router_addr).unwrap_or(&router_binding);
        let peer_binding = Peer {
            details: *peer,
            updates: HashSet::new(),
        };
        let updates = router
            .peers
            .get(&peer.peer_address)
            .unwrap_or(&peer_binding);

        updates.updates.iter().cloned().collect()
    }

    fn remove_peer(&mut self, router_addr: &IpAddr, peer: &BGPkitPeer) {
        let router = self._get_router(router_addr);
        router.remove_peer(peer);
    }

    fn update(&mut self, router_addr: &IpAddr, peer: &BGPkitPeer, update: &Update) -> bool {
        let router = self._get_router(router_addr);
        router.update(peer, update)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Peer {
    details: BGPkitPeer,
    updates: HashSet<TimedPrefix>,
}

#[derive(Serialize, Deserialize)]
struct Router {
    peers: HashMap<IpAddr, Peer>,
}

impl Router {
    fn new() -> Router {
        Router {
            peers: HashMap::new(),
        }
    }

    fn add_peer(&mut self, peer: &BGPkitPeer) {
        self.peers.entry(peer.peer_address).or_insert_with(|| Peer {
            details: *peer,
            updates: HashSet::new(),
        });
    }

    fn remove_peer(&mut self, peer: &BGPkitPeer) {
        self.peers.remove(&peer.peer_address);
    }

    fn update(&mut self, peer: &BGPkitPeer, update: &Update) -> bool {
        self.add_peer(peer);
        let peer = self.peers.get_mut(&peer.peer_address).unwrap();

        let now: i64 = chrono::Utc::now().timestamp_millis();
        let timed_prefix = TimedPrefix {
            prefix: update.prefix,
            is_post_policy: update.is_post_policy,
            is_adj_rib_out: update.is_adj_rib_out,
            timestamp: now,
        };

        // Will emit the update only if (1) announced + not present or (2) withdrawn + present
        // Which is a XOR operation
        let emit = update.announced ^ peer.updates.contains(&timed_prefix);

        if update.announced {
            // Announced prefix: add the update or overwrite it if present
            peer.updates.replace(timed_prefix);
        } else {
            // Withdrawn prefix: remove the update if present
            peer.updates.remove(&timed_prefix);
        }
        emit
    }
}

pub fn synthesize_withdraw_update(prefix: TimedPrefix) -> Update {
    Update {
        prefix: prefix.prefix,
        announced: false,
        origin: Origin::INCOMPLETE,
        path: None,
        communities: vec![],
        is_post_policy: prefix.is_post_policy,
        is_adj_rib_out: prefix.is_adj_rib_out,
        timestamp: Utc::now(),
        synthetic: true,
    }
}

pub async fn peer_up_withdraws_handler(
    state: AsyncState,
    router_addr: IpAddr,
    bgp_peer: BGPkitPeer,
    tx: Sender<String>,
) {
    let startup = chrono::Utc::now();
    let random = {
        let mut rng = rand::rng();
        rng.random_range(-60.0..60.0) as i64
    };
    let sleep_time = 300 + random; // 5 minutes +/- 1 minute

    tokio::time::sleep(Duration::from_secs(sleep_time as u64)).await;

    log::info!(
        "state - startup withdraws handler - {} - {} removing updates older than {}",
        router_addr,
        bgp_peer.peer_address,
        startup
    );

    let state_lock: std::sync::MutexGuard<'_, State> = state.lock().unwrap();
    let peer = match state_lock
        .store
        .get_peer(&router_addr, &bgp_peer.peer_address)
    {
        Some(peer) => peer,
        None => return,
    };

    drop(state_lock);

    let mut synthetic_updates = Vec::new();
    for update in peer.updates {
        if update.timestamp < startup.timestamp_millis() {
            // This update has been re-announced after startup
            // Emit a synthetic withdraw update
            synthetic_updates.push((
                router_addr,
                peer.details,
                synthesize_withdraw_update(update.clone()),
            ));
        }
    }

    log::info!(
        "state - startup withdraws handler - {} - {} emitting {} synthetic withdraw updates",
        router_addr,
        bgp_peer.peer_address,
        synthetic_updates.len()
    );

    let mut state_lock: std::sync::MutexGuard<'_, State> = state.lock().unwrap();
    for (router_addr, peer, update) in &mut synthetic_updates {
        let update_str = format_update(*router_addr, 0, peer, update);
        log::trace!("{:?}", update_str);

        // Sent to the event pipeline
        tx.send(update_str).unwrap();

        // Remove the update from the state
        state_lock.store.update(router_addr, peer, update);
    }
}

pub async fn dump_handler(state: AsyncState, cfg: StateConfig) {
    loop {
        // TODO do not spawn this task if state is disabled
        tokio::time::sleep(Duration::from_secs(cfg.interval)).await;
        if cfg.enable {
            log::debug!("state - dump handler - dumping state to {}", cfg.path);
            dump(state.clone());
        }
    }
}
