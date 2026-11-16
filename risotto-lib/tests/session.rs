use bgpkit_parser::bmp::messages::{
    BmpCommonHeader, BmpMsgType, BmpPerPeerHeader, PeerDownNotification, PeerDownReason, PeerFlags,
    PeerUpNotification, PerPeerFlags, RouteMonitoring as BmpRouteMonitoring, TerminationMessage,
    TerminationReason, TerminationTlv, TerminationTlvType, TerminationTlvValue,
};
use bgpkit_parser::models::capabilities::{BgpCapabilityType, FourOctetAsCapability};
use bgpkit_parser::models::{
    Asn, Attributes, BgpMessage, BgpOpenMessage, BgpUpdateMessage, Capability, CapabilityValue,
    NetworkPrefix, OptParam, ParamValue,
};
use bgpkit_parser::parser::bmp::messages::{BmpMessage, BmpMessageBody};
use chrono::DateTime;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;

use risotto_lib::event::{EventBody, MessageType, TERMINATION_REASON_UNSPECIFIED};
use risotto_lib::BmpSession;

fn session() -> BmpSession {
    BmpSession::with_start(
        SocketAddr::from_str("192.0.1.1:4000").unwrap(),
        DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
    )
}

fn peer_header(flags: PeerFlags) -> BmpPerPeerHeader {
    BmpPerPeerHeader {
        peer_flags: PerPeerFlags::PeerFlags(flags),
        peer_ip: IpAddr::from_str("192.0.2.1").unwrap(),
        peer_asn: Asn::new_32bit(65001),
        peer_bgp_id: Ipv4Addr::from_str("192.0.2.1").unwrap(),
        timestamp: 1_700_000_000.123456,
        ..Default::default()
    }
}

fn message(
    msg_type: BmpMsgType,
    header: Option<BmpPerPeerHeader>,
    body: BmpMessageBody,
) -> BmpMessage {
    BmpMessage {
        common_header: BmpCommonHeader {
            version: 3,
            msg_len: 0,
            msg_type,
        },
        per_peer_header: header,
        message_body: body,
    }
}

fn open_message(asn: u32) -> BgpMessage {
    BgpMessage::Open(BgpOpenMessage {
        version: 4,
        asn: Asn::new_16bit(23456),
        hold_time: 90,
        bgp_identifier: Ipv4Addr::from_str("192.0.2.1").unwrap(),
        extended_length: false,
        opt_params: vec![OptParam {
            param_type: 2,
            param_value: ParamValue::Capacities(vec![Capability {
                ty: BgpCapabilityType::SUPPORT_FOR_4_OCTET_AS_NUMBER_CAPABILITY,
                value: CapabilityValue::FourOctetAs(FourOctetAsCapability { asn }),
            }]),
        }],
    })
}

#[test]
fn route_monitoring_is_flattened_per_prefix() {
    let body = BmpMessageBody::RouteMonitoring(BmpRouteMonitoring {
        bgp_message: BgpMessage::Update(BgpUpdateMessage {
            announced_prefixes: vec![NetworkPrefix {
                prefix: "10.0.1.0/24".parse().unwrap(),
                path_id: Some(7),
            }],
            withdrawn_prefixes: vec![NetworkPrefix {
                prefix: "10.0.2.0/24".parse().unwrap(),
                path_id: None,
            }],
            attributes: Attributes::default(),
        }),
    });
    let msg = message(
        BmpMsgType::RouteMonitoring,
        Some(peer_header(PeerFlags::IS_POST_POLICY)),
        body,
    );

    let events = session().process(msg).unwrap();
    assert_eq!(events.len(), 2);

    let event = &events[0];
    assert_eq!(event.body.message_type(), MessageType::RouteMonitoring);
    assert_eq!(
        event.router_addr,
        IpAddr::from_str("::ffff:192.0.1.1").unwrap()
    );
    assert_eq!(event.router_port, 4000);
    assert_eq!(event.session_start.timestamp(), 1_700_000_000);
    assert!(!event.synthetic);

    let EventBody::RouteMonitoring(header, rm) = &event.body else {
        panic!("expected route monitoring");
    };
    assert_eq!(
        header.peer_addr,
        IpAddr::from_str("::ffff:192.0.2.1").unwrap()
    );
    assert_eq!(header.peer_asn, 65001);
    assert!(header.is_post_policy);
    assert!(!header.is_adj_rib_out);
    assert_eq!(header.peer_flags, PeerFlags::IS_POST_POLICY.bits());
    assert_eq!(header.timestamp.timestamp_subsec_micros(), 123456);
    assert_eq!(rm.prefix_addr, IpAddr::from_str("::ffff:10.0.1.0").unwrap());
    assert_eq!(rm.prefix_len, 24);
    assert_eq!(rm.path_id, Some(7));
    assert!(rm.announced);

    let EventBody::RouteMonitoring(_, rm) = &events[1].body else {
        panic!("expected route monitoring");
    };
    assert_eq!(rm.prefix_addr, IpAddr::from_str("::ffff:10.0.2.0").unwrap());
    assert!(!rm.announced);
}

#[test]
fn end_of_rib_is_not_forwarded() {
    // RFC 4724: an UPDATE with no prefixes and no attributes is the IPv4 unicast End-of-RIB
    let body = BmpMessageBody::RouteMonitoring(BmpRouteMonitoring {
        bgp_message: BgpMessage::Update(BgpUpdateMessage::default()),
    });
    let msg = message(
        BmpMsgType::RouteMonitoring,
        Some(peer_header(PeerFlags::empty())),
        body,
    );
    assert!(session().process(msg).unwrap().is_empty());
}

#[test]
fn per_peer_message_without_header_is_an_error() {
    let body = BmpMessageBody::RouteMonitoring(BmpRouteMonitoring {
        bgp_message: BgpMessage::Update(BgpUpdateMessage::default()),
    });
    let msg = message(BmpMsgType::RouteMonitoring, None, body);
    assert!(session().process(msg).is_err());
}

#[test]
fn peer_up_carries_open_messages() {
    let body = BmpMessageBody::PeerUpNotification(PeerUpNotification {
        local_addr: IpAddr::from_str("192.0.2.254").unwrap(),
        local_port: 179,
        remote_port: 50000,
        sent_open: open_message(65000),
        received_open: open_message(4200000000),
        tlvs: vec![],
    });
    let msg = message(
        BmpMsgType::PeerUpNotification,
        Some(peer_header(PeerFlags::empty())),
        body,
    );

    let events = session().process(msg).unwrap();
    assert_eq!(events.len(), 1);
    let EventBody::PeerUp(_, peer_up) = &events[0].body else {
        panic!("expected peer up");
    };
    assert_eq!(
        peer_up.local_addr,
        IpAddr::from_str("::ffff:192.0.2.254").unwrap()
    );
    assert_eq!(peer_up.remote_port, 50000);
    let received = peer_up.received_open.as_ref().unwrap();
    // The 4-octet AS capability takes precedence over AS_TRANS in "My AS"
    assert_eq!(received.asn, 4200000000);
    assert_eq!(received.hold_time, 90);
    assert_eq!(received.capabilities, vec![65]);
}

#[test]
fn peer_down_decodes_notification() {
    // BGP NOTIFICATION PDU: 16-byte marker, length, type 3, code 6 (Cease), subcode 2
    let mut pdu = vec![0xff; 16];
    pdu.extend_from_slice(&[0, 21, 3, 6, 2]);
    let body = BmpMessageBody::PeerDownNotification(PeerDownNotification {
        reason: PeerDownReason::RemoteSystemClosedNotificationPduFollows,
        data: Some(pdu),
    });
    let msg = message(
        BmpMsgType::PeerDownNotification,
        Some(peer_header(PeerFlags::empty())),
        body,
    );

    let events = session().process(msg).unwrap();
    let EventBody::PeerDown(_, peer_down) = &events[0].body else {
        panic!("expected peer down");
    };
    assert_eq!(peer_down.reason, 3);
    assert_eq!(peer_down.notification, Some((6, 2)));
    assert_eq!(peer_down.fsm_event_code, None);
}

#[test]
fn close_without_termination_is_synthetic() {
    let mut session = session();
    let event = session.close().expect("synthetic termination");
    assert!(event.synthetic);
    let EventBody::Termination(termination) = &event.body else {
        panic!("expected termination");
    };
    assert_eq!(termination.reason, Some(TERMINATION_REASON_UNSPECIFIED));

    // Closing twice does not emit a second termination
    assert!(session.close().is_none());
}

#[test]
fn close_after_termination_emits_nothing() {
    let mut session = session();
    let body = BmpMessageBody::TerminationMessage(TerminationMessage {
        tlvs: vec![TerminationTlv {
            info_type: TerminationTlvType::Reason,
            info_len: 2,
            info_value: TerminationTlvValue::Reason(TerminationReason::AdministrativelyClosed),
        }],
    });
    let events = session
        .process(message(BmpMsgType::TerminationMessage, None, body))
        .unwrap();
    let EventBody::Termination(termination) = &events[0].body else {
        panic!("expected termination");
    };
    assert!(!events[0].synthetic);
    assert_eq!(termination.reason, Some(0));
    assert!(session.close().is_none());
}

fn peer_up() -> BmpMessageBody {
    BmpMessageBody::PeerUpNotification(PeerUpNotification {
        local_addr: IpAddr::from_str("192.0.2.254").unwrap(),
        local_port: 179,
        remote_port: 50000,
        sent_open: open_message(65000),
        received_open: open_message(65001),
        tlvs: vec![],
    })
}

fn announce(prefix: &str) -> BmpMessageBody {
    BmpMessageBody::RouteMonitoring(BmpRouteMonitoring {
        bgp_message: BgpMessage::Update(BgpUpdateMessage {
            announced_prefixes: vec![NetworkPrefix {
                prefix: prefix.parse().unwrap(),
                path_id: None,
            }],
            withdrawn_prefixes: vec![],
            attributes: Attributes::default(),
        }),
    })
}

fn end_of_rib() -> BmpMessageBody {
    BmpMessageBody::RouteMonitoring(BmpRouteMonitoring {
        bgp_message: BgpMessage::Update(BgpUpdateMessage::default()),
    })
}

/// Process a Route Monitoring message and return the `initial_dump` flag of its first event.
fn route(session: &mut BmpSession, flags: PeerFlags, body: BmpMessageBody) -> bool {
    let events = session
        .process(message(
            BmpMsgType::RouteMonitoring,
            Some(peer_header(flags)),
            body,
        ))
        .unwrap();
    let EventBody::RouteMonitoring(_, rm) = &events[0].body else {
        panic!("expected route monitoring");
    };
    rm.initial_dump
}

/// Process an End-of-RIB marker, which yields no event.
fn eor(session: &mut BmpSession, flags: PeerFlags) {
    let events = session
        .process(message(
            BmpMsgType::RouteMonitoring,
            Some(peer_header(flags)),
            end_of_rib(),
        ))
        .unwrap();
    assert!(events.is_empty());
}

#[test]
fn initial_dump_ends_with_end_of_rib() {
    let mut session = session();
    let pre = PeerFlags::empty();
    let post = PeerFlags::IS_POST_POLICY;

    // Without Peer Up in this session, updates are not flagged
    assert!(!route(&mut session, pre, announce("10.0.1.0/24")));

    session
        .process(message(
            BmpMsgType::PeerUpNotification,
            Some(peer_header(pre)),
            peer_up(),
        ))
        .unwrap();

    assert!(route(&mut session, pre, announce("10.0.1.0/24")));
    assert!(route(&mut session, post, announce("10.0.1.0/24")));

    // End-of-RIB of the pre-policy view only ends the pre-policy dump
    eor(&mut session, pre);
    assert!(!route(&mut session, pre, announce("10.0.2.0/24")));
    assert!(route(&mut session, post, announce("10.0.2.0/24")));

    // A session reset starts a new dump
    session
        .process(message(
            BmpMsgType::PeerUpNotification,
            Some(peer_header(pre)),
            peer_up(),
        ))
        .unwrap();
    assert!(route(&mut session, pre, announce("10.0.3.0/24")));
}

#[test]
fn prefixes_carry_their_address_family() {
    let EventBody::RouteMonitoring(_, rm) = &session()
        .process(message(
            BmpMsgType::RouteMonitoring,
            Some(peer_header(PeerFlags::empty())),
            announce("10.0.1.0/24"),
        ))
        .unwrap()[0]
        .body
    else {
        panic!("expected route monitoring");
    };
    assert_eq!((rm.afi, rm.safi), (1, 1));
}

#[test]
fn events_are_numbered_per_session() {
    let mut session = session();
    let events = session
        .process(message(
            BmpMsgType::RouteMonitoring,
            Some(peer_header(PeerFlags::empty())),
            BmpMessageBody::RouteMonitoring(BmpRouteMonitoring {
                bgp_message: BgpMessage::Update(BgpUpdateMessage {
                    announced_prefixes: vec![
                        NetworkPrefix {
                            prefix: "10.0.1.0/24".parse().unwrap(),
                            path_id: None,
                        },
                        NetworkPrefix {
                            prefix: "10.0.2.0/24".parse().unwrap(),
                            path_id: None,
                        },
                    ],
                    withdrawn_prefixes: vec![],
                    attributes: Attributes::default(),
                }),
            }),
        ))
        .unwrap();
    let termination = session.close().unwrap();
    assert_eq!(
        [events[0].sequence, events[1].sequence, termination.sequence],
        [0, 1, 2]
    );
}
