-- Routing state reconstructed from the BMP messages, as keyframes + deltas.
--
-- Semantics:
-- - A Peer Down withdraws all the routes of the peer (RFC 7854, section 4.9).
-- - The end of a BMP session is a loss of observability: the routes are kept,
--   flagged as stale. A session has ended when it has a Termination (real or
--   synthetic), or when it was established before the latest collector start.
--   Stale routes are dropped when their session ended more than 7 days ago.
-- - A route is current when it was received after its peer's latest Peer Up, in
--   its router's latest session.
-- - After a new Peer Up (or a new session of the router), the routes received
--   before it stay visible, flagged as refreshing, until the new table dump is
--   complete (End-of-RIB): routes that are not re-sent by then were withdrawn.
--   - A peer that came up again: until its End-of-RIB for the route's RIB view
--     (pre/post-policy, Adj-RIB-In/Out) and address family.
--   - A peer without Peer Up in the router's new session: until every peer that
--     came up in the new session sent an End-of-RIB.
--
-- Storage:
-- - Deltas are the bmp.updates, bmp.peer_events, bmp.sessions and
--   bmp.end_of_ribs logs (partitioned by day, with TTL).
-- - Keyframes (bmp.rib_keyframes) hold the state of each router at a point in
--   time: its routes, the last event of each of its peers, and its latest
--   session. They are rebuilt periodically from the previous keyframe and the
--   deltas since. Current state = latest complete keyframe + deltas since.
-- - Nothing is updated or deleted: withdrawn routes and dead epochs are not
--   written to the next keyframe, and old keyframes expire with their partition.
--
-- Retention requirements: the logs must be kept longer than the keyframe
-- interval plus the delta margin, and keyframes for at least two intervals.
-- bmp.rib_keyframe_age must be monitored: a router whose keyframes stop being
-- built for longer than the keyframe retention loses its long-lived routes.

-- State of each router at keyframe time. Every row holds the number of rows of its
-- router keyframe (row_count): only complete keyframes are used, so an interrupted
-- keyframe build never loses state.
CREATE TABLE bmp.rib_keyframes
(
    keyframe_ns          DateTime64(9),
    row_count            UInt64,
    -- route: a visible route
    -- peer: last Peer Up / Peer Down of a peer (peer_up, version)
    -- session: latest BMP session of the router (session_start_ns, ended,
    --          time_received_ns = when it ended)
    kind                 Enum8('route' = 1, 'peer' = 2, 'session' = 3),

    -- Version of the row: the BMP session and the position of the event in it
    session_start_ns     DateTime64(9),
    sequence             UInt64,
    peer_up              Bool,
    ended                Bool,
    time_received_ns     DateTime64(9),

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
    afi                  UInt16,
    safi                 UInt8,

    origin               LowCardinality(String),
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

    -- Number of rows per keyframe, maintained with each part: the completeness
    -- check (bmp.rib_latest_keyframes) reads it instead of counting every row
    PROJECTION keyframe_counts
    (
        SELECT router_addr, keyframe_ns, count(), any(row_count)
        GROUP BY router_addr, keyframe_ns
    )
)
ENGINE = MergeTree()
PARTITION BY toDate(keyframe_ns)
ORDER BY (router_addr, keyframe_ns, kind, peer_addr, peer_distinguisher, is_post_policy,
          is_adj_rib_out, prefix_addr, prefix_len, path_id)
TTL toDate(keyframe_ns) + INTERVAL 3 DAY DELETE;

-- Latest complete keyframe of each router.
CREATE VIEW bmp.rib_latest_keyframes AS
SELECT router_addr, max(keyframe_ns) AS keyframe_ns
FROM (
    SELECT router_addr, keyframe_ns
    FROM bmp.rib_keyframes
    GROUP BY router_addr, keyframe_ns
    HAVING count() = any(row_count)
)
GROUP BY router_addr;

-- In the views below, deltas are read from `delta_margin` before the keyframe,
-- so that rows inserted late (e.g. Kafka lag) are not missed. Reading a row twice
-- is harmless: every row has a version (session_start_ns, sequence), and only the
-- latest one is kept.

-- Latest session of each router, whether it has ended, and when.
CREATE VIEW bmp.router_state AS
WITH
    toIntervalMinute(10) AS delta_margin,
    (SELECT max(time_received_ns) FROM bmp.sessions WHERE event = 'collector_start') AS collector_start_ns
SELECT
    router_addr,
    session_start_ns,
    toBool(max(is_ended) OR session_start_ns < collector_start_ns) AS ended,
    if(max(is_ended), minIf(end_ns, is_ended),
       if(session_start_ns < collector_start_ns, collector_start_ns, toDateTime64(0, 9))) AS ended_ns
FROM (
    SELECT router_addr, session_start_ns, ended AS is_ended, time_received_ns AS end_ns
    FROM bmp.rib_keyframes
    WHERE kind = 'session' AND (router_addr, keyframe_ns) IN (SELECT * FROM bmp.rib_latest_keyframes)
    UNION ALL
    SELECT s.router_addr, s.session_start_ns, s.event = 'termination', s.time_received_ns
    FROM bmp.sessions AS s
    LEFT JOIN bmp.rib_latest_keyframes AS k ON s.router_addr = k.router_addr
    WHERE s.event != 'collector_start' AND s.time_inserted_ns > k.keyframe_ns - delta_margin
)
GROUP BY router_addr, session_start_ns
ORDER BY session_start_ns DESC
LIMIT 1 BY router_addr;

-- Last Peer Up / Peer Down of each peer, and its last Peer Down if retained.
CREATE VIEW bmp.peer_state AS
WITH toIntervalMinute(10) AS delta_margin
SELECT
    router_addr, peer_addr, peer_distinguisher,
    argMax(is_up, (session_start_ns, sequence)) AS peer_up,
    max((session_start_ns, sequence)) AS version,
    maxIf((session_start_ns, sequence), NOT is_up) AS last_down_version
FROM (
    SELECT router_addr, peer_addr, peer_distinguisher, peer_up AS is_up, session_start_ns, sequence
    FROM bmp.rib_keyframes
    WHERE kind = 'peer' AND (router_addr, keyframe_ns) IN (SELECT * FROM bmp.rib_latest_keyframes)
    UNION ALL
    SELECT e.router_addr, e.peer_addr, e.peer_distinguisher, e.event = 'up', e.session_start_ns, e.sequence
    FROM bmp.peer_events AS e
    LEFT JOIN bmp.rib_latest_keyframes AS k ON e.router_addr = k.router_addr
    WHERE e.time_inserted_ns > k.keyframe_ns - delta_margin
)
GROUP BY router_addr, peer_addr, peer_distinguisher;

-- Routes: visible routes, current or refreshing.
CREATE VIEW bmp.rib_current AS
WITH
    toIntervalMinute(10) AS delta_margin,
    toIntervalDay(7) AS stale_retention,
    -- Candidate rows: keyframe routes, and updates since the keyframe
    candidates AS (
        SELECT
            session_start_ns, sequence, true AS announced, time_received_ns,
            router_addr, router_port, peer_type, peer_distinguisher, peer_addr, peer_bgp_id, peer_asn,
            is_post_policy, is_adj_rib_out, prefix_addr, prefix_len, path_id, afi, safi,
            origin, as_path, next_hop, multi_exit_disc, local_preference, only_to_customer,
            atomic_aggregate, aggregator_asn, aggregator_bgp_id, communities, extended_communities,
            large_communities, originator_id, cluster_list
        FROM bmp.rib_keyframes
        WHERE kind = 'route' AND (router_addr, keyframe_ns) IN (SELECT * FROM bmp.rib_latest_keyframes)
        UNION ALL
        SELECT
            u.session_start_ns, u.sequence, u.announced, u.time_received_ns,
            u.router_addr, u.router_port, u.peer_type, u.peer_distinguisher, u.peer_addr, u.peer_bgp_id, u.peer_asn,
            u.is_post_policy, u.is_adj_rib_out, u.prefix_addr, u.prefix_len, u.path_id, u.afi, u.safi,
            u.origin, u.as_path, u.next_hop, u.multi_exit_disc, u.local_preference, u.only_to_customer,
            u.atomic_aggregate, u.aggregator_asn, u.aggregator_bgp_id, u.communities, u.extended_communities,
            u.large_communities, u.originator_id, u.cluster_list
        FROM bmp.updates AS u
        LEFT JOIN bmp.rib_latest_keyframes AS k ON u.router_addr = k.router_addr
        -- Routers without keyframe get the default keyframe_ns (1970): all updates
        WHERE u.time_inserted_ns > k.keyframe_ns - delta_margin
    ),
    -- Latest version of each route
    routes AS (
        SELECT
            router_addr, peer_addr, peer_distinguisher, is_post_policy, is_adj_rib_out,
            prefix_addr, prefix_len, path_id,
            argMax(
                (session_start_ns, sequence, announced, time_received_ns,
                 router_port, peer_type, peer_bgp_id, peer_asn, afi, safi,
                 origin, as_path, next_hop, multi_exit_disc, local_preference, only_to_customer,
                 atomic_aggregate, aggregator_asn, aggregator_bgp_id, communities, extended_communities,
                 large_communities, originator_id, cluster_list),
                (session_start_ns, sequence)
            ) AS r
        FROM candidates
        GROUP BY router_addr, peer_addr, peer_distinguisher, is_post_policy, is_adj_rib_out,
                 prefix_addr, prefix_len, path_id
    ),
    -- Latest End-of-RIB of each peer, RIB view and family (since the keyframe)
    end_of_ribs AS (
        SELECT
            e.router_addr, e.peer_addr, e.peer_distinguisher, e.is_post_policy, e.is_adj_rib_out,
            e.afi, e.safi,
            max((e.session_start_ns, e.sequence)) AS version
        FROM bmp.end_of_ribs AS e
        LEFT JOIN bmp.rib_latest_keyframes AS k ON e.router_addr = k.router_addr
        WHERE e.time_inserted_ns > k.keyframe_ns - delta_margin
        GROUP BY ALL
    ),
    -- Routers whose latest session still has a table dump in progress: a peer
    -- that came up in the session has not sent any End-of-RIB since its Peer Up
    dumping_routers AS (
        SELECT DISTINCT p.router_addr AS router_addr, true AS dumping
        FROM bmp.peer_state AS p
        INNER JOIN bmp.router_state AS s ON p.router_addr = s.router_addr
        LEFT JOIN (
            SELECT router_addr, peer_addr, peer_distinguisher, max(version) AS version
            FROM end_of_ribs
            GROUP BY ALL
        ) AS e
            ON p.router_addr = e.router_addr
            AND p.peer_addr = e.peer_addr
            AND p.peer_distinguisher = e.peer_distinguisher
        WHERE p.peer_up AND p.version.1 = s.session_start_ns AND e.version <= p.version
    ),
    visible AS (
        SELECT
            routes.router_addr AS router_addr,
            routes.peer_addr AS peer_addr,
            routes.peer_distinguisher AS peer_distinguisher,
            routes.is_post_policy AS is_post_policy,
            routes.is_adj_rib_out AS is_adj_rib_out,
            routes.prefix_addr AS prefix_addr,
            routes.prefix_len AS prefix_len,
            routes.path_id AS path_id,
            routes.r AS r,
            s.ended AS stale,
            s.ended_ns AS ended_ns,
            -- Received after the peer's latest Peer Up, in the latest session
            r.1 = s.session_start_ns AND p.version < (r.1, r.2) AS current,
            -- The peer came up again in the latest session, no Peer Down since the
            -- route, and the new dump of the route's view and family is not complete
            p.version.1 = s.session_start_ns AND p.version > (r.1, r.2)
                AND p.last_down_version < (r.1, r.2) AND e.version <= p.version AS refreshing_peer,
            -- The peer did not come up in the router's latest session (yet), the
            -- route was current in its session, and the session dump is in progress
            p.version.1 < s.session_start_ns AND p.version < (r.1, r.2)
                AND d.dumping AS refreshing_router
        FROM routes
        INNER JOIN bmp.peer_state AS p
            ON routes.router_addr = p.router_addr
            AND routes.peer_addr = p.peer_addr
            AND routes.peer_distinguisher = p.peer_distinguisher
        INNER JOIN bmp.router_state AS s ON routes.router_addr = s.router_addr
        LEFT JOIN end_of_ribs AS e
            ON routes.router_addr = e.router_addr
            AND routes.peer_addr = e.peer_addr
            AND routes.peer_distinguisher = e.peer_distinguisher
            AND routes.is_post_policy = e.is_post_policy
            AND routes.is_adj_rib_out = e.is_adj_rib_out
            AND r.9 = e.afi
            AND r.10 = e.safi
        LEFT JOIN dumping_routers AS d ON routes.router_addr = d.router_addr
        WHERE r.3 AND p.peer_up
    )
SELECT
    r.1 AS session_start_ns,
    r.2 AS sequence,
    stale,
    toBool(NOT current) AS refreshing,
    r.4 AS time_received_ns,
    router_addr,
    r.5 AS router_port,
    r.6 AS peer_type,
    peer_distinguisher,
    peer_addr,
    r.7 AS peer_bgp_id,
    r.8 AS peer_asn,
    is_post_policy,
    is_adj_rib_out,
    prefix_addr,
    prefix_len,
    path_id,
    r.9 AS afi,
    r.10 AS safi,
    r.11 AS origin,
    r.12 AS as_path,
    r.13 AS next_hop,
    r.14 AS multi_exit_disc,
    r.15 AS local_preference,
    r.16 AS only_to_customer,
    r.17 AS atomic_aggregate,
    r.18 AS aggregator_asn,
    r.19 AS aggregator_bgp_id,
    r.20 AS communities,
    r.21 AS extended_communities,
    r.22 AS large_communities,
    r.23 AS originator_id,
    r.24 AS cluster_list
FROM visible
WHERE (current OR refreshing_peer OR refreshing_router)
    -- Stale routes are dropped when their session ended long ago
    AND NOT (stale AND ended_ns < now64(9) - stale_retention);

-- Keyframe of every router: routes, peers and session. Used by the refreshable
-- materialized view below (and by tests).
CREATE VIEW bmp.rib_keyframe_source AS
SELECT
    count() OVER (PARTITION BY router_addr) AS row_count,
    *
FROM (
    SELECT
        'route' AS kind, session_start_ns, sequence, false AS peer_up, stale AS ended, time_received_ns,
        router_addr, router_port, peer_type, peer_distinguisher, peer_addr, peer_bgp_id, peer_asn,
        is_post_policy, is_adj_rib_out, prefix_addr, prefix_len, path_id, afi, safi,
        origin, as_path, next_hop, multi_exit_disc, local_preference, only_to_customer,
        atomic_aggregate, aggregator_asn, aggregator_bgp_id, communities, extended_communities,
        large_communities, originator_id, cluster_list
    FROM bmp.rib_current
    UNION ALL
    SELECT
        'peer', version.1, version.2, peer_up, false, toDateTime64(0, 9),
        router_addr, 0, 0, peer_distinguisher, peer_addr, toIPv4('0.0.0.0'), 0,
        false, false, toIPv6('::'), 0, 0, 0, 0,
        '', [], toIPv6('::'), 0, 0, 0,
        false, 0, toIPv4('0.0.0.0'), [], [],
        [], toIPv4('0.0.0.0'), []
    FROM bmp.peer_state
    UNION ALL
    SELECT
        'session', session_start_ns, 0, false, ended, ended_ns,
        router_addr, 0, 0, 0, toIPv6('::'), toIPv4('0.0.0.0'), 0,
        false, false, toIPv6('::'), 0, 0, 0, 0,
        '', [], toIPv6('::'), 0, 0, 0,
        false, 0, toIPv4('0.0.0.0'), [], [],
        [], toIPv4('0.0.0.0'), []
    FROM bmp.router_state
);

-- Keyframes: every hour, rebuild the state of one 24th of the routers, so that
-- every router gets a keyframe every day, and the load is spread over the day.
-- Each run has its own keyframe_ns, so that a re-run in the same hour (retry,
-- manual SYSTEM REFRESH) writes a new keyframe instead of duplicating rows.
CREATE MATERIALIZED VIEW bmp.rib_keyframes_mv
REFRESH EVERY 1 HOUR APPEND TO bmp.rib_keyframes AS
WITH
    now64(9) AS kf,
    24 AS slices
SELECT kf AS keyframe_ns, *
FROM bmp.rib_keyframe_source
WHERE cityHash64(router_addr) % slices = toHour(kf) % slices;

-- Age of the latest complete keyframe of each router, to monitor: it must stay
-- below the keyframe retention (3 days), and should stay below about 1 day.
-- A router without keyframe counts from the start of its latest session.
CREATE VIEW bmp.rib_keyframe_age AS
SELECT
    s.router_addr AS router_addr,
    k.keyframe_ns AS keyframe_ns,
    dateDiff('second', if(k.keyframe_ns > 0, k.keyframe_ns, s.session_start_ns), now64(9)) AS age_seconds
FROM bmp.router_state AS s
LEFT JOIN bmp.rib_latest_keyframes AS k ON s.router_addr = k.router_addr;
