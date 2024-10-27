# Testbed

Docker Compose setup to facilitate the tests of Risotto.

The testbed consists in two [bird](https://bird.network.cz/) routers exchanging static routes via a eBGP session.  
You can find the bird configuration of the routers [here](./config/).  
The two routers are configured to send BMP messages to a Risotto instance accessible via `10.0.0.100` bridge IP address.

## Usage

Start the testbed:

```sh
docker compose up --build --force-recreate
```

Check Risotto state: 

```sh
curl -s http://10.0.0.100:3000 |jq
```

You can exec into one router container to interact with bird:

```sh
docker exec -ti testbed-bird_10-1 /bin/bash
birdc
```