CREATE DATABASE IF NOT EXISTS bmp;
CREATE TABLE bmp.from_kafka
(
	timestamp DateTime64,
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
	origin String,
	path Array(UInt32),
	communities Array(Tuple(UInt32, UInt16)),
	synthetic bool,
)
ENGINE = Kafka()
SETTINGS
    kafka_broker_list = '10.0.0.100:9092',
    kafka_topic_list = 'risotto-updates',
    kafka_group_name = 'clickhouse-risotto-group',
    kafka_format = 'CSV',
    kafka_max_rows_per_message = 1048576;

CREATE TABLE bmp.updates
(
	timestamp DateTime64,
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
	origin String,
	path Array(UInt32),
	communities Array(Tuple(UInt32, UInt16)),
	synthetic bool,
)
ENGINE = MergeTree()
ORDER BY (timestamp, router_addr, peer_addr, prefix_addr, prefix_len)
TTL toDateTime(timestamp) + INTERVAL 7 DAY DELETE;

CREATE MATERIALIZED VIEW bmp.from_kafka_mv TO bmp.updates
AS SELECT * FROM bmp.from_kafka;
