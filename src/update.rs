use chrono::{DateTime, MappedLocalTime, TimeZone, Utc};

use bgpkit_parser::bmp::messages::RouteMonitoring;
use bgpkit_parser::models::*;
use core::net::IpAddr;
use log::error;

pub struct UpdateHeader {
    pub timestamp: i64,
    pub is_post_policy: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub prefix: NetworkPrefix,
    pub announced: bool,
    pub origin: Origin,
    pub path: Option<AsPath>,
    pub communities: Vec<MetaCommunity>,
    pub is_post_policy: bool,
    pub timestamp: DateTime<Utc>,
    pub synthetic: bool,
}

pub fn decode_updates(message: RouteMonitoring, header: UpdateHeader) -> Option<Vec<Update>> {
    let mut updates = Vec::new();

    match message.bgp_message {
        bgpkit_parser::models::BgpMessage::Update(bgp_update) => {
            // https://datatracker.ietf.org/doc/html/rfc4271
            let mut prefixes_to_update = Vec::new();
            for prefix in bgp_update.announced_prefixes {
                prefixes_to_update.push((prefix, true));
            }
            for prefix in bgp_update.withdrawn_prefixes {
                prefixes_to_update.push((prefix, false));
            }

            // https://datatracker.ietf.org/doc/html/rfc4760
            let attributes = bgp_update.attributes;
            match attributes.get_reachable_nlri() {
                Some(nlri) => {
                    for prefix in &nlri.prefixes {
                        prefixes_to_update.push((*prefix, true));
                    }
                }
                None => (),
            }
            match attributes.get_unreachable_nlri() {
                Some(nlri) => {
                    for prefix in &nlri.prefixes {
                        prefixes_to_update.push((*prefix, false));
                    }
                }
                None => (),
            }

            // Get the other attributes
            let origin = attributes.origin();
            let path = match attributes.as_path() {
                Some(path) => Some(path.clone()),
                None => None,
            };
            let communities: Vec<MetaCommunity> = attributes.iter_communities().collect();

            let timestamp = match Utc.timestamp_millis_opt(header.timestamp) {
                MappedLocalTime::Single(dt) => dt,
                _ => {
                    error!(
                        "bmp - failed to parse timestamp: {}, using Utc::now()",
                        header.timestamp
                    );
                    Utc::now()
                }
            };

            for (prefix, announced) in prefixes_to_update {
                updates.push(Update {
                    prefix,
                    announced,
                    origin,
                    path: path.clone(),
                    communities: communities.clone(),
                    is_post_policy: header.is_post_policy,
                    timestamp,
                    synthetic: false,
                });
            }

            return Some(updates);
        }
        _ => None,
    }
}

pub fn create_withdraw_update(prefix: NetworkPrefix, timestamp: DateTime<Utc>) -> Update {
    Update {
        prefix: prefix,
        announced: false,
        origin: Origin::INCOMPLETE,
        path: None,
        communities: vec![],
        is_post_policy: false,
        timestamp,
        synthetic: true,
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
// timestamp,router_addr,router_port,peer_addr,peer_bgp_id,peer_asn,prefix_addr,prefix_len,announced,is_post_policy,origin,path,communities,synthetic
pub fn format_update(
    router_addr: IpAddr,
    router_port: u16,
    peer: &Peer,
    update: &Update,
) -> String {
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

    let mut row: Vec<String> = Vec::new();
    row.push(format!("{}", update.timestamp.timestamp_millis()));
    row.push(format!("{}", map_to_ipv6(router_addr)));
    row.push(format!("{}", router_port));
    row.push(format!("{}", map_to_ipv6(peer.peer_address)));
    row.push(format!("{}", peer.peer_bgp_id));
    row.push(format!("{}", peer.peer_asn));
    row.push(format!("{}", map_to_ipv6(update.prefix.prefix.addr())));
    row.push(format!("{}", update.prefix.prefix.prefix_len()));
    row.push(format!("{}", update.is_post_policy));
    row.push(format!("{}", update.announced));
    row.push(format!("{}", update.origin));
    row.push(format!("{}", as_path_str));
    row.push(format!("{}", communities_str));
    row.push(format!("{}", update.synthetic));

    return row.join(",");
}
