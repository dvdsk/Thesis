#!/usr/bin/env bash
set -e

source $(dirname "$0")/../deploy.sh

# check which run this will be
run_numb=$((ls *.last_run 2>/dev/null || echo "-1.run_numb") \
	| sort \
	| head -n 1 \
	| cut -d " " -f 2 \
	| cut -d "." -f 1)

run_numb=$(($run_numb + 1))
rm *.last_run >& /dev/null || true
touch $run_numb.last_run

# deploy cluster
client_port=50978
nserver_nodes=3
bin="meta-server"
args="\
	--cluster-size $nserver_nodes \
	--tracing-endpoint fs1.cm.cluster \
	--run-numb $run_numb \
	deploy \
	--client-port $client_port \
	--control-port 50972"

res=$(deploy $nserver_nodes $bin $args)
echo $res
resv_numb1=$(echo $res | cut -d " " -f 1)
server_nodes=$(echo $res | cut -d " " -f 2-)

# deploy clients
client_nodes=1
server_ips=$(to_infiniband_ips $server_nodes)

echo giving fs-cluster time to find eachother and elect a leader
sleep 1

args="$client_port $server_ips"
res=$(deploy $client_nodes "bench_mkdir" $args)
resv_numb2=$(echo $res | cut -d " " -f 1)
attach cluster_b

cleanup $resv_numb1 cluster_a
cleanup $resv_numb2 cluster_b
