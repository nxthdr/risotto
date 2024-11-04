# Risotto 

> [!WARNING]
> Risotto is currently in early-stage development.

Risotto 😋 is a BGP updates collector that gathers BGP updates from routers via BMP and streams them into an events pipeline, such as Apache Kafka or Redpanda.

Risotto maintains an internal state representing connected routers and their associated BGP data (peers, announced prefixes, etc.).  
This state helps (1) prevent duplicate updates from being emitted during BMP session resets and (2) enables the generation of synthetic withdrawn prefix updates in the event of a Peer Notification Down.  

An HTTP API provides the ability to retrieve the number of announced and withdrawn prefixes from each peer.

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
  enabled: true
  host: kafka.example.com
  port: 9092
  topic: bgp-updates
```

* Run your Docker container

```bash
docker run -v $(PWD)/risotto.yml:/config/risotto.yml ghcr.io/nxthdr/risotto:main -c /config/risotto.yml
```

## Contributing

Check the [testbed](./testbed/) to try Risotto locally along with two [Bird](https://bird.network.cz/) instances using Docker Compose.