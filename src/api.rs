use axum::{extract::State, routing::get, Json, Router};
use bgpkit_parser::models::{AsPath, AsPathSegment, MetaCommunity};
use core::net::{IpAddr, Ipv4Addr};
use serde::{Deserialize, Serialize};

use crate::db::DB;

#[derive(Debug, Serialize, Deserialize)]
struct APIRouter {
    router_addr: IpAddr,
    router_port: u16,
    peers: Vec<APIPeers>,
}

#[derive(Debug, Serialize, Deserialize)]
struct APIPeers {
    peer_addr: IpAddr,
    peer_bgp_id: Ipv4Addr,
    peer_asn: u32,
    updates: Vec<APIUpdate>,
}

#[derive(Debug, Serialize, Deserialize)]
struct APIUpdate {
    prefix: String,
    origin: String,
    path: Vec<u32>,
    communities: Vec<(u32, u16)>,
    timestamp: String,
}

pub fn app(db: DB) -> Router {
    Router::new().route("/", get(root).with_state(db))
}

fn construct_as_path(path: Option<AsPath>) -> Vec<u32> {
    match path {
        Some(mut path) => {
            let mut contructed_path: Vec<u32> = Vec::new();
            path.dedup_coalesce();
            for segment in path.into_segments_iter() {
                match segment {
                    AsPathSegment::AsSequence(dedup_asns) => {
                        for asn in dedup_asns {
                            contructed_path.push(asn.to_u32());
                        }
                    }
                    _ => (),
                }
            }
            contructed_path
        }
        None => Vec::new(),
    }
}

fn construct_communities(communities: Vec<MetaCommunity>) -> Vec<(u32, u16)> {
    let mut constructed_communities = Vec::new();
    for community in communities {
        match community {
            MetaCommunity::Plain(community) => match community {
                bgpkit_parser::models::Community::Custom(asn, value) => {
                    constructed_communities.push((asn.to_u32(), value));
                }
                _ => (), // TODO
            },
            _ => (), // TODO
        }
    }
    constructed_communities
}

fn format(db: DB) -> Vec<APIRouter> {
    let routers = db.routers.lock().unwrap();
    let mut api_routers = Vec::new();
    for router in routers.values() {
        let mut api_peers = Vec::new();
        for (peer, updates) in &router.peers {
            let mut api_updates = Vec::new();

            for (_, update) in updates {
                api_updates.push(APIUpdate {
                    prefix: update.prefix.to_string(),
                    origin: update.origin.to_string(),
                    path: construct_as_path(update.path.clone()),
                    communities: construct_communities(update.communities.clone()),
                    timestamp: update.timestamp.to_rfc3339(),
                });
            }

            api_peers.push(APIPeers {
                peer_addr: peer.peer_address,
                peer_bgp_id: peer.peer_bgp_id,
                peer_asn: peer.peer_asn.to_u32(),
                updates: api_updates,
            });
        }

        api_routers.push(APIRouter {
            router_addr: router.addr,
            router_port: router.port,
            peers: api_peers,
        });
    }
    api_routers
}

async fn root(State(db): State<DB>) -> Json<Vec<APIRouter>> {
    let api_routers = format(db);
    Json(api_routers)
}
