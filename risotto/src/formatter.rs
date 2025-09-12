use capnp::message::Builder;
use capnp::serialize;
use std::net::IpAddr;

use risotto_lib::update::Update;

use crate::update_capnp::update;

pub fn serialize_ip_addr(ip: IpAddr) -> Vec<u8> {
    match ip {
        IpAddr::V4(addr) => addr.to_ipv6_mapped().octets().to_vec(),
        IpAddr::V6(addr) => addr.octets().to_vec(),
    }
}

pub fn serialize_update(update: &Update) -> Vec<u8> {
    let mut message = Builder::new_default();
    {
        let mut u = message.init_root::<update::Builder>();
        u.set_time_received_ns(update.time_received_ns.timestamp_nanos_opt().unwrap() as u64);
        u.set_time_bmp_header_ns(update.time_bmp_header_ns.timestamp_nanos_opt().unwrap() as u64);
        u.set_router_addr(&serialize_ip_addr(update.router_addr));
        u.set_router_port(update.router_port);
        u.set_peer_addr(&serialize_ip_addr(update.peer_addr));
        u.set_peer_bgp_id(update.peer_bgp_id.into());
        u.set_peer_asn(update.peer_asn);
        u.set_prefix_addr(&serialize_ip_addr(update.prefix_addr));
        u.set_prefix_len(update.prefix_len);
        u.set_is_post_policy(update.is_post_policy);
        u.set_is_adj_rib_out(update.is_adj_rib_out);
        u.set_announced(update.announced);
        u.set_synthetic(update.synthetic);

        // BGP Attributes - structured fields
        u.set_origin(&update.origin);

        // AS Path
        let mut as_path = u.reborrow().init_as_path(update.as_path.len() as u32);
        for (i, &asn) in update.as_path.iter().enumerate() {
            as_path.set(i as u32, asn);
        }

        // Next Hop
        if let Some(next_hop) = update.next_hop {
            u.set_next_hop(&serialize_ip_addr(next_hop));
        }

        // Multi Exit Discriminator
        u.set_multi_exit_disc(update.multi_exit_discriminator.unwrap_or(0));

        // Local Preference
        u.set_local_preference(update.local_preference.unwrap_or(0));

        // Only To Customer
        u.set_only_to_customer(update.only_to_customer.unwrap_or(0));

        // Atomic Aggregate
        u.set_atomic_aggregate(update.atomic_aggregate);

        // Aggregator
        if let (Some(asn), Some(bgp_id)) = (update.aggregator_asn, update.aggregator_bgp_id) {
            u.set_aggregator_asn(asn);
            u.set_aggregator_bgp_id(bgp_id);
        }

        // Communities
        let mut communities = u
            .reborrow()
            .init_communities(update.communities.len() as u32);
        for (i, &(asn, value)) in update.communities.iter().enumerate() {
            let mut community = communities.reborrow().get(i as u32);
            community.set_asn(asn);
            community.set_value(value);
        }

        // Extended Communities
        let mut ext_communities = u
            .reborrow()
            .init_extended_communities(update.extended_communities.len() as u32);
        for (i, (type_high, type_low, value)) in update.extended_communities.iter().enumerate() {
            let mut ext_community = ext_communities.reborrow().get(i as u32);
            ext_community.set_type_high(*type_high);
            ext_community.set_type_low(*type_low);
            ext_community.set_value(value);
        }

        // Large Communities
        let mut large_communities = u
            .reborrow()
            .init_large_communities(update.large_communities.len() as u32);
        for (i, &(global_admin, local_data1, local_data2)) in
            update.large_communities.iter().enumerate()
        {
            let mut large_community = large_communities.reborrow().get(i as u32);
            large_community.set_global_admin(global_admin);
            large_community.set_local_data_part1(local_data1);
            large_community.set_local_data_part2(local_data2);
        }

        // Originator ID
        if let Some(originator_id) = update.originator_id {
            u.set_originator_id(originator_id);
        }

        // Cluster List
        let mut cluster_list = u
            .reborrow()
            .init_cluster_list(update.cluster_list.len() as u32);
        for (i, &cluster_id) in update.cluster_list.iter().enumerate() {
            cluster_list.set(i as u32, cluster_id);
        }

        // MP Reach NLRI
        if let Some(afi) = update.mp_reach_afi {
            u.set_mp_reach_afi(afi);
        }
        if let Some(safi) = update.mp_reach_safi {
            u.set_mp_reach_safi(safi);
        }

        // MP Unreach NLRI
        if let Some(afi) = update.mp_unreach_afi {
            u.set_mp_unreach_afi(afi);
        }
        if let Some(safi) = update.mp_unreach_safi {
            u.set_mp_unreach_safi(safi);
        }
    }

    serialize::write_message_to_words(&message)
}
