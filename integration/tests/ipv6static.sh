docker exec -ti integration-gobgp10-1 gobgp global rib add fd49:1020:cafe::/48 -a ipv6
docker exec -ti integration-gobgp20-1 gobgp global rib add fd49:1020:beef::/48 -a ipv6
