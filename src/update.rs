use chrono::{DateTime, Utc};

use crate::router::Router;
use bgpkit_parser::bmp::messages::RouteMonitoring;
use bgpkit_parser::models::*;
use core::net::IpAddr;

#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub prefix: NetworkPrefix,
    pub announced: bool,
    pub origin: Origin,
    pub path: Option<AsPath>,
    pub communities: Vec<MetaCommunity>,
    pub timestamp: DateTime<Utc>,
    pub synthetic: bool,
}

pub fn decode_updates(message: RouteMonitoring) -> Option<Vec<Update>> {
    let mut updates = Vec::new();

    match message.bgp_message {
        bgpkit_parser::models::BgpMessage::Update(bgp_update) => {
            let mut prefixes_to_update = Vec::new();
            for prefix in bgp_update.announced_prefixes {
                prefixes_to_update.push((prefix, true));
            }
            for prefix in bgp_update.withdrawn_prefixes {
                prefixes_to_update.push((prefix, false));
            }

            let attributes = bgp_update.attributes;
            let origin = attributes.origin();
            let path = match attributes.as_path() {
                Some(path) => Some(path.clone()),
                None => None,
            };
            let communities: Vec<MetaCommunity> = attributes.iter_communities().collect();
            for (prefix, announced) in prefixes_to_update {
                updates.push(Update {
                    prefix: prefix,
                    announced: announced,
                    origin: origin,
                    path: path.clone(),
                    communities: communities.clone(),
                    timestamp: Utc::now(),
                    synthetic: false,
                });
            }

            return Some(updates);
        }
        _ => None,
    }
}

pub fn construct_as_path(path: Option<AsPath>) -> Vec<u32> {
    match path {
        Some(mut path) => {
            let mut contructed_path: Vec<u32> = Vec::new();
            path.coalesce();
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

pub fn construct_communities(communities: Vec<MetaCommunity>) -> Vec<(u32, u16)> {
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

fn map_to_ipv6(ip: IpAddr) -> IpAddr {
    if ip.is_ipv4() {
        format!("::ffff:{}", ip.to_string()).parse().unwrap()
    } else {
        ip
    }
}

// Returns a CSV line corresponding to this schema
// router_addr,router_port,peer_addr,peer_bgp_id,peer_asn,prefix_addr,prefix_len,origin,announced,synthetic,path,communities,timestamp
pub fn format_update(router: &Router, peer: &Peer, update: &Update) -> String {
    let as_path_str = construct_as_path(update.path.clone())
        .iter()
        .map(|x| x.to_string())
        .collect::<Vec<String>>()
        .join(",");
    let as_path_str = format!("\"[{}]\"", as_path_str);

    let communities_str = construct_communities(update.communities.clone())
        .iter()
        .map(|x| format!("({},{})", x.0, x.1))
        .collect::<Vec<String>>()
        .join(",");
    let communities_str = format!("\"[{}]\"", communities_str);

    let row: Vec<String> = vec![
        map_to_ipv6(router.addr).to_string(),
        router.port.to_string(),
        map_to_ipv6(peer.peer_address).to_string(),
        peer.peer_bgp_id.to_string(),
        peer.peer_asn.to_string(),
        map_to_ipv6(update.prefix.prefix.addr()).to_string(),
        update.prefix.prefix.prefix_len().to_string(),
        update.origin.to_string(),
        update.announced.to_string(),
        update.synthetic.to_string(),
        as_path_str,
        communities_str,
        update.timestamp.timestamp().to_string(),
    ];

    return row.join(",");
}
