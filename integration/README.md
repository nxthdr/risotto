# Integration tests

This setup provides an environment for integration testing of Risotto's BMP message processing and Kafka production capabilities.

**Components:**

*   **BGP Routers:**
    *   Two [GoBGP](https://github.com/osrg/gobgp) instances configured to peer with each other.
    *   Two [BIRD](https://bird.network.cz/) 3.3.3 instances configured to peer and exchange routes with each other, monitoring their pre-policy and post-policy Adj-RIB-In.
*   **Risotto:** The instance under test, listening for BMP messages.
*   **Redpanda:** A Kafka instance serving as the message broker for the BMP messages produced by Risotto.
*   **ClickHouse:** A database used to consume and store the messages from Redpanda.

**Data Flow:**

1.  All four routers (GoBGP and BIRD) send their BMP messages to the Risotto instance (`10.0.0.100`).
2.  Risotto decodes these BMP messages and produces them to the `risotto` topic in Redpanda.
3.  ClickHouse uses a table based on the [Kafka engine](https://clickhouse.com/docs/en/engines/table-engines/integrations/kafka) (`bmp.from_kafka`) to read messages from the Redpanda topic. Materialized views then dispatch them by message type into `bmp.updates` (Route Monitoring), `bmp.peer_events` (Peer Up and Peer Down), `bmp.sessions` (Initiation and Termination) and `bmp.stats` (Statistics Report).

**Test Scenarios:**

*   The initial configuration tests basic BMP message reception (session establishment, etc.) from GoBGP and the processing of route exchanges from BIRD.
*   Additional test scenarios, located in the [`./tests/`](./tests/) directory, can be applied to dynamically modify the GoBGP routers' configuration (e.g., announcing or withdrawing routes) to test specific Risotto use cases.

## Usage

* Start the environment

```sh
docker compose up -d --build --force-recreate --renew-anon-volumes
```

* Check Prometheus metrics

```sh
curl -s http://127.0.0.1:8080/metrics
```

* Interact with the gobgp routers:

```sh
docker exec -ti integration-gobgp10-1 gobgp neighbor
```

* Interact with redpanda container:

```sh
docker exec -ti integration-redpanda-1 rpk group describe clickhouse-risotto-group
```

* Check the ClickHouse tables:

```sh
docker exec -ti integration-clickhouse-1 clickhouse-client --query "SELECT * FROM bmp.updates"
docker exec -ti integration-clickhouse-1 clickhouse-client --query "SELECT * FROM bmp.peer_events"
docker exec -ti integration-clickhouse-1 clickhouse-client --query "SELECT * FROM bmp.sessions"
```

* Check the initial table dump is flagged, after restarting Risotto so that routers dump their tables again:

```sh
docker compose restart risotto
docker exec -ti integration-clickhouse-1 clickhouse-client --query "SELECT initial_dump, count() FROM bmp.updates GROUP BY initial_dump"
```

* Finally, stop the environment:

```sh
docker compose down
```
