use bgpkit_parser::bmp::messages::{BmpMessage, PerPeerFlags, RouteMonitoring};
use bgpkit_parser::models::*;
use chrono::{DateTime, MappedLocalTime, TimeZone, Utc};
use core::net::IpAddr;
use std::net::Ipv4Addr;
use tracing::error;

#[derive(Debug, Clone, PartialEq)]
pub struct UpdateMetadata {
    pub timestamp: i64,
    pub router_addr: IpAddr,
    pub router_port: u16,
    pub peer_addr: IpAddr,
    pub peer_bgp_id: Ipv4Addr,
    pub peer_asn: u32,
    pub is_post_policy: bool,
    pub is_adj_rib_out: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub timestamp: DateTime<Utc>,
    pub router_addr: IpAddr,
    pub router_port: u16,
    pub peer_addr: IpAddr,
    pub peer_bgp_id: Ipv4Addr,
    pub peer_asn: u32,
    pub prefix_addr: IpAddr,
    pub prefix_len: u8,
    pub announced: bool,
    pub is_post_policy: bool,
    pub is_adj_rib_out: bool,
    pub origin: Origin,
    pub path: Vec<u32>,
    pub communities: Vec<(u32, u16)>,
    pub synthetic: bool,
}

pub fn decode_updates(message: RouteMonitoring, metadata: UpdateMetadata) -> Option<Vec<Update>> {
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
            if let Some(nlri) = attributes.get_reachable_nlri() {
                for prefix in &nlri.prefixes {
                    prefixes_to_update.push((*prefix, true));
                }
            }
            if let Some(nlri) = attributes.get_unreachable_nlri() {
                for prefix in &nlri.prefixes {
                    prefixes_to_update.push((*prefix, false));
                }
            }

            // Get the other attributes
            let origin = attributes.origin();
            let path = match attributes.as_path() {
                Some(path) => Some(path.clone()),
                None => None,
            };
            let communities: Vec<MetaCommunity> = attributes.iter_communities().collect();

            let timestamp = match Utc.timestamp_millis_opt(metadata.timestamp) {
                MappedLocalTime::Single(dt) => dt,
                _ => {
                    error!(
                        "failed to parse timestamp: {}, using Utc::now()",
                        metadata.timestamp
                    );
                    Utc::now()
                }
            };

            for (prefix, announced) in prefixes_to_update {
                updates.push(Update {
                    timestamp,
                    router_addr: metadata.router_addr,
                    router_port: metadata.router_port,
                    peer_addr: metadata.peer_addr,
                    peer_bgp_id: metadata.peer_bgp_id,
                    peer_asn: metadata.peer_asn,
                    prefix_addr: map_to_ipv6(prefix.prefix.addr()),
                    prefix_len: prefix.prefix.prefix_len(),
                    announced,
                    is_post_policy: metadata.is_post_policy,
                    is_adj_rib_out: metadata.is_adj_rib_out,
                    origin,
                    path: new_path(path.clone()),
                    communities: new_communities(&communities.clone()),
                    synthetic: false,
                });
            }

            Some(updates)
        }
        _ => None,
    }
}

pub fn new_metadata(
    router_addr: IpAddr,
    router_port: u16,
    message: BmpMessage,
) -> Option<UpdateMetadata> {
    // Get peer information
    let Some(pph) = message.per_peer_header else {
        return None;
    };
    let peer = Peer::new(pph.peer_bgp_id, pph.peer_ip, pph.peer_asn);

    // Get header information
    let timestamp = (pph.timestamp * 1000.0) as i64;

    let is_post_policy = match pph.peer_flags {
        PerPeerFlags::PeerFlags(flags) => flags.is_post_policy(),
        PerPeerFlags::LocalRibPeerFlags(_) => false,
    };

    let is_adj_rib_out = match pph.peer_flags {
        PerPeerFlags::PeerFlags(flags) => flags.is_adj_rib_out(),
        PerPeerFlags::LocalRibPeerFlags(_) => false,
    };

    Some(UpdateMetadata {
        timestamp,
        router_addr,
        router_port,
        peer_addr: map_to_ipv6(peer.peer_address),
        peer_bgp_id: peer.peer_bgp_id,
        peer_asn: peer.peer_asn.to_u32(),
        is_post_policy,
        is_adj_rib_out,
    })
}

pub fn new_peer_from_metadata(metadata: UpdateMetadata) -> Peer {
    Peer::new(
        metadata.peer_bgp_id,
        metadata.peer_addr,
        Asn::new_32bit(metadata.peer_asn),
    )
}

pub fn new_path(path: Option<AsPath>) -> Vec<u32> {
    match path {
        Some(mut path) => {
            let mut constructed_path: Vec<u32> = Vec::new();
            path.coalesce();
            for segment in path.into_segments_iter() {
                if let AsPathSegment::AsSequence(dedup_asns) = segment {
                    for asn in dedup_asns {
                        constructed_path.push(asn.to_u32());
                    }
                }
            }
            constructed_path
        }
        None => Vec::new(),
    }
}

pub fn new_communities(communities: &[MetaCommunity]) -> Vec<(u32, u16)> {
    let mut constructed_communities = Vec::new();
    for community in communities {
        match community {
            MetaCommunity::Plain(community) => match community {
                bgpkit_parser::models::Community::Custom(asn, value) => {
                    constructed_communities.push((asn.to_u32(), *value));
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
        format!("::ffff:{}", ip).parse().unwrap()
    } else {
        ip
    }
}
