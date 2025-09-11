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
        u.set_next_hop(&serialize_ip_addr(
            update.next_hop.unwrap_or(IpAddr::from([0; 16])),
        ));
        u.set_origin(&update.origin);
        let _ = u.set_path(&update.path[..]);
        u.set_local_preference(update.local_preference.unwrap_or(0));
        u.set_med(update.med.unwrap_or(0));
        let mut communities = u
            .reborrow()
            .init_communities(update.communities.len() as u32);
        for (i, &(asn, value)) in update.communities.iter().enumerate() {
            let mut community = communities.reborrow().get(i as u32);
            community.set_asn(asn);
            community.set_value(value);
        }
        u.set_synthetic(update.synthetic);
        
        // Serialize BGP attributes
        let mut attributes = u
            .reborrow()
            .init_attributes(update.attributes.len() as u32);
        for (i, (type_code, value)) in update.attributes.iter().enumerate() {
            let mut attribute = attributes.reborrow().get(i as u32);
            attribute.set_type_code(*type_code);
            attribute.set_value(value);
        }
    }

    serialize::write_message_to_words(&message)
}
