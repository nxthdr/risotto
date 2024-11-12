use crate::db::DB;
use axum::{extract::State, routing::get, Json, Router};
use core::net::{IpAddr, Ipv4Addr};
use metrics::{Key, Label, Recorder};
use metrics_exporter_prometheus::PrometheusBuilder;
use serde::{Deserialize, Serialize};

static METADATA: metrics::Metadata =
    metrics::Metadata::new(module_path!(), metrics::Level::INFO, Some(module_path!()));

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
    ipv4: usize,
    ipv6: usize,
}

#[derive(Clone)]
struct AppState {
    db: DB,
}

pub fn app(db: DB) -> Router {
    let app_state = AppState { db: db.clone() };

    Router::new()
        .route("/", get(root).with_state(app_state.clone()))
        .route("/metrics", get(metrics).with_state(app_state.clone()))
}

async fn format(db: DB) -> Vec<APIRouter> {
    let routers = db.lock().unwrap();
    routers
        .values()
        .map(|router| {
            let api_peers = router.peers.iter().map(|(peer, updates)| {
                let (ipv4, ipv6) = updates.iter().fold((0, 0), |(mut ipv4, mut ipv6), update| {
                    if update.prefix.addr().is_ipv4() {
                        ipv4 += 1;
                    } else {
                        ipv6 += 1;
                    };

                    (ipv4, ipv6)
                });

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

async fn root(State(AppState { db, .. }): State<AppState>) -> Json<Vec<APIRouter>> {
    let api_routers = format(db).await;
    Json(api_routers)
}

async fn metrics(State(AppState { db }): State<AppState>) -> String {
    let routers = db.lock().unwrap();
    let recorder = PrometheusBuilder::new().build_recorder();

    recorder.describe_gauge(
        "risotto_bgp_peers".into(),
        None,
        "Number of BGP peers per router".into(),
    );
    for router in routers.values() {
        let labels = vec![Label::new("router", router.addr.to_string())];
        let key = Key::from_parts("risotto_bgp_peers", labels);
        recorder
            .register_gauge(&key, &METADATA)
            .set(router.peers.len() as f64);
    }

    recorder.describe_gauge(
        "risotto_bgp_updates".into(),
        None,
        "Number of BGP updates per (router, peer)".into(),
    );
    for router in routers.values() {
        for (peer, updates) in router.peers.iter() {
            let labels = vec![
                Label::new("router", router.addr.to_string()),
                Label::new("peer", peer.peer_address.to_string()),
            ];
            let key = Key::from_parts("risotto_bgp_updates", labels);
            recorder
                .register_gauge(&key, &METADATA)
                .set(updates.len() as f64);
        }
    }

    recorder.handle().render()
}
