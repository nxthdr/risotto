use axum::{extract::State, routing::get, Json, Router};
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
    ipv4: APIUpdate,
    ipv6: APIUpdate,
}

#[derive(Debug, Serialize, Deserialize)]
struct APIUpdate {
    announced_updates: usize,
    withdrawn_updates: usize,
}

pub fn app(db: DB) -> Router {
    Router::new().route("/", get(root).with_state(db))
}

fn format(db: DB) -> Vec<APIRouter> {
    let routers = db.routers.lock().unwrap();
    routers
        .values()
        .map(|router| {
            let api_peers = router.peers.iter().map(|(peer, updates)| {
                let (ipv4, ipv6) = updates.iter().fold(
                    (
                        APIUpdate {
                            announced_updates: 0,
                            withdrawn_updates: 0,
                        },
                        APIUpdate {
                            announced_updates: 0,
                            withdrawn_updates: 0,
                        },
                    ),
                    |(mut ipv4, mut ipv6), (_, update)| {
                        let counter = if update.prefix.prefix.addr().is_ipv4() {
                            &mut ipv4
                        } else {
                            &mut ipv6
                        };

                        if update.announced {
                            counter.announced_updates += 1;
                        } else {
                            counter.withdrawn_updates += 1;
                        }

                        (ipv4, ipv6)
                    },
                );

                APIPeers {
                    peer_addr: peer.peer_address,
                    peer_bgp_id: peer.peer_bgp_id,
                    peer_asn: peer.peer_asn.to_u32(),
                    ipv4,
                    ipv6,
                }
            });

            APIRouter {
                router_addr: router.addr,
                router_port: router.port,
                peers: api_peers.collect(),
            }
        })
        .collect()
}

async fn root(State(db): State<DB>) -> Json<Vec<APIRouter>> {
    let api_routers = format(db);
    Json(api_routers)
}
