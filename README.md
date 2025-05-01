<p align="center">
  <img src="https://nxthdr.dev/risotto/logo.png" height="256" width="256" alt="Project Logo" />
</p>

# Risotto

Risotto 😋 is a BGP collector that processes BMP protocol messages from routers and publishes updates to Kafka/Redpanda. This repository includes both the Risotto collector application and the Risotto library.

The collector application streams BGP updates to a Kafka topic, enabling downstream components to consume them. The library offers essential components for decoding BMP messages and generating BGP updates.

## Quick Start

The easiest way to use Risotto with Docker. This command will output the help message and exit:

```bash
docker run ghcr.io/nxthdr/risotto:main --help
```

To run Risotto with Docker with the default parameters, you can use the following command:

```bash
docker run \
  -p 4000:4000 \
  -p 8080:8080 \
  ghcr.io/nxthdr/risotto:main
```

By default, Risotto listens on port `4000` for BMP messages.
Additionally, a Prometheus HTTP endpoint is available at `http://localhost:8080/metrics` to monitor the collector's performance and statistics.

## State Management

Risotto maintains a state representing connected routers and their associated BGP peers and announced prefixes. This state is dumped to a file at specified intervals.
This state addresses two challenges when handling BMP data:
- **Duplicate announcements** from BMP session resets, where the router resends all active prefixes to the collector after a restart or connectivity issue.
- **Missing withdraws** that occur when a BGP session goes down and the router is implemented not to send the withdraws messages, or when the collector experiences downtime. These scenarios can result in stale or inaccurate BGP state in downstream systems.

Risotto checks each incoming update against its state. If the prefix is already present, the update is not sent downstream. Missing withdraws are generated synthetically when receiving Peer Down notifications, if the withdraws have not been sent by the router, by using the prefixes stored in the state for this router and downed peer.

When the collector restarts, Risotto infers any missing withdraws from the initial Peer Up sequence, ensuring the database remains accurate despite downtime. However, any announcements received after the last saved state may be replayed. In short, Risotto guarantees a consistent database state, though it may contain some duplicate announcements following a restart.

Conversely, Risotto can be configured to stream updates to the event pipeline as is by disabling the state usage.

## Contributing

Refer to the Docker Compose [integration](./integration/) tests to try Risotto locally. The setup includes BIRD and GoBGP routers announcing BGP updates between them, and transmitting BMP messages to Risotto.
