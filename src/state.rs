use bgpkit_parser::models::NetworkPrefix;
use core::net::IpAddr;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::sync::{Arc, Mutex};

use crate::settings::StateConfig;
use crate::update::Update;

pub type AsyncState = Arc<Mutex<State>>;

pub fn new_state(_sc: &StateConfig) -> AsyncState {
    Arc::new(Mutex::new(State::new(_sc)))
}

pub fn dump(state: AsyncState, cfg: StateConfig) {
    let state = state.lock().unwrap();
    let file = std::fs::File::create(cfg.path).unwrap();
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer(&mut writer, &state.store).unwrap();
}

pub fn load(state: AsyncState, cfg: StateConfig) {
    let mut state = state.lock().unwrap();

    let file = match std::fs::File::open(cfg.path) {
        Ok(file) => file,
        Err(_) => return,
    };

    let reader = std::io::BufReader::new(file);
    let store: MemoryStore = serde_json::from_reader(reader).unwrap();
    state.store = store;
}

pub struct State {
    store: MemoryStore,
}

impl State {
    pub fn new(_sc: &StateConfig) -> State {
        State {
            store: MemoryStore::new(),
        }
    }

    // Get all the updates from the state
    pub fn get_all(&self) -> Result<Vec<(IpAddr, IpAddr, NetworkPrefix)>, Box<dyn Error>> {
        Ok(self.store.get_all())
    }

    // Get the updates for a specific router and peer
    pub fn get_updates(
        &self,
        router_addr: &IpAddr,
        peer_addr: &IpAddr,
    ) -> Result<Vec<NetworkPrefix>, Box<dyn Error>> {
        Ok(self.store.get_updates(router_addr, peer_addr))
    }

    // Remove all updates for a specific router and peer
    pub fn remove_updates(
        &mut self,
        router_addr: &IpAddr,
        peer_addr: &IpAddr,
    ) -> Result<(), Box<dyn Error>> {
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
        let emit = self.store.update(router_addr, peer_addr, update);
        Ok(emit)
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
        let router = self
            .routers
            .entry(router_addr.clone())
            .or_insert(Router::new());
        router
    }

    fn get_all(&self) -> Vec<(IpAddr, IpAddr, NetworkPrefix)> {
        let mut res = Vec::new();
        for (router_addr, router) in &self.routers {
            for (peer_addr, updates) in &router.peers {
                for update in updates {
                    res.push((router_addr.clone(), peer_addr.clone(), update.clone()));
                }
            }
        }
        res
    }

    fn get_updates(&self, router_addr: &IpAddr, peer_addr: &IpAddr) -> Vec<NetworkPrefix> {
        let router = self.routers.get(router_addr).unwrap();
        let updates = router.peers.get(peer_addr).unwrap();
        updates.iter().cloned().collect()
    }

    fn remove_peer(&mut self, router_addr: &IpAddr, peer_addr: &IpAddr) {
        let router = self._get_router(router_addr);
        router.remove_peer(peer_addr);
    }

    fn update(&mut self, router_addr: &IpAddr, peer_addr: &IpAddr, update: &Update) -> bool {
        let router = self._get_router(router_addr);
        router.update(peer_addr, update)
    }
}

#[derive(Serialize, Deserialize)]
struct Router {
    peers: HashMap<IpAddr, HashSet<NetworkPrefix>>,
}

impl Router {
    fn new() -> Router {
        Router {
            peers: HashMap::new(),
        }
    }

    fn add_peer(&mut self, peer: &IpAddr) {
        if !self.peers.contains_key(peer) {
            self.peers.insert(peer.clone(), HashSet::new());
        }
    }

    fn remove_peer(&mut self, peer: &IpAddr) {
        self.peers.remove(peer);
    }

    fn update(&mut self, peer: &IpAddr, update: &Update) -> bool {
        self.add_peer(&peer);
        let updates = self.peers.get_mut(&peer).unwrap();

        // Will emit the update only if (1) announced + not present or (2) withdrawn + present
        // Which is a XOR operation
        let emit = update.announced ^ updates.contains(&update.prefix);

        if update.announced {
            // Announced prefix: add the update or overwrite it if present
            updates.insert(update.prefix);
        } else {
            // Withdrawn prefix: remove the update if present
            updates.remove(&update.prefix);
        }
        return emit;
    }
}
