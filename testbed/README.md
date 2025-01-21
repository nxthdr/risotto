# Testbed

Docker Compose setup to facilitate the tests of Risotto.

The testbed consists in two [Bird](https://bird.network.cz/) routers exchanging static routes via a eBGP session.
Check out the [routers](./config/bird/) and [Risotto](./config/risotto/) configuration.

Both routers are configured to send BMP messages to the Risotto instance accessible at the bridge IP address `10.0.0.100`.
Risotto sends the BMP messages to a Redpanda instance.

The `risotto.from_kafka` table is using the ClickHouse [Kafka engine](https://clickhouse.com/docs/en/engines/table-engines/integrations/kafka) to fetch the results from Redpanda. The `risotto.updates` table is used to store the results.


## Usage

* Start the testbed

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

* You can exec into one router container to interact with bird

```sh
docker exec -ti testbed-bird_10-1 /bin/bash
birdc
```

* You can exec into redpanda container to interact with it

```sh
docker exec -ti testbed-redpanda-1 /bin/bash
rpk topic consume bgp-updates
```