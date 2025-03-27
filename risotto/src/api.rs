use axum::{extract::State as AxumState, routing::get, Json, Router};
use core::net::IpAddr;
use metrics::{Key, Label, Recorder};
use metrics_exporter_prometheus::PrometheusBuilder;
use serde::{Deserialize, Serialize};

use risotto_lib::state::AsyncState;

static METADATA: metrics::Metadata =
    metrics::Metadata::new(module_path!(), metrics::Level::INFO, Some(module_path!()));

#[derive(Debug, Serialize, Deserialize)]
struct APIRouter {
    router_addr: IpAddr,
    peers: Vec<APIPeer>,
}

#[derive(Debug, Serialize, Deserialize)]
struct APIPeer {
    peer_addr: IpAddr,
    ipv4: usize,
    ipv6: usize,
}

#[derive(Clone)]
struct AppState {
    state: Option<AsyncState>,
}

pub fn app(state: Option<AsyncState>) -> Router {
    let app_state = AppState {
        state: state.clone(),
    };

    Router::new()
        .route("/", get(root).with_state(app_state.clone()))
        .route("/metrics", get(metrics).with_state(app_state.clone()))
}

async fn format(state: AsyncState) -> Vec<APIRouter> {
    let mut api_routers: Vec<APIRouter> = Vec::new();
    let state = state.lock().unwrap();

    for (router_addr, peer_addr, update_prefix) in state.get_all().unwrap() {
        // Find the router in the list of routers
        let mut router = None;
        for r in &mut api_routers {
            if r.router_addr == router_addr {
                router = Some(r);
                break;
            }
        }

        // If the router is not found, create a new one
        let router = match router {
            Some(r) => r,
            None => {
                let r = APIRouter {
                    router_addr,
                    peers: Vec::new(),
                };
                api_routers.push(r);
                api_routers.last_mut().unwrap()
            }
        };

        // Find the peer in the list of peers
        let mut peer = None;
        for p in &mut router.peers {
            if p.peer_addr == peer_addr {
                peer = Some(p);
                break;
            }
        }

        // If the peer is not found, create a new one
        let peer = match peer {
            Some(p) => p,
            None => {
                let p = APIPeer {
                    peer_addr,
                    ipv4: 0,
                    ipv6: 0,
                };
                router.peers.push(p);
                router.peers.last_mut().unwrap()
            }
        };

        if update_prefix.prefix_addr.is_ipv4() {
            peer.ipv4 += 1;
        } else {
            peer.ipv6 += 1;
        };
    }
    api_routers
}

async fn root(AxumState(AppState { state, .. }): AxumState<AppState>) -> Json<Vec<APIRouter>> {
    match state {
        Some(state) => {
            let api_routers = format(state).await;
            Json(api_routers)
        }
        None => {
            // TODO: return an HTTP error instead
            return Json(Vec::new());
        }
    }
}

async fn metrics(AxumState(AppState { state }): AxumState<AppState>) -> String {
    if state.is_none() {
        return String::new();
    }

    let state = state.unwrap();

    let recorder = PrometheusBuilder::new().build_recorder();
    let api_routers = format(state).await;

    recorder.describe_gauge(
        "risotto_bgp_peers".into(),
        None,
        "Number of BGP peers per router".into(),
    );
    for api_router in &api_routers {
        let labels = vec![Label::new("router", api_router.router_addr.to_string())];
        let key = Key::from_parts("risotto_bgp_peers", labels);
        recorder
            .register_gauge(&key, &METADATA)
            .set(api_router.peers.len() as f64);
    }

    recorder.describe_gauge(
        "risotto_bgp_updates".into(),
        None,
        "Number of BGP updates per (router, peer)".into(),
    );
    for api_router in &api_routers {
        for api_peer in &api_router.peers {
            let total = api_peer.ipv4 + api_peer.ipv6;
            let labels = vec![
                Label::new("router", api_router.router_addr.to_string()),
                Label::new("peer", api_peer.peer_addr.to_string()),
            ];
            let key = Key::from_parts("risotto_bgp_updates", labels);
            recorder.register_gauge(&key, &METADATA).set(total as f64);
        }
    }

    recorder.handle().render()
}
