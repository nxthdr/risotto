use axum::{extract::State as AxumState, routing::get, Json, Router};
use core::net::IpAddr;
use metrics::{counter, gauge, Label};
use metrics_exporter_prometheus::PrometheusHandle;
use serde::{Deserialize, Serialize};

use risotto_lib::{state::AsyncState, state_store::store::StateStore, statistics::AsyncStatistics};

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

struct AppState<T: StateStore> {
    state: Option<AsyncState<T>>,
    statistics: AsyncStatistics,
    prom_handle: PrometheusHandle,
}

impl<T: StateStore> Clone for AppState<T> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            statistics: self.statistics.clone(),
            prom_handle: self.prom_handle.clone(),
        }
    }
}

pub fn app<T: StateStore>(
    state: Option<AsyncState<T>>,
    statistics: AsyncStatistics,
    prom_handle: PrometheusHandle,
) -> Router {
    let app_state = AppState {
        state,
        statistics,
        prom_handle,
    };
    Router::new()
        .route("/", get(root).with_state(app_state.clone()))
        .route("/metrics", get(metrics).with_state(app_state.clone()))
}

async fn format<T: StateStore>(state: AsyncState<T>) -> Vec<APIRouter> {
    let mut api_routers: Vec<APIRouter> = Vec::new();
    let state = state.lock().await;
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

async fn root<T: StateStore>(
    AxumState(AppState { state, .. }): AxumState<AppState<T>>,
) -> Json<Vec<APIRouter>> {
    match state.as_ref() {
        Some(state) => {
            let api_routers = format(state.clone()).await;
            Json(api_routers)
        }
        None => Json(Vec::new()),
    }
}

async fn metrics<T: StateStore>(
    AxumState(AppState {
        state,
        statistics,
        prom_handle,
    }): AxumState<AppState<T>>,
) -> String {
    // Set the state metrics if enabled
    if !state.is_none() {
        let state = state.unwrap();
        let api_routers = format(state).await;

        for api_router in &api_routers {
            let labels = vec![Label::new("router", api_router.router_addr.to_string())];
            gauge!("risotto_state_bgp_peers", labels).set(api_router.peers.len() as f64);
        }

        for api_router in &api_routers {
            for api_peer in &api_router.peers {
                let total = api_peer.ipv4 + api_peer.ipv6;
                let labels = vec![
                    Label::new("router", api_router.router_addr.to_string()),
                    Label::new("peer", api_peer.peer_addr.to_string()),
                ];
                gauge!("risotto_state_bgp_updates", labels).set(total as f64);
            }
        }
    }

    // Set the statistics metrics
    let statistics = statistics.lock().await;
    let metric_name = "risotto_bmp_messages_total";
    counter!(metric_name, vec![Label::new("type", "initiation")])
        .absolute(statistics.rx_bmp_initiation as u64);
    counter!(metric_name, vec![Label::new("type", "peer_up")])
        .absolute(statistics.rx_bmp_peer_up as u64);
    counter!(metric_name, vec![Label::new("type", "route_monitoring")])
        .absolute(statistics.rx_bmp_route_monitoring as u64);
    counter!(metric_name, vec![Label::new("type", "route_mirroring")])
        .absolute(statistics.rx_bmp_route_mirroring as u64);
    counter!(metric_name, vec![Label::new("type", "peer_down")])
        .absolute(statistics.rx_bmp_peer_down as u64);
    counter!(metric_name, vec![Label::new("type", "termination")])
        .absolute(statistics.rx_bmp_termination as u64);
    counter!(metric_name, vec![Label::new("type", "stats_report")])
        .absolute(statistics.rx_bmp_stats_report as u64);

    prom_handle.render()
}
