CREATE DATABASE IF NOT EXISTS bmp;
CREATE TABLE bmp.from_kafka
(
	timeReceivedNs UInt64,
	timeBmpHeaderNs UInt64,
	routerAddr IPv6,
	routerPort UInt32,
	peerAddr IPv6,
	peerBgpId IPv4,
	peerAsn UInt32,
	prefixAddr IPv6,
	prefixLen UInt8,
	isPostPolicy bool,
	isAdjRibOut bool,
	announced bool,
	nextHop IPv6,
	origin String,
	path Array(UInt32),
	localPreference UInt32,
	med UInt32,
	communities Array(Tuple(UInt32, UInt16)),
	synthetic bool,
)
ENGINE = Kafka()
SETTINGS
    kafka_broker_list = '10.0.0.100:9092',
    kafka_topic_list = 'risotto-updates',
    kafka_group_name = 'clickhouse-risotto-group',
    kafka_format = 'CapnProto',
    kafka_schema = 'update.capnp:Update',
    kafka_max_rows_per_message = 1048576;

CREATE TABLE bmp.updates
(
	date Date,
	time_inserted_ns DateTime64(9),
	time_received_ns DateTime64(9),
	time_bmp_header_ns DateTime64(9),
	router_addr IPv6,
	router_port UInt32,
	peer_addr IPv6,
	peer_bgp_id IPv4,
	peer_asn UInt32,
	prefix_addr IPv6,
	prefix_len UInt8,
	is_post_policy bool,
	is_adj_rib_out bool,
	announced bool,
	next_hop IPv6,
	origin String,
	path Array(UInt32),
	local_preference UInt32,
	med UInt32,
	communities Array(Tuple(UInt32, UInt16)),
	synthetic bool,
)
ENGINE = MergeTree()
ORDER BY (time_received_ns, router_addr, peer_addr, prefix_addr, prefix_len)
PARTITION BY date
TTL date + INTERVAL 7 DAY DELETE;

CREATE MATERIALIZED VIEW bmp.from_kafka_mv TO bmp.updates
AS SELECT
	toDate(timeReceivedNs) AS date,
	now() AS time_inserted_ns,
	toDateTime64(timeReceivedNs/1000000000, 9) AS time_received_ns,
	toDateTime64(timeBmpHeaderNs/1000000000, 9) AS time_bmp_header_ns,
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
	nextHop as next_hop,
	origin,
	path,
	localPreference AS local_preference,
	med,
	communities,
	synthetic
FROM bmp.from_kafka;
