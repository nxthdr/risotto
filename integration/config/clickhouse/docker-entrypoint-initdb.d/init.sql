CREATE DATABASE IF NOT EXISTS bmp;

-- Raw BMP messages from Risotto (one row per Kafka Cap'n Proto message).
-- Every message has the common fields, a per-peer header (except Initiation and
-- Termination) and exactly one body, selected by `messageType`:
-- 0 RouteMonitoring, 1 StatsReport, 2 PeerDown, 3 PeerUp, 4 Initiation, 5 Termination.
-- Unset bodies are read as default values. IP addresses inside nested structs are
-- read as FixedString(16) (an unset address is 0 bytes, which IPv6 rejects) and cast
-- to IPv6 in the materialized views.
CREATE TABLE bmp.from_kafka
(
    timeReceivedNs   UInt64,
    routerAddr       IPv6,
    routerPort       UInt16,
    sessionStartNs   UInt64,
    synthetic        Bool,
    messageType      UInt8,
    sequence         UInt64,

    peerHeader Tuple(
        peerType          UInt8,
        peerFlags         UInt8,
        peerDistinguisher UInt64,
        peerAddr          FixedString(16),
        peerAsn           UInt32,
        peerBgpId         IPv4,
        timestampNs       UInt64,
        isPostPolicy      Bool,
        isAdjRibOut       Bool
    ),

    routeMonitoring Tuple(
        prefixAddr          FixedString(16),
        prefixLen           UInt8,
        pathId              UInt32,
        announced           Bool,
        origin              String,
        asPath              Array(UInt32),
        nextHop             FixedString(16),
        multiExitDisc       UInt32,
        localPreference     UInt32,
        onlyToCustomer      UInt32,
        atomicAggregate     Bool,
        aggregatorAsn       UInt32,
        aggregatorBgpId     IPv4,
        communities         Array(Tuple(UInt32, UInt16)),
        extendedCommunities Array(Tuple(UInt8, UInt8, String)),
        largeCommunities    Array(Tuple(UInt32, UInt32, UInt32)),
        originatorId        IPv4,
        clusterList         Array(UInt32),
        mpReachAfi          UInt16,
        mpReachSafi         UInt8,
        mpUnreachAfi        UInt16,
        mpUnreachSafi       UInt8,
        afi                 UInt16,
        safi                UInt8,
        initialDump         Bool
    ),

    statsReport Tuple(
        counters Array(Tuple(statType UInt16, afi UInt16, safi UInt8, value UInt64))
    ),

    peerDown Tuple(
        reason              UInt8,
        data                String,
        notificationCode    UInt8,
        notificationSubcode UInt8,
        fsmEventCode        UInt16
    ),

    peerUp Tuple(
        localAddr    FixedString(16),
        localPort    UInt16,
        remotePort   UInt16,
        sentOpen     Tuple(version UInt8, asn UInt32, holdTime UInt16, bgpId IPv4, capabilities Array(UInt8)),
        receivedOpen Tuple(version UInt8, asn UInt32, holdTime UInt16, bgpId IPv4, capabilities Array(UInt8)),
        information  Array(Tuple(infoType UInt16, value String))
    ),

    initiation Tuple(
        information Array(Tuple(infoType UInt16, value String))
    ),

    termination Tuple(
        reason      UInt16,
        information Array(Tuple(infoType UInt16, value String))
    ),

    -- End-of-RIB markers are Route Monitoring messages (messageType 0) with this
    -- body set instead of routeMonitoring. An unset body reads as afi = 0.
    endOfRib Tuple(
        afi  UInt16,
        safi UInt8
    )
)
ENGINE = Kafka()
SETTINGS
    kafka_broker_list = '10.0.0.100:9092',
    kafka_topic_list = 'risotto',
    kafka_group_name = 'clickhouse-risotto-group',
    kafka_format = 'CapnProto',
    kafka_schema = 'message.capnp:Message',
    kafka_max_rows_per_message = 1048576;

-- Route Monitoring: one row per prefix update
CREATE TABLE bmp.updates
(
    date                 Date,
    time_inserted_ns     DateTime64(9),
    time_received_ns     DateTime64(9),
    time_bmp_header_ns   DateTime64(9),
    session_start_ns     DateTime64(9),
    sequence             UInt64,

    router_addr          IPv6,
    router_port          UInt16,
    peer_type            UInt8,
    peer_distinguisher   UInt64,
    peer_addr            IPv6,
    peer_bgp_id          IPv4,
    peer_asn             UInt32,
    is_post_policy       Bool,
    is_adj_rib_out       Bool,

    prefix_addr          IPv6,
    prefix_len           UInt8,
    path_id              UInt32,
    announced            Bool,
    afi                  UInt16,
    safi                 UInt8,
    -- Received during the initial table dump after Peer Up (before End-of-RIB):
    -- not a routing change, filter out with `NOT initial_dump` to study BGP dynamics
    initial_dump         Bool,

    origin               String,
    as_path              Array(UInt32),
    next_hop             IPv6,
    multi_exit_disc      UInt32,
    local_preference     UInt32,
    only_to_customer     UInt32,
    atomic_aggregate     Bool,
    aggregator_asn       UInt32,
    aggregator_bgp_id    IPv4,
    communities          Array(Tuple(UInt32, UInt16)),
    extended_communities Array(Tuple(UInt8, UInt8, String)),
    large_communities    Array(Tuple(UInt32, UInt32, UInt32)),
    originator_id        IPv4,
    cluster_list         Array(UInt32),
    mp_reach_afi         UInt16,
    mp_reach_safi        UInt8,
    mp_unreach_afi       UInt16,
    mp_unreach_safi      UInt8
)
ENGINE = MergeTree()
ORDER BY (time_received_ns, router_addr, peer_addr, prefix_addr, prefix_len)
PARTITION BY date
TTL date + INTERVAL 7 DAY DELETE;

CREATE MATERIALIZED VIEW bmp.updates_mv TO bmp.updates AS
SELECT
    toDate(fromUnixTimestamp64Nano(toInt64(timeReceivedNs))) AS date,
    now64(9) AS time_inserted_ns,
    fromUnixTimestamp64Nano(toInt64(timeReceivedNs)) AS time_received_ns,
    fromUnixTimestamp64Nano(toInt64(peerHeader.timestampNs)) AS time_bmp_header_ns,
    fromUnixTimestamp64Nano(toInt64(sessionStartNs)) AS session_start_ns,
    sequence,

    routerAddr AS router_addr,
    routerPort AS router_port,
    peerHeader.peerType AS peer_type,
    peerHeader.peerDistinguisher AS peer_distinguisher,
    CAST(peerHeader.peerAddr AS IPv6) AS peer_addr,
    peerHeader.peerBgpId AS peer_bgp_id,
    peerHeader.peerAsn AS peer_asn,
    peerHeader.isPostPolicy AS is_post_policy,
    peerHeader.isAdjRibOut AS is_adj_rib_out,

    CAST(routeMonitoring.prefixAddr AS IPv6) AS prefix_addr,
    routeMonitoring.prefixLen AS prefix_len,
    routeMonitoring.pathId AS path_id,
    routeMonitoring.announced AS announced,
    routeMonitoring.afi AS afi,
    routeMonitoring.safi AS safi,
    routeMonitoring.initialDump AS initial_dump,

    routeMonitoring.origin AS origin,
    routeMonitoring.asPath AS as_path,
    CAST(routeMonitoring.nextHop AS IPv6) AS next_hop,
    routeMonitoring.multiExitDisc AS multi_exit_disc,
    routeMonitoring.localPreference AS local_preference,
    routeMonitoring.onlyToCustomer AS only_to_customer,
    routeMonitoring.atomicAggregate AS atomic_aggregate,
    routeMonitoring.aggregatorAsn AS aggregator_asn,
    routeMonitoring.aggregatorBgpId AS aggregator_bgp_id,
    routeMonitoring.communities AS communities,
    routeMonitoring.extendedCommunities AS extended_communities,
    routeMonitoring.largeCommunities AS large_communities,
    routeMonitoring.originatorId AS originator_id,
    routeMonitoring.clusterList AS cluster_list,
    routeMonitoring.mpReachAfi AS mp_reach_afi,
    routeMonitoring.mpReachSafi AS mp_reach_safi,
    routeMonitoring.mpUnreachAfi AS mp_unreach_afi,
    routeMonitoring.mpUnreachSafi AS mp_unreach_safi
FROM bmp.from_kafka
WHERE messageType = 0 AND endOfRib.afi = 0;

-- End-of-RIB markers (RFC 4724): the table dump of a peer's RIB view
-- (pre/post-policy, Adj-RIB-In/Out) and address family is complete.
CREATE TABLE bmp.end_of_ribs
(
    date                Date,
    time_inserted_ns    DateTime64(9),
    time_received_ns    DateTime64(9),
    time_bmp_header_ns  DateTime64(9),
    session_start_ns    DateTime64(9),
    sequence            UInt64,

    router_addr         IPv6,
    router_port         UInt16,
    peer_type           UInt8,
    peer_distinguisher  UInt64,
    peer_addr           IPv6,
    peer_bgp_id         IPv4,
    peer_asn            UInt32,
    is_post_policy      Bool,
    is_adj_rib_out      Bool,

    afi                 UInt16,
    safi                UInt8
)
ENGINE = MergeTree()
ORDER BY (time_received_ns, router_addr, peer_addr)
PARTITION BY date
TTL date + INTERVAL 7 DAY DELETE;

CREATE MATERIALIZED VIEW bmp.end_of_ribs_mv TO bmp.end_of_ribs AS
SELECT
    toDate(fromUnixTimestamp64Nano(toInt64(timeReceivedNs))) AS date,
    now64(9) AS time_inserted_ns,
    fromUnixTimestamp64Nano(toInt64(timeReceivedNs)) AS time_received_ns,
    fromUnixTimestamp64Nano(toInt64(peerHeader.timestampNs)) AS time_bmp_header_ns,
    fromUnixTimestamp64Nano(toInt64(sessionStartNs)) AS session_start_ns,
    sequence,

    routerAddr AS router_addr,
    routerPort AS router_port,
    peerHeader.peerType AS peer_type,
    peerHeader.peerDistinguisher AS peer_distinguisher,
    CAST(peerHeader.peerAddr AS IPv6) AS peer_addr,
    peerHeader.peerBgpId AS peer_bgp_id,
    peerHeader.peerAsn AS peer_asn,
    peerHeader.isPostPolicy AS is_post_policy,
    peerHeader.isAdjRibOut AS is_adj_rib_out,

    endOfRib.afi AS afi,
    endOfRib.safi AS safi
FROM bmp.from_kafka
WHERE messageType = 0 AND endOfRib.afi != 0;

-- Peer Up and Peer Down notifications.
CREATE TABLE bmp.peer_events
(
    date                  Date,
    time_inserted_ns      DateTime64(9),
    time_received_ns      DateTime64(9),
    time_bmp_header_ns    DateTime64(9),
    session_start_ns      DateTime64(9),
    sequence              UInt64,

    router_addr           IPv6,
    router_port           UInt16,
    peer_type             UInt8,
    peer_distinguisher    UInt64,
    peer_addr             IPv6,
    peer_bgp_id           IPv4,
    peer_asn              UInt32,
    is_post_policy        Bool,
    is_adj_rib_out        Bool,

    event                 Enum8('down' = 2, 'up' = 3),

    -- Peer Down
    down_reason           UInt8,
    down_data             String,
    notification_code     UInt8,
    notification_subcode  UInt8,
    fsm_event_code        UInt16,

    -- Peer Up
    local_addr            IPv6,
    local_port            UInt16,
    remote_port           UInt16,
    sent_open             Tuple(version UInt8, asn UInt32, hold_time UInt16, bgp_id IPv4, capabilities Array(UInt8)),
    received_open         Tuple(version UInt8, asn UInt32, hold_time UInt16, bgp_id IPv4, capabilities Array(UInt8)),
    information           Array(Tuple(info_type UInt16, value String))
)
ENGINE = MergeTree()
ORDER BY (router_addr, peer_addr, time_received_ns)
PARTITION BY date
TTL date + INTERVAL 7 DAY DELETE;

CREATE MATERIALIZED VIEW bmp.peer_events_mv TO bmp.peer_events AS
SELECT
    toDate(fromUnixTimestamp64Nano(toInt64(timeReceivedNs))) AS date,
    now64(9) AS time_inserted_ns,
    fromUnixTimestamp64Nano(toInt64(timeReceivedNs)) AS time_received_ns,
    fromUnixTimestamp64Nano(toInt64(peerHeader.timestampNs)) AS time_bmp_header_ns,
    fromUnixTimestamp64Nano(toInt64(sessionStartNs)) AS session_start_ns,
    sequence,

    routerAddr AS router_addr,
    routerPort AS router_port,
    peerHeader.peerType AS peer_type,
    peerHeader.peerDistinguisher AS peer_distinguisher,
    CAST(peerHeader.peerAddr AS IPv6) AS peer_addr,
    peerHeader.peerBgpId AS peer_bgp_id,
    peerHeader.peerAsn AS peer_asn,
    peerHeader.isPostPolicy AS is_post_policy,
    peerHeader.isAdjRibOut AS is_adj_rib_out,

    CAST(messageType, 'Enum8(\'down\' = 2, \'up\' = 3)') AS event,

    peerDown.reason AS down_reason,
    peerDown.data AS down_data,
    peerDown.notificationCode AS notification_code,
    peerDown.notificationSubcode AS notification_subcode,
    peerDown.fsmEventCode AS fsm_event_code,

    CAST(peerUp.localAddr AS IPv6) AS local_addr,
    peerUp.localPort AS local_port,
    peerUp.remotePort AS remote_port,
    peerUp.sentOpen AS sent_open,
    peerUp.receivedOpen AS received_open,
    peerUp.information AS information
FROM bmp.from_kafka
WHERE messageType IN (2, 3);

-- Initiation and Termination messages: start and end of BMP sessions.
-- A synthetic Termination is emitted by Risotto when a session ends without a
-- Termination message; its information says why ('connection closed' or
-- 'collector shutdown'). Risotto also records when it starts (`collector_start`,
-- no router): sessions established before it are over, even without Termination
-- (e.g. when the collector crashed).
CREATE TABLE bmp.sessions
(
    date                Date,
    time_inserted_ns    DateTime64(9),
    time_received_ns    DateTime64(9),
    session_start_ns    DateTime64(9),
    sequence            UInt64,

    router_addr         IPv6,
    router_port         UInt16,
    synthetic           Bool,

    event               Enum16('initiation' = 4, 'termination' = 5, 'collector_start' = 255),
    termination_reason  UInt16,
    information         Array(Tuple(info_type UInt16, value String))
)
ENGINE = MergeTree()
ORDER BY (router_addr, time_received_ns)
PARTITION BY date
TTL date + INTERVAL 7 DAY DELETE;

CREATE MATERIALIZED VIEW bmp.sessions_mv TO bmp.sessions AS
SELECT
    toDate(fromUnixTimestamp64Nano(toInt64(timeReceivedNs))) AS date,
    now64(9) AS time_inserted_ns,
    fromUnixTimestamp64Nano(toInt64(timeReceivedNs)) AS time_received_ns,
    fromUnixTimestamp64Nano(toInt64(sessionStartNs)) AS session_start_ns,
    sequence,

    routerAddr AS router_addr,
    routerPort AS router_port,
    synthetic,

    CAST(messageType, 'Enum16(\'initiation\' = 4, \'termination\' = 5, \'collector_start\' = 255)') AS event,
    termination.reason AS termination_reason,
    if(messageType = 4, initiation.information, termination.information) AS information
FROM bmp.from_kafka
WHERE messageType IN (4, 5, 255);

-- Statistics Reports: one row per counter
CREATE TABLE bmp.stats
(
    date                Date,
    time_inserted_ns    DateTime64(9),
    time_received_ns    DateTime64(9),
    time_bmp_header_ns  DateTime64(9),
    session_start_ns    DateTime64(9),
    sequence            UInt64,

    router_addr         IPv6,
    router_port         UInt16,
    peer_type           UInt8,
    peer_distinguisher  UInt64,
    peer_addr           IPv6,
    peer_bgp_id         IPv4,
    peer_asn            UInt32,

    stat_type           UInt16,
    afi                 UInt16,
    safi                UInt8,
    value               UInt64
)
ENGINE = MergeTree()
ORDER BY (router_addr, peer_addr, stat_type, time_received_ns)
PARTITION BY date
TTL date + INTERVAL 7 DAY DELETE;

CREATE MATERIALIZED VIEW bmp.stats_mv TO bmp.stats AS
SELECT
    toDate(fromUnixTimestamp64Nano(toInt64(timeReceivedNs))) AS date,
    now64(9) AS time_inserted_ns,
    fromUnixTimestamp64Nano(toInt64(timeReceivedNs)) AS time_received_ns,
    fromUnixTimestamp64Nano(toInt64(peerHeader.timestampNs)) AS time_bmp_header_ns,
    fromUnixTimestamp64Nano(toInt64(sessionStartNs)) AS session_start_ns,
    sequence,

    routerAddr AS router_addr,
    routerPort AS router_port,
    peerHeader.peerType AS peer_type,
    peerHeader.peerDistinguisher AS peer_distinguisher,
    CAST(peerHeader.peerAddr AS IPv6) AS peer_addr,
    peerHeader.peerBgpId AS peer_bgp_id,
    peerHeader.peerAsn AS peer_asn,

    counter.statType AS stat_type,
    counter.afi AS afi,
    counter.safi AS safi,
    counter.value AS value
FROM bmp.from_kafka
ARRAY JOIN statsReport.counters AS counter
WHERE messageType = 1;
