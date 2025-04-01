# Integration tests

The integration tests setup consists in two [gobgp](https://github.com/osrg/gobgp) routers peering together.
Check out the [routers](./config/gobgp/) and [Risotto](./config/risotto/) configuration.

In the [tests](./tests/) folder you can find the test cases that are run against the setup.

Both routers are configured to send BMP messages to the Risotto instance accessible at the bridge IP address `10.0.0.100`.
Risotto sends the BMP messages to a Redpanda instance.

The `risotto.from_kafka` table is using the ClickHouse [Kafka engine](https://clickhouse.com/docs/en/engines/table-engines/integrations/kafka) to fetch the results from Redpanda. The `risotto.updates` table is used to store the results.


## Usage

* Start the environment

```sh
docker compose up --build --force-recreate --renew-anon-volumes
```

* You can check Risotto state

```sh
curl -s http://127.0.0.1:3000 |jq
```

* Or get Prometheus metrics

```sh
curl -s http://127.0.0.1:3000/metrics
```

* You can interact with the gobgp routers this way:

```sh
docker exec -ti integration-gobgp10-1 gobgp neighbor
```

* You can exec into redpanda container to interact with it

```sh
docker exec -ti integration-redpanda-1 /bin/bash
rpk topic consume bgp-updates
```

* Stop the environment

```sh
docker compose down
```
