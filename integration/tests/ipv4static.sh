docker exec -ti integration-gobgp10-1 gobgp global rib add 172.16.10.0/24 -a ipv4
docker exec -ti integration-gobgp20-1 gobgp global rib add 172.16.20.0/24 -a ipv4
