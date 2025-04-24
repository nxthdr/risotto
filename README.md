<p align="center">
  <img src="https://nxthdr.dev/risotto/logo.png" height="256" width="256" alt="Project Logo" />
</p>

# Risotto

Risotto 😋 is a BGP collector that processes BMP protocol messages from routers and publishes updates to Kafka/Redpanda. This repository includes both the Risotto collector application and the Risotto library.

The collector application streams BGP updates to a Kafka topic, enabling downstream components to consume them. The library offers essential components for decoding BMP messages and generating BGP updates.

## State Management

Risotto maintains a state representing connected routers and their associated BGP peers and announced prefixes.
This state addresses two challenges when handling BMP data:
- **Duplicate announcements** from BMP session resets, where the router resends all active prefixes to the collector after a restart or connectivity issue.
- **Missing withdraws** when Peer Down notifications occur or when the collector is offline, resulting in incorrect BGP state downstream.

Duplicate announcements could, in theory, be handled by the database, but less data manipulation is better. Instead, Risotto checks each incoming update against its state. If the prefix is already present, the update is discarded.

Missing withdraws are generated synthetically when receiving Peer Down notifications for the prefixes already announced by the downed peer.

For persistance, Risotto dumps its state at specified interval, and fetches it at startup. Risotto is able to infer any missing withdraws that would have occured during downtime, from the initial peer up flow. This ensures the database remains accurate, even if the collector is restarted. On the other hand, a restart may result in duplicate announcements.
In other words, Risotto guaranties that the database is always in a consistent state, but may contain some duplicate announcements.

Conversely, Risotto can be configured to stream updates as is to the event pipeline without any state management. It is useful if there are other components downstream that can handle the state management.

## Quick Start

The easiest way to use Risotto with Docker.

* Create a `risotto.yml` configuration file

```yml
api:
  address: 0.0.0.0
  port: 3000

bmp:
  address: 0.0.0.0
  port: 4000

kafka:
  enable: true
  brokers: "kafka.example.com:9092"
  auth_protocol: PLAINTEXT
  topic: bgp-updates

state:
  enable: true
  path: /app/dump.json
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

An Prometheus HTTP endpoint is available at `http://localhost:3000/metrics` to monitor the collector's performance and statistics.

## Contributing

Refer to the Docker Compose [integration](./integration/) tests to try Risotto locally. The setup includes two routers sharing updates announced between them, and transmitting BMP messages to Risotto.
