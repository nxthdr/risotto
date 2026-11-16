use bgpkit_parser::bmp::messages::RouteMonitoring as BmpRouteMonitoring;
use bgpkit_parser::models::*;
use core::net::IpAddr;

use crate::event::RouteMonitoring;

// Prefixes outside MP_(UN)REACH_NLRI are IPv4 unicast
const IPV4_UNICAST: (u16, u8) = (1, 1);

fn family(nlri: &Nlri) -> (u16, u8) {
    (u16::from(nlri.afi), u8::from(nlri.safi))
}

/// Address family (AFI, SAFI) of an End-of-RIB marker (RFC 4724),
/// or `None` if the message is not an End-of-RIB.
pub fn end_of_rib_family(message: &BmpRouteMonitoring) -> Option<(u16, u8)> {
    let BgpMessage::Update(bgp_update) = &message.bgp_message else {
        return None;
    };
    if !bgp_update.is_end_of_rib() {
        return None;
    }
    // IPv4 unicast End-of-RIB is an empty UPDATE; other families use an empty MP_UNREACH_NLRI
    Some(
        bgp_update
            .attributes
            .get_unreachable_nlri()
            .map(family)
            .unwrap_or(IPV4_UNICAST),
    )
}

/// Flatten a BMP Route Monitoring message into one [`RouteMonitoring`] per prefix.
/// Returns an empty vector for non-UPDATE messages and End-of-RIB markers.
pub fn decode_route_monitoring(message: BmpRouteMonitoring) -> Vec<RouteMonitoring> {
    let bgp_update = match message.bgp_message {
        BgpMessage::Update(update) => update,
        _ => return Vec::new(),
    };

    let attributes = &bgp_update.attributes;
    let reachable = attributes.get_reachable_nlri();
    let unreachable = attributes.get_unreachable_nlri();

    // Next hop: NEXT_HOP attribute for IPv4 unicast NLRI, MP_REACH_NLRI next hop otherwise
    let next_hop = attributes.next_hop().map(map_to_ipv6);
    let mp_next_hop = reachable
        .and_then(|nlri| nlri.next_hop.as_ref())
        .map(|nh| map_to_ipv6(nh.addr()));

    // (prefix, announced, address family, next hop)
    type Entry = (NetworkPrefix, bool, (u16, u8), Option<IpAddr>);
    let prefixes: Vec<Entry> = bgp_update
        .announced_prefixes
        .iter()
        .map(|p| (*p, true, IPV4_UNICAST, next_hop))
        .chain(
            bgp_update
                .withdrawn_prefixes
                .iter()
                .map(|p| (*p, false, IPV4_UNICAST, None)),
        )
        .chain(reachable.iter().flat_map(|n| {
            n.prefixes
                .iter()
                .map(move |p| (*p, true, family(n), mp_next_hop))
        }))
        .chain(
            unreachable
                .iter()
                .flat_map(|n| n.prefixes.iter().map(move |p| (*p, false, family(n), None))),
        )
        .collect();

    if prefixes.is_empty() {
        return Vec::new();
    }

    let communities: Vec<MetaCommunity> = attributes.iter_communities().collect();

    // Attributes are shared by every prefix of the UPDATE
    let base = RouteMonitoring {
        prefix_addr: IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED),
        prefix_len: 0,
        path_id: None,
        announced: false,
        afi: 0,
        safi: 0,
        initial_dump: false,

        origin: attributes.origin().to_string(),
        as_path: new_path(attributes.effective_as_path()),
        next_hop: None,
        multi_exit_discriminator: attributes.multi_exit_discriminator(),
        local_preference: attributes.local_preference(),
        only_to_customer: attributes.only_to_customer().map(|asn| asn.to_u32()),
        atomic_aggregate: attributes.atomic_aggregate(),
        aggregator_asn: attributes.aggregator().map(|(asn, _)| asn.to_u32()),
        aggregator_bgp_id: attributes.aggregator().map(|(_, id)| u32::from(id)),
        communities: new_communities(&communities),
        extended_communities: extract_extended_communities(&communities),
        large_communities: extract_large_communities(&communities),
        originator_id: attributes.origin_id().map(u32::from),
        cluster_list: attributes.clusters().map_or_else(Vec::new, |c| c.to_vec()),
        mp_reach_afi: reachable.map(|nlri| u16::from(nlri.afi)),
        mp_reach_safi: reachable.map(|nlri| u8::from(nlri.safi)),
        mp_unreach_afi: unreachable.map(|nlri| u16::from(nlri.afi)),
        mp_unreach_safi: unreachable.map(|nlri| u8::from(nlri.safi)),
    };

    prefixes
        .into_iter()
        .map(
            |(prefix, announced, (afi, safi), next_hop)| RouteMonitoring {
                prefix_addr: map_to_ipv6(prefix.prefix.addr()),
                prefix_len: prefix.prefix.prefix_len(),
                path_id: prefix.path_id,
                announced,
                afi,
                safi,
                next_hop,
                ..base.clone()
            },
        )
        .collect()
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
    communities
        .iter()
        .filter_map(|community| match community {
            MetaCommunity::Plain(community) => match community {
                bgpkit_parser::models::Community::Custom(asn, value) => {
                    Some((asn.to_u32(), *value))
                }
                // Well-known communities use reserved ASN 65535 (0xFFFF)
                bgpkit_parser::models::Community::NoExport => Some((0xFFFF, 0xFF01)),
                bgpkit_parser::models::Community::NoAdvertise => Some((0xFFFF, 0xFF02)),
                bgpkit_parser::models::Community::NoExportSubConfed => Some((0xFFFF, 0xFF03)),
            },
            // For extended and large communities, we could extract the first 32 bits
            // but since the return type is (u32, u16), we'll skip them for now
            // as they don't fit the standard community format
            MetaCommunity::Extended(_)
            | MetaCommunity::Ipv6Extended(_)
            | MetaCommunity::Large(_) => None,
        })
        .collect()
}

pub fn map_to_ipv6(ip: IpAddr) -> IpAddr {
    if ip.is_ipv4() {
        format!("::ffff:{}", ip).parse().unwrap()
    } else {
        ip
    }
}

// Helper functions for extracting community data into simple types
fn extract_extended_communities(communities: &[MetaCommunity]) -> Vec<(u8, u8, Vec<u8>)> {
    communities
        .iter()
        .filter_map(|community| match community {
            MetaCommunity::Extended(ext_comm) => {
                let type_byte = u8::from(ext_comm.community_type());
                match ext_comm {
                    bgpkit_parser::models::ExtendedCommunity::TransitiveTwoOctetAs(ec)
                    | bgpkit_parser::models::ExtendedCommunity::NonTransitiveTwoOctetAs(ec) => {
                        Some((
                            type_byte,
                            ec.subtype,
                            [
                                &ec.global_admin.to_u32().to_be_bytes()[2..],
                                &ec.local_admin,
                            ]
                            .concat(),
                        ))
                    }
                    bgpkit_parser::models::ExtendedCommunity::TransitiveIpv4Addr(ec)
                    | bgpkit_parser::models::ExtendedCommunity::NonTransitiveIpv4Addr(ec) => {
                        Some((
                            type_byte,
                            ec.subtype,
                            [
                                ec.global_admin.octets().as_slice(),
                                ec.local_admin.as_slice(),
                            ]
                            .concat(),
                        ))
                    }
                    bgpkit_parser::models::ExtendedCommunity::TransitiveFourOctetAs(ec)
                    | bgpkit_parser::models::ExtendedCommunity::NonTransitiveFourOctetAs(ec) => {
                        Some((
                            type_byte,
                            ec.subtype,
                            [
                                ec.global_admin.to_u32().to_be_bytes().as_slice(),
                                ec.local_admin.as_slice(),
                            ]
                            .concat(),
                        ))
                    }
                    bgpkit_parser::models::ExtendedCommunity::TransitiveOpaque(ec)
                    | bgpkit_parser::models::ExtendedCommunity::NonTransitiveOpaque(ec) => {
                        Some((type_byte, ec.subtype, ec.value.to_vec()))
                    }
                    bgpkit_parser::models::ExtendedCommunity::FlowSpecTrafficRate(fs) => Some((
                        type_byte,
                        0,
                        [
                            &fs.as_number.to_be_bytes()[..],
                            &fs.rate_bytes_per_sec.to_be_bytes()[..],
                        ]
                        .concat(),
                    )),
                    bgpkit_parser::models::ExtendedCommunity::FlowSpecTrafficAction(fs) => {
                        Some((type_byte, 0, vec![fs.sample as u8, fs.terminal as u8]))
                    }
                    bgpkit_parser::models::ExtendedCommunity::FlowSpecRedirect(ec) => Some((
                        type_byte,
                        ec.subtype,
                        [
                            &ec.global_admin.to_u32().to_be_bytes()[2..],
                            &ec.local_admin,
                        ]
                        .concat(),
                    )),
                    bgpkit_parser::models::ExtendedCommunity::FlowSpecTrafficMarking(fs) => {
                        Some((type_byte, 0, vec![fs.dscp]))
                    }
                    // RFC 7432 MAC Mobility: flags (bit 0 = sticky), reserved, 4-octet sequence
                    bgpkit_parser::models::ExtendedCommunity::MacMobility(mm) => Some((
                        type_byte,
                        0x00,
                        [
                            [u8::from(mm.sticky), 0u8].as_slice(),
                            mm.sequence.to_be_bytes().as_slice(),
                        ]
                        .concat(),
                    )),
                    // RFC 10005 link bandwidth: 2-octet global admin + IEEE-754 bandwidth
                    bgpkit_parser::models::ExtendedCommunity::LinkBandwidth(lb) => Some((
                        type_byte,
                        0x04,
                        [
                            lb.global_admin.to_be_bytes().as_slice(),
                            lb.bandwidth.to_be_bytes().as_slice(),
                        ]
                        .concat(),
                    )),
                    bgpkit_parser::models::ExtendedCommunity::Raw(raw) => {
                        Some((raw[0], raw[1], raw[2..].to_vec()))
                    }
                }
            }
            MetaCommunity::Ipv6Extended(ipv6_ext_comm) => {
                let type_byte = u8::from(ipv6_ext_comm.community_type);
                Some((
                    type_byte,
                    ipv6_ext_comm.subtype,
                    [
                        ipv6_ext_comm.global_admin.octets().as_slice(),
                        &ipv6_ext_comm.local_admin,
                    ]
                    .concat(),
                ))
            }
            _ => None,
        })
        .collect()
}

fn extract_large_communities(communities: &[MetaCommunity]) -> Vec<(u32, u32, u32)> {
    communities
        .iter()
        .filter_map(|community| match community {
            MetaCommunity::Large(large_comm) => Some((
                large_comm.global_admin,
                large_comm.local_data[0],
                large_comm.local_data[1],
            )),
            _ => None,
        })
        .collect()
}
