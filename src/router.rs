use bgpkit_parser::models::NetworkPrefix;
use bgpkit_parser::models::Peer;
use core::net::IpAddr;
use std::collections::{HashMap, HashSet};

use crate::update::Update;

#[derive(Debug)]
pub struct Router {
    pub addr: IpAddr,
    pub port: u16,
    pub peers: HashMap<Peer, HashSet<NetworkPrefix>>,
}

impl Router {
    pub fn add_peer(&mut self, peer: &Peer) {
        if !self.peers.contains_key(peer) {
            self.peers.insert(peer.clone(), HashSet::new());
        }
    }

    pub fn remove_peer(&mut self, peer: &Peer) {
        self.peers.remove(peer);
    }

    pub fn update(&mut self, peer: &Peer, update: &Update) -> bool {
        self.add_peer(&peer);
        let updates = self.peers.get_mut(&peer).unwrap();

        // Will emit the update only if (1) announced + not present or (2) withdrawn + present
        // Which is a XOR operation
        let emit = update.announced ^ updates.contains(&update.prefix);

        if update.announced {
            // Announced prefix: add the update if not present
            // https://datatracker.ietf.org/doc/html/rfc4271#section-9.1.4
            updates.insert(update.prefix);
        } else {
            // Withdrawn prefix: remove the update if present
            updates.remove(&update.prefix);
        }
        return emit;
    }
}

pub fn new_router(addr: IpAddr, port: u16) -> Router {
    return Router {
        addr: addr,
        port: port,
        peers: HashMap::new(),
    };
}
