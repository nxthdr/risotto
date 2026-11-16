use anyhow::{anyhow, Result};
use bgpkit_parser::bmp::messages::{
    BmpPerPeerHeader, InitiationMessage, PeerDownNotification, PeerUpNotification, PerPeerFlags,
    StatsData, StatsReport as BmpStatsReport, TerminationMessage, TerminationTlvValue,
};
use bgpkit_parser::models::{BgpMessage, CapabilityValue, ParamValue};
use bgpkit_parser::parse_bmp_msg;
use bgpkit_parser::parser::bmp::messages::{BmpMessage, BmpMessageBody};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};

use crate::event::*;
use crate::route_monitoring::{decode_route_monitoring, end_of_rib_family, map_to_ipv6};

/// A peer within a BMP session: peer address and peer distinguisher.
type PeerKey = (IpAddr, u64);

/// A RIB view and address family of a peer: (is_post_policy, is_adj_rib_out, afi, safi).
type RibFamily = (bool, bool, u16, u8);

/// Decoder for the messages of one BMP session (one TCP connection from a router).
///
/// It keeps no routing state. It only tracks, for the lifetime of the connection:
/// - whether the router sent a Termination message, so that a synthetic Termination
///   can be emitted when the connection ends without one (see [`BmpSession::close`]);
/// - which RIB views and address families of each peer have sent their End-of-RIB
///   since Peer Up, to flag the updates of the initial table dump (`initial_dump`).
#[derive(Debug, Clone)]
pub struct BmpSession {
    router_addr: IpAddr,
    router_port: u16,
    start: DateTime<Utc>,
    /// Sequence number of the next event.
    sequence: u64,
    terminated: bool,
    /// Peers seen up in this session, with the RIB families whose End-of-RIB was received.
    dumps: HashMap<PeerKey, HashSet<RibFamily>>,
}

impl BmpSession {
    /// Start a new session for a router connection, established now.
    pub fn new(router: SocketAddr) -> Self {
        Self::with_start(router, Utc::now())
    }

    /// Start a new session for a router connection, established at `start`.
    pub fn with_start(router: SocketAddr, start: DateTime<Utc>) -> Self {
        BmpSession {
            router_addr: map_to_ipv6(router.ip()),
            router_port: router.port(),
            start,
            sequence: 0,
            terminated: false,
            dumps: HashMap::new(),
        }
    }

    pub fn start(&self) -> DateTime<Utc> {
        self.start
    }

    /// Decode one raw BMP message into events.
    pub fn decode(&mut self, bytes: &mut Bytes) -> Result<Vec<Event>> {
        let message =
            parse_bmp_msg(bytes).map_err(|e| anyhow!("failed to parse BMP message: {}", e))?;
        self.process(message)
    }

    /// Convert one parsed BMP message into events.
    /// Route Monitoring messages yield one event per prefix; other messages yield one event.
    pub fn process(&mut self, message: BmpMessage) -> Result<Vec<Event>> {
        let header = message.per_peer_header.map(new_peer_header);
        let message_type = message.common_header.msg_type;
        let peer_header = || {
            header
                .clone()
                .ok_or_else(|| anyhow!("{:?}: missing per-peer header", message_type))
        };

        let bodies = match message.message_body {
            BmpMessageBody::RouteMonitoring(body) => {
                let header = peer_header()?;
                // End-of-RIB markers only end the initial dump, they are not forwarded
                if let Some((afi, safi)) = end_of_rib_family(&body) {
                    self.end_initial_dump(&header, afi, safi);
                    return Ok(Vec::new());
                }
                decode_route_monitoring(body)
                    .into_iter()
                    .map(|mut rm| {
                        self.track_initial_dump(&header, &mut rm);
                        EventBody::RouteMonitoring(header.clone(), rm)
                    })
                    .collect()
            }
            BmpMessageBody::StatsReport(body) => {
                vec![EventBody::StatsReport(
                    peer_header()?,
                    new_stats_report(body),
                )]
            }
            BmpMessageBody::PeerDownNotification(body) => {
                let header = peer_header()?;
                self.dumps.remove(&peer_key(&header));
                vec![EventBody::PeerDown(header, new_peer_down(body))]
            }
            BmpMessageBody::PeerUpNotification(body) => {
                let header = peer_header()?;
                // A new initial table dump starts for every RIB view and family
                self.dumps.insert(peer_key(&header), HashSet::new());
                vec![EventBody::PeerUp(header, new_peer_up(body))]
            }
            BmpMessageBody::InitiationMessage(body) => {
                vec![EventBody::Initiation(new_initiation(body))]
            }
            BmpMessageBody::TerminationMessage(body) => {
                self.terminated = true;
                self.dumps.clear();
                vec![EventBody::Termination(new_termination(body))]
            }
            BmpMessageBody::RouteMirroring(_) => vec![EventBody::RouteMirroring(peer_header()?)],
        };

        Ok(bodies.into_iter().map(|b| self.event(b, false)).collect())
    }

    /// Close the session. Returns a synthetic Termination event if the router
    /// did not send a Termination message, so downstream consumers know that
    /// the session ended. This is a loss of observability, not a Peer Down:
    /// the BGP sessions of the router are not known to have gone down.
    pub fn close(&mut self) -> Option<Event> {
        if self.terminated {
            return None;
        }
        self.terminated = true;
        let body = EventBody::Termination(Termination {
            reason: Some(TERMINATION_REASON_UNSPECIFIED),
            information: vec![],
        });
        Some(self.event(body, true))
    }

    /// Flag updates received between Peer Up and the End-of-RIB of their RIB view and family.
    /// Updates of peers whose Peer Up was not seen in this session are not flagged.
    fn track_initial_dump(&self, header: &PeerHeader, rm: &mut RouteMonitoring) {
        if let Some(completed) = self.dumps.get(&peer_key(header)) {
            let family = (
                header.is_post_policy,
                header.is_adj_rib_out,
                rm.afi,
                rm.safi,
            );
            rm.initial_dump = !completed.contains(&family);
        }
    }

    /// Record the End-of-RIB of a peer's RIB view and family: its initial dump is complete.
    fn end_initial_dump(&mut self, header: &PeerHeader, afi: u16, safi: u8) {
        if let Some(completed) = self.dumps.get_mut(&peer_key(header)) {
            completed.insert((header.is_post_policy, header.is_adj_rib_out, afi, safi));
        }
    }

    fn event(&mut self, body: EventBody, synthetic: bool) -> Event {
        let sequence = self.sequence;
        self.sequence += 1;
        Event {
            time_received: Utc::now(),
            router_addr: self.router_addr,
            router_port: self.router_port,
            session_start: self.start,
            sequence,
            synthetic,
            body,
        }
    }
}

/// Decode a single BMP message outside of any session context.
/// Prefer [`BmpSession`] for collectors, which also tracks session termination.
pub fn decode_bmp_message(router: SocketAddr, bytes: &mut Bytes) -> Result<Vec<Event>> {
    BmpSession::new(router).decode(bytes)
}

fn peer_key(header: &PeerHeader) -> PeerKey {
    (header.peer_addr, header.peer_distinguisher)
}

pub fn new_peer_header(pph: BmpPerPeerHeader) -> PeerHeader {
    let (peer_flags, is_post_policy, is_adj_rib_out) = match pph.peer_flags {
        PerPeerFlags::PeerFlags(flags) => {
            (flags.bits(), flags.is_post_policy(), flags.is_adj_rib_out())
        }
        PerPeerFlags::LocalRibPeerFlags(flags) => (flags.bits(), false, false),
    };

    PeerHeader {
        peer_type: u8::from(pph.peer_type),
        peer_flags,
        peer_distinguisher: pph.peer_distinguisher,
        peer_addr: map_to_ipv6(pph.peer_ip),
        peer_asn: pph.peer_asn.to_u32(),
        peer_bgp_id: pph.peer_bgp_id,
        timestamp: timestamp_from_secs(pph.timestamp),
        is_post_policy,
        is_adj_rib_out,
    }
}

/// Convert the per-peer header timestamp (seconds with microsecond precision) to a date.
fn timestamp_from_secs(timestamp: f64) -> DateTime<Utc> {
    let secs = timestamp.trunc();
    let micros = ((timestamp - secs) * 1e6).round().clamp(0.0, 999_999.0) as u32;
    DateTime::from_timestamp(secs as i64, micros * 1000).unwrap_or_default()
}

pub fn new_peer_down(body: PeerDownNotification) -> PeerDown {
    let reason = u8::from(body.reason);
    let data = body.data.unwrap_or_default();

    // Reasons 1 and 3: a BGP NOTIFICATION PDU follows (19-byte BGP header, then code and subcode)
    let notification = match reason {
        1 | 3 if data.len() >= 21 => Some((data[19], data[20])),
        _ => None,
    };
    // Reason 2: a 2-byte FSM event code follows
    let fsm_event_code = match reason {
        2 if data.len() >= 2 => Some(u16::from_be_bytes([data[0], data[1]])),
        _ => None,
    };

    PeerDown {
        reason,
        data,
        notification,
        fsm_event_code,
    }
}

pub fn new_peer_up(body: PeerUpNotification) -> PeerUp {
    PeerUp {
        local_addr: map_to_ipv6(body.local_addr),
        local_port: body.local_port,
        remote_port: body.remote_port,
        sent_open: new_open_message(&body.sent_open),
        received_open: new_open_message(&body.received_open),
        information: body
            .tlvs
            .into_iter()
            .map(|tlv| InformationTlv {
                info_type: u16::from(tlv.info_type),
                value: tlv.info_value,
            })
            .collect(),
    }
}

fn new_open_message(message: &BgpMessage) -> Option<OpenMessage> {
    let BgpMessage::Open(open) = message else {
        return None;
    };

    let capabilities: Vec<_> = open
        .opt_params
        .iter()
        .filter_map(|param| match &param.param_value {
            ParamValue::Capacities(caps) => Some(caps),
            ParamValue::Raw(_) => None,
        })
        .flatten()
        .collect();

    let four_octet_asn = capabilities.iter().find_map(|cap| match &cap.value {
        CapabilityValue::FourOctetAs(c) => Some(c.asn),
        _ => None,
    });

    Some(OpenMessage {
        version: open.version,
        asn: four_octet_asn.unwrap_or(open.asn.to_u32()),
        hold_time: open.hold_time,
        bgp_id: open.bgp_identifier,
        capabilities: capabilities.iter().map(|cap| u8::from(cap.ty)).collect(),
    })
}

pub fn new_stats_report(body: BmpStatsReport) -> StatsReport {
    let counters = body
        .counters
        .into_iter()
        .filter_map(|counter| {
            let stat_type = u16::from(counter.stat_type);
            let (afi, safi, value) = match counter.stat_data {
                StatsData::Counter(v) => (None, None, v as u64),
                StatsData::Gauge(v) => (None, None, v),
                StatsData::AfiSafiGauge(afi, safi, v) => (Some(afi), Some(safi), v),
                StatsData::Unknown(_) => return None,
            };
            Some(StatCounter {
                stat_type,
                afi,
                safi,
                value,
            })
        })
        .collect();
    StatsReport { counters }
}

pub fn new_initiation(body: InitiationMessage) -> Initiation {
    Initiation {
        information: body
            .tlvs
            .into_iter()
            .map(|tlv| InformationTlv {
                info_type: u16::from(tlv.info_type),
                value: tlv.info,
            })
            .collect(),
    }
}

pub fn new_termination(body: TerminationMessage) -> Termination {
    let mut termination = Termination::default();
    for tlv in body.tlvs {
        match tlv.info_value {
            TerminationTlvValue::Reason(reason) => termination.reason = Some(u16::from(reason)),
            TerminationTlvValue::String(value) => termination.information.push(InformationTlv {
                info_type: u16::from(tlv.info_type),
                value,
            }),
        }
    }
    termination
}
