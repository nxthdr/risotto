<p align="center">
  <img src="https://nxthdr.dev/risotto/logo.png" height="256" width="256" alt="Project Logo" />
</p>

# Risotto

Risotto 😋 is a high-performance collector that processes BMP messages from routers and publishes updates to Kafka/Redpanda. This repository includes both the Risotto collector application and the Risotto library.

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

## Data Model

Risotto is stateless: it decodes each BMP message ([RFC 7854](https://datatracker.ietf.org/doc/html/rfc7854)) and publishes it to Kafka, without deduplicating or curating anything. Every message type is forwarded (Route Monitoring, Peer Up, Peer Down, Initiation, Termination and Statistics Report), so downstream consumers can reconstruct routing state themselves, for instance in the database.

All messages are published to a single Kafka topic, using the [`message.capnp`](./risotto/schemas/message.capnp) Cap'n Proto schema: each message carries common fields (router, session, message type) and the body of its type. Route Monitoring messages are flattened into one message per prefix.

A few fields help consumers make sense of the stream:

- **Session**: a BMP session (one TCP connection from a router) is identified by the router address, router port and `sessionStartNs`. Within a session, `sequence` numbers every message from 0: a gap means messages were lost between Risotto and the consumer.
- **Synthetic Termination**: when a session ends without a Termination message (router crash, network failure), Risotto emits a Termination with `synthetic = true`, so that consumers know the session ended. The end of a session is a loss of observability, not a Peer Down: the router's BGP sessions may still be up, and only Peer Down messages withdraw a peer's routes ([RFC 7854, section 4.9](https://datatracker.ietf.org/doc/html/rfc7854#section-4.9)).
- **Initial dump**: after a Peer Up, routers dump their whole table, then send an End-of-RIB marker ([RFC 4724](https://datatracker.ietf.org/doc/html/rfc4724)) per RIB view and address family. Updates received before the End-of-RIB are flagged with `initialDump = true`: they are not routing changes, and can be filtered out to study BGP dynamics. End-of-RIB markers themselves are not forwarded.

The library (`risotto-lib`) exposes the same model: `BmpSession` decodes the raw messages of a BMP session into `Event`s, which you can serialize and route as you wish.

## Contributing

Refer to the Docker Compose [integration](./integration/) tests to try Risotto locally. The setup includes BIRD and GoBGP routers announcing BGP updates between them, and transmitting BMP messages to Risotto.
