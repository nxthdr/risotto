CREATE DATABASE IF NOT EXISTS bmp;

-- Table to store raw updates from Kafka
CREATE TABLE bmp.from_kafka
(
    -- metadata
    timeReceivedNs     UInt64,
    timeBmpHeaderNs    UInt64,
    routerAddr         IPv6,
    routerPort         UInt16,
    peerAddr           IPv6,
    peerBgpId          IPv4,
    peerAsn            UInt32,
    prefixAddr         IPv6,
    prefixLen          UInt8,
    isPostPolicy       Bool,
    isAdjRibOut        Bool,
    announced          Bool,
    synthetic          Bool,

    -- BGP attributes
    origin             String,
    asPath             Array(UInt32),
    nextHop            IPv6,
    multiExitDisc      UInt32,
    localPreference    UInt32,
    onlyToCustomer     UInt32,
    atomicAggregate    Bool,
    aggregatorAsn      UInt32,
    aggregatorBgpId    UInt32,
    communities        Array(Tuple(UInt32, UInt16)),
    extendedCommunities Array(Tuple(UInt8, UInt8, String)),
    largeCommunities   Array(Tuple(UInt32, UInt32, UInt32)),
    originatorId       UInt32,
    clusterList        Array(UInt32),
    mpReachAfi         UInt16,
    mpReachSafi        UInt8,
    mpUnreachAfi       UInt16,
    mpUnreachSafi      UInt8
)
ENGINE = Kafka()
SETTINGS
    kafka_broker_list = '10.0.0.100:9092',
    kafka_topic_list = 'risotto-updates',
    kafka_group_name = 'clickhouse-risotto-group',
    kafka_format = 'CapnProto',
    kafka_schema = 'update.capnp:Update',
    kafka_max_rows_per_message = 1048576;

-- Table to store processed updates
CREATE TABLE bmp.updates
(
	-- metadata
    date               Date,
    time_inserted_ns   DateTime64(9),
    time_received_ns   DateTime64(9),
    time_bmp_header_ns DateTime64(9),

    router_addr        IPv6,
    router_port        UInt16,
    peer_addr          IPv6,
    peer_bgp_id        IPv4,
    peer_asn           UInt32,
    prefix_addr        IPv6,
    prefix_len         UInt8,
    is_post_policy     Bool,
    is_adj_rib_out     Bool,
    announced          Bool,
    synthetic          Bool,

    -- BGP attributes
    origin             String,
    as_path            Array(UInt32),
    next_hop           IPv6,
    multi_exit_disc    UInt32,
    local_preference   UInt32,
    only_to_customer   UInt32,
    atomic_aggregate   Bool,
    aggregator_asn     UInt32,
    aggregator_bgp_id  IPv4,
    communities        Array(Tuple(UInt32, UInt16)),
    extended_communities Array(Tuple(UInt8, UInt8, String)),
    large_communities  Array(Tuple(UInt32, UInt32, UInt32)),
    originator_id      IPv4,
    cluster_list       Array(UInt32),
    mp_reach_afi       UInt16,
    mp_reach_safi      UInt8,
    mp_unreach_afi     UInt16,
    mp_unreach_safi    UInt8
)
ENGINE = MergeTree()
ORDER BY (time_received_ns, router_addr, peer_addr, prefix_addr, prefix_len)
PARTITION BY date
TTL date + INTERVAL 7 DAY DELETE;

-- Materialized view to transform and load data from Kafka
CREATE MATERIALIZED VIEW bmp.from_kafka_mv
TO bmp.updates
AS
SELECT
	-- metadata
    toDate(toDateTime64(timeReceivedNs / 1000000000, 9)) AS date,
    now64(9) AS time_inserted_ns,
    toDateTime64(timeReceivedNs / 1000000000, 9) AS time_received_ns,
    toDateTime64(timeBmpHeaderNs / 1000000000, 9) AS time_bmp_header_ns,

    routerAddr AS router_addr,
    routerPort AS router_port,
    peerAddr AS peer_addr,
    peerBgpId AS peer_bgp_id,
    peerAsn AS peer_asn,
    prefixAddr AS prefix_addr,
    prefixLen AS prefix_len,
    isPostPolicy AS is_post_policy,
    isAdjRibOut AS is_adj_rib_out,
    announced,
    synthetic,

	-- BGP attributes
    origin,
    asPath AS as_path,
    nextHop AS next_hop,
    multiExitDisc AS multi_exit_disc,
    localPreference AS local_preference,
    onlyToCustomer AS only_to_customer,
    atomicAggregate AS atomic_aggregate,
    aggregatorAsn AS aggregator_asn,
    aggregatorBgpId AS aggregator_bgp_id,
    communities,
    extendedCommunities AS extended_communities,
    largeCommunities AS large_communities,
    originatorId AS originator_id,
    clusterList AS cluster_list,
    mpReachAfi AS mp_reach_afi,
    mpReachSafi AS mp_reach_safi,
    mpUnreachAfi AS mp_unreach_afi,
    mpUnreachSafi AS mp_unreach_safi
FROM bmp.from_kafka;
