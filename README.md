# Risotto 

> [!WARNING]
> Risotto is currently in early-stage development.

Risotto 😋 is a RIS server designed to collect BGP updates from routers via BMP, and output them into an events pipeline (e.g., Apache Kafka).

Risotto maintains an internal state representing connected routers and their associated BGP data (peers, received prefixes, etc.), accessible via an API.  
This state helps prevent duplicate updates from being output during BMP session resets. Also, it allows to emit synthetic withdrawn prefixes updates in case of Peer Notification Down.

## Try it out

Check the [testbed](./testbed/) to try Risotto locally using Docker Compose. 