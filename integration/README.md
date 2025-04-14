# Integration tests

The integration tests setup consists in two [gobgp](https://github.com/osrg/gobgp) routers peering together.
Check out the [routers](./config/gobgp/) and [Risotto](./config/risotto/) configuration.

In the [tests](./tests/) folder you can find the test cases that are run against the setup.

Both routers are configured to send BMP messages to the Risotto instance accessible at the bridge IP address `10.0.0.100`.
Risotto sends the BMP messages to a Redpanda instance.

The `bmp.from_kafka` table is using the ClickHouse [Kafka engine](https://clickhouse.com/docs/en/engines/table-engines/integrations/kafka) to fetch the results from Redpanda. The `bmp.updates` table is used to store the results.


## Usage

* Start the environment

```sh
docker compose up --build --force-recreate --renew-anon-volumes
```

* Check Risotto state:

```sh
curl -s http://127.0.0.1:3000 |jq
```

* Check Prometheus metrics

```sh
curl -s http://127.0.0.1:3000/metrics
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
```

* Finally, stop the environment:

```sh
docker compose down
```
