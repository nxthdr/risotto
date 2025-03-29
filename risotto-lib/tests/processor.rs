use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use std::sync::mpsc;

use bgpkit_parser::bmp::messages::{
    PeerDownNotification, PeerDownReason, PeerUpNotification, RouteMonitoring,
};
use bgpkit_parser::models::{
    Asn, Attributes, BgpMessage, BgpOpenMessage, BgpUpdateMessage, NetworkPrefix,
};
use chrono::DateTime;
use risotto_lib::processor::{peer_down_notification, peer_up_notification, route_monitoring};
use risotto_lib::state::new_state;
use risotto_lib::update::{Update, UpdateMetadata};

fn default_open_message() -> BgpOpenMessage {
    BgpOpenMessage {
        version: 0,
        asn: Asn::from(0),
        hold_time: 0,
        sender_ip: Ipv4Addr::from_str("0.0.0.0").unwrap(),
        extended_length: false,
        opt_params: vec![],
    }
}

#[tokio::test]
async fn test_peer_up_notification() {
    let (tx, rx) = mpsc::channel();
    let state = new_state();

    let metadata = UpdateMetadata {
        timestamp: 0,
        router_addr: IpAddr::from_str("192.0.1.0").unwrap(),
        router_port: 179,
        peer_addr: IpAddr::from_str("192.0.2.0").unwrap(),
        peer_bgp_id: Ipv4Addr::from_str("192.0.2.0").unwrap(),
        peer_asn: 65000,
        is_post_policy: false,
        is_adj_rib_out: false,
    };

    let body = PeerUpNotification {
        local_addr: IpAddr::from_str("192.0.2.0").unwrap(),
        local_port: 10000,
        remote_port: 10001,
        sent_open: BgpMessage::Open(default_open_message()),
        received_open: BgpMessage::Open(default_open_message()),
        tlvs: vec![],
    };

    peer_up_notification(Some(state), tx, metadata, body).await;

    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn test_route_monitoring() {
    let mut tests = vec![];
    tests.push((
        UpdateMetadata {
            timestamp: 0,
            router_addr: IpAddr::from_str("192.0.1.0").unwrap(),
            router_port: 179,
            peer_addr: IpAddr::from_str("192.0.2.0").unwrap(),
            peer_bgp_id: Ipv4Addr::from_str("192.0.2.0").unwrap(),
            peer_asn: 65000,
            is_post_policy: false,
            is_adj_rib_out: false,
        },
        RouteMonitoring {
            bgp_message: BgpMessage::Update(BgpUpdateMessage {
                announced_prefixes: vec![
                    (NetworkPrefix {
                        prefix: "10.0.1.0/24".parse().unwrap(),
                        path_id: 0,
                    }),
                ],
                withdrawn_prefixes: vec![],
                attributes: Attributes::default(),
            }),
        },
        vec![Update {
            timestamp: DateTime::from_timestamp(0, 0).unwrap(),
            router_addr: IpAddr::from_str("192.0.1.0").unwrap(),
            router_port: 179,
            peer_addr: IpAddr::from_str("192.0.2.0").unwrap(),
            peer_bgp_id: Ipv4Addr::from_str("192.0.2.0").unwrap(),
            peer_asn: 65000,
            prefix_addr: IpAddr::from_str("::ffff:10.0.1.0").unwrap(),
            prefix_len: 24,
            announced: true,
            is_post_policy: false,
            is_adj_rib_out: false,
            origin: "INCOMPLETE".to_string(),
            path: vec![],
            communities: vec![],
            synthetic: false,
        }],
    ));

    for (metadata, body, expects) in tests {
        let (tx, rx) = mpsc::channel();
        let state = new_state();

        route_monitoring(Some(state), tx, metadata, body).await;

        for expect in expects.iter() {
            let update = rx.recv().unwrap();
            assert_eq!(update, expect.clone());
        }
    }
}

#[tokio::test]
async fn test_peer_down_notification() {
    let (tx, rx) = mpsc::channel();
    let state = new_state();

    let metadata = UpdateMetadata {
        timestamp: 0,
        router_addr: IpAddr::from_str("192.0.1.0").unwrap(),
        router_port: 179,
        peer_addr: IpAddr::from_str("192.0.2.0").unwrap(),
        peer_bgp_id: Ipv4Addr::from_str("192.0.2.0").unwrap(),
        peer_asn: 65000,
        is_post_policy: false,
        is_adj_rib_out: false,
    };

    let body = PeerDownNotification {
        reason: PeerDownReason::PeerDeConfigured,
        data: None,
    };

    peer_down_notification(Some(state), tx, metadata, body).await;

    assert!(rx.try_recv().is_err());
}
