use capnp::message::Builder;
use capnp::serialize;
use chrono::{DateTime, Utc};
use std::net::{IpAddr, Ipv6Addr};

use risotto_lib::event::{
    Event, EventBody, InformationTlv, OpenMessage, PeerDown, PeerHeader, PeerUp, RouteMonitoring,
    StatsReport,
};

use crate::message_capnp::{information_tlv, message, open_message, peer_header};

pub fn serialize_ip_addr(ip: IpAddr) -> Vec<u8> {
    match ip {
        IpAddr::V4(addr) => addr.to_ipv6_mapped().octets().to_vec(),
        IpAddr::V6(addr) => addr.octets().to_vec(),
    }
}

fn nanos(time: DateTime<Utc>) -> u64 {
    time.timestamp_nanos_opt().unwrap_or_default() as u64
}

pub fn serialize_event(event: &Event) -> Vec<u8> {
    let mut builder = Builder::new_default();
    {
        let mut m = builder.init_root::<message::Builder>();
        m.set_time_received_ns(nanos(event.time_received));
        m.set_router_addr(&serialize_ip_addr(event.router_addr));
        m.set_router_port(event.router_port);
        m.set_session_start_ns(nanos(event.session_start));
        m.set_sequence(event.sequence);
        m.set_synthetic(event.synthetic);
        m.set_message_type(event.body.message_type() as u8);

        if let Some(header) = event.body.peer_header() {
            set_peer_header(m.reborrow().init_peer_header(), header);
        }

        match &event.body {
            EventBody::RouteMonitoring(_, rm) => {
                set_route_monitoring(m.reborrow().init_route_monitoring(), rm)
            }
            EventBody::StatsReport(_, stats) => {
                set_stats_report(m.reborrow().init_stats_report(), stats)
            }
            EventBody::PeerDown(_, peer_down) => {
                set_peer_down(m.reborrow().init_peer_down(), peer_down)
            }
            EventBody::PeerUp(_, peer_up) => set_peer_up(m.reborrow().init_peer_up(), peer_up),
            EventBody::Initiation(initiation) => {
                let i = m.reborrow().init_initiation();
                set_information(
                    i.init_information(initiation.information.len() as u32),
                    &initiation.information,
                );
            }
            EventBody::Termination(termination) => {
                let mut t = m.reborrow().init_termination();
                t.set_reason(termination.reason.unwrap_or_default());
                set_information(
                    t.init_information(termination.information.len() as u32),
                    &termination.information,
                );
            }
            EventBody::RouteMirroring(_) => {}
        }
    }

    serialize::write_message_to_words(&builder)
}

fn set_peer_header(mut h: peer_header::Builder, header: &PeerHeader) {
    h.set_peer_type(header.peer_type);
    h.set_peer_flags(header.peer_flags);
    h.set_peer_distinguisher(header.peer_distinguisher);
    h.set_peer_addr(&serialize_ip_addr(header.peer_addr));
    h.set_peer_asn(header.peer_asn);
    h.set_peer_bgp_id(header.peer_bgp_id.into());
    h.set_timestamp_ns(nanos(header.timestamp));
    h.set_is_post_policy(header.is_post_policy);
    h.set_is_adj_rib_out(header.is_adj_rib_out);
}

fn set_route_monitoring(
    mut r: crate::message_capnp::route_monitoring::Builder,
    rm: &RouteMonitoring,
) {
    r.set_prefix_addr(&serialize_ip_addr(rm.prefix_addr));
    r.set_prefix_len(rm.prefix_len);
    r.set_path_id(rm.path_id.unwrap_or_default());
    r.set_announced(rm.announced);
    r.set_afi(rm.afi);
    r.set_safi(rm.safi);
    r.set_initial_dump(rm.initial_dump);

    r.set_origin(&rm.origin);

    let mut as_path = r.reborrow().init_as_path(rm.as_path.len() as u32);
    for (i, &asn) in rm.as_path.iter().enumerate() {
        as_path.set(i as u32, asn);
    }

    let next_hop = rm.next_hop.unwrap_or(IpAddr::V6(Ipv6Addr::UNSPECIFIED));
    r.set_next_hop(&serialize_ip_addr(next_hop));
    r.set_multi_exit_disc(rm.multi_exit_discriminator.unwrap_or_default());
    r.set_local_preference(rm.local_preference.unwrap_or_default());
    r.set_only_to_customer(rm.only_to_customer.unwrap_or_default());
    r.set_atomic_aggregate(rm.atomic_aggregate);
    if let (Some(asn), Some(bgp_id)) = (rm.aggregator_asn, rm.aggregator_bgp_id) {
        r.set_aggregator_asn(asn);
        r.set_aggregator_bgp_id(bgp_id);
    }

    let mut communities = r.reborrow().init_communities(rm.communities.len() as u32);
    for (i, &(asn, value)) in rm.communities.iter().enumerate() {
        let mut c = communities.reborrow().get(i as u32);
        c.set_asn(asn);
        c.set_value(value);
    }

    let mut ext_communities = r
        .reborrow()
        .init_extended_communities(rm.extended_communities.len() as u32);
    for (i, (type_high, type_low, value)) in rm.extended_communities.iter().enumerate() {
        let mut c = ext_communities.reborrow().get(i as u32);
        c.set_type_high(*type_high);
        c.set_type_low(*type_low);
        c.set_value(value);
    }

    let mut large_communities = r
        .reborrow()
        .init_large_communities(rm.large_communities.len() as u32);
    for (i, &(global_admin, local_data1, local_data2)) in rm.large_communities.iter().enumerate() {
        let mut c = large_communities.reborrow().get(i as u32);
        c.set_global_admin(global_admin);
        c.set_local_data_part1(local_data1);
        c.set_local_data_part2(local_data2);
    }

    r.set_originator_id(rm.originator_id.unwrap_or_default());

    let mut cluster_list = r.reborrow().init_cluster_list(rm.cluster_list.len() as u32);
    for (i, &cluster_id) in rm.cluster_list.iter().enumerate() {
        cluster_list.set(i as u32, cluster_id);
    }

    r.set_mp_reach_afi(rm.mp_reach_afi.unwrap_or_default());
    r.set_mp_reach_safi(rm.mp_reach_safi.unwrap_or_default());
    r.set_mp_unreach_afi(rm.mp_unreach_afi.unwrap_or_default());
    r.set_mp_unreach_safi(rm.mp_unreach_safi.unwrap_or_default());
}

fn set_stats_report(s: crate::message_capnp::stats_report::Builder, stats: &StatsReport) {
    let mut counters = s.init_counters(stats.counters.len() as u32);
    for (i, counter) in stats.counters.iter().enumerate() {
        let mut c = counters.reborrow().get(i as u32);
        c.set_stat_type(counter.stat_type);
        c.set_afi(counter.afi.unwrap_or_default());
        c.set_safi(counter.safi.unwrap_or_default());
        c.set_value(counter.value);
    }
}

fn set_peer_down(mut p: crate::message_capnp::peer_down::Builder, peer_down: &PeerDown) {
    p.set_reason(peer_down.reason);
    p.set_data(&peer_down.data);
    if let Some((code, subcode)) = peer_down.notification {
        p.set_notification_code(code);
        p.set_notification_subcode(subcode);
    }
    p.set_fsm_event_code(peer_down.fsm_event_code.unwrap_or_default());
}

fn set_peer_up(mut p: crate::message_capnp::peer_up::Builder, peer_up: &PeerUp) {
    p.set_local_addr(&serialize_ip_addr(peer_up.local_addr));
    p.set_local_port(peer_up.local_port);
    p.set_remote_port(peer_up.remote_port);
    if let Some(open) = &peer_up.sent_open {
        set_open_message(p.reborrow().init_sent_open(), open);
    }
    if let Some(open) = &peer_up.received_open {
        set_open_message(p.reborrow().init_received_open(), open);
    }
    set_information(
        p.init_information(peer_up.information.len() as u32),
        &peer_up.information,
    );
}

fn set_open_message(mut o: open_message::Builder, open: &OpenMessage) {
    o.set_version(open.version);
    o.set_asn(open.asn);
    o.set_hold_time(open.hold_time);
    o.set_bgp_id(open.bgp_id.into());
    let mut capabilities = o.init_capabilities(open.capabilities.len() as u32);
    for (i, &code) in open.capabilities.iter().enumerate() {
        capabilities.set(i as u32, code);
    }
}

fn set_information(
    mut list: capnp::struct_list::Builder<information_tlv::Owned>,
    information: &[InformationTlv],
) {
    for (i, tlv) in information.iter().enumerate() {
        let mut t = list.reborrow().get(i as u32);
        t.set_info_type(tlv.info_type);
        t.set_value(&tlv.value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use capnp::message::ReaderOptions;
    use risotto_lib::event::{PeerDown, Termination};
    use std::net::Ipv4Addr;

    fn header() -> PeerHeader {
        PeerHeader {
            peer_type: 0,
            peer_flags: 0,
            peer_distinguisher: 0,
            peer_addr: "192.0.2.1".parse().unwrap(),
            peer_asn: 65001,
            peer_bgp_id: Ipv4Addr::new(192, 0, 2, 1),
            timestamp: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            is_post_policy: false,
            is_adj_rib_out: false,
        }
    }

    fn event(body: EventBody) -> Event {
        Event {
            time_received: DateTime::from_timestamp(1_700_000_001, 0).unwrap(),
            router_addr: "192.0.1.1".parse().unwrap(),
            router_port: 4000,
            session_start: DateTime::from_timestamp(1_700_000_000, 5).unwrap(),
            sequence: 42,
            synthetic: false,
            body,
        }
    }

    fn read(bytes: &[u8]) -> capnp::message::Reader<capnp::serialize::OwnedSegments> {
        serialize::read_message(&mut &bytes[..], ReaderOptions::new()).unwrap()
    }

    #[test]
    fn peer_down_round_trip() {
        let bytes = serialize_event(&event(EventBody::PeerDown(
            header(),
            PeerDown {
                reason: 3,
                data: vec![1, 2],
                notification: Some((6, 2)),
                fsm_event_code: None,
            },
        )));
        let reader = read(&bytes);
        let m = reader.get_root::<message::Reader>().unwrap();

        assert_eq!(m.get_message_type(), 2);
        assert_eq!(m.get_session_start_ns(), 1_700_000_000_000_000_005);
        assert_eq!(m.get_sequence(), 42);
        assert_eq!(
            m.get_router_addr().unwrap(),
            serialize_ip_addr("192.0.1.1".parse().unwrap())
        );
        assert!(m.has_peer_header());
        assert_eq!(m.get_peer_header().unwrap().get_peer_asn(), 65001);
        let peer_down = m.get_peer_down().unwrap();
        assert_eq!(peer_down.get_reason(), 3);
        assert_eq!(peer_down.get_notification_code(), 6);
        assert_eq!(peer_down.get_notification_subcode(), 2);
        // Only the matching body is set
        assert!(!m.has_route_monitoring());
        assert!(!m.has_peer_up());
    }

    #[test]
    fn synthetic_termination_round_trip() {
        let mut e = event(EventBody::Termination(Termination {
            reason: Some(1),
            information: vec![],
        }));
        e.synthetic = true;
        let bytes = serialize_event(&e);
        let reader = read(&bytes);
        let m = reader.get_root::<message::Reader>().unwrap();

        assert_eq!(m.get_message_type(), 5);
        assert!(m.get_synthetic());
        assert!(!m.has_peer_header());
        assert_eq!(m.get_termination().unwrap().get_reason(), 1);
    }
}
