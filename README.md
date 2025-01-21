# Risotto

> [!WARNING]
> Currently in early-stage development.

Risotto 😋 is a BGP updates collector that gathers BGP updates from routers via BMP and streams them into an events pipeline, such as Apache Kafka or Redpanda.

## State Management

Risotto maintains a state representing connected routers and their associated BGP peers and announced prefixes.
This state addresses two challenges when handling BMP data:
- **Duplicate announcements** from BMP session resets, where the router resends all active prefixes to the collector after a restart or connectivity issue.
- **Missing withdraws** when Peer Down notifications occur or when the collector is offline, resulting in incorrect BGP state downstream.

Duplicate announcements could, in theory, be handled by the database, but less data manipulation is better. Instead, Risotto checks each incoming update against its state. If the prefix is already present, the update is discarded.

For Peer Down notifications, Risotto leverages its state to generate synthetic withdraws for the prefixes announced by the downed peer.

For persistance, Risotto dumps its state at specified interval, and fetches it at startup. Risotto is able to infer any missing withdraws that would have occured during downtime, from the initial peer up flow. This ensures the database remains accurate, even if the collector is restarted. On the other hand, a restart may result in duplicate announcements.
In other words, Risotto guaranties that the database is always in a consistent state, but may contain some duplicate announcements.

Conversely, Risotto can be configured to stream updates as is to the event pipeline without any state management. It is useful if there are other components downstream that can handle the state management.

## Quick Start

The easiest way to use risotto is using Docker.

* Create a `risotto.yml` configuration file

```yml
api:
  address: 0.0.0.0
  port: 3000

bmp:
  address: 0.0.0.0
  port: 4000

kafka:
  host: kafka.example.com
  port: 9092
  topic: bgp-updates

state:
  enable: true
  path: /app/dump.txt
  save_interval: 10
```

* Run your Docker container

```bash
docker run \
    -v $(PWD)/risotto.yml:/config/risotto.yml \
    -p 3000:3000 \
    -p 4000:4000 \
    ghcr.io/nxthdr/risotto:main -c /config/risotto.yml
```

An HTTP API provides the ability to retrieve the current state of the collector, including the list of connected routers and their BGP data.

```sh
curl -s http://localhost:3000
```

## Contributing

Refer to the Docker Compose [testbed](./testbed/) to try Risotto locally. The setup includes two [Bird](https://bird.network.cz/) routers that connect to Risotto, sharing updates announced between them.