use std::net::IpAddr;

use crate::state::{RouterPeerUpdate, TimedPrefix};
use crate::update::Update;

pub trait StateStore: Send + Sync + 'static {
    fn get_all(&self) -> Vec<RouterPeerUpdate>;
    fn get_updates_by_peer(&self, router_addr: &IpAddr, peer_addr: &IpAddr) -> Vec<TimedPrefix>;
    fn remove_peer(&mut self, router_addr: &IpAddr, peer_addr: &IpAddr);
    fn update(&mut self, router_addr: &IpAddr, peer_addr: &IpAddr, update: &Update) -> bool;
}
