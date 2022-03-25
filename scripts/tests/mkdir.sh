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
	--client-port 50975 \
	--control-port $client_port"

res=$(deploy $numb_nodes $bin $args)
echo $res
resv_numb1=$(echo $res | cut -d " " -f 1)
nodes=$(echo $res | cut -d " " -f 2-)
ips=$(to_infiniband_ips $nodes)

echo giving fs-cluster time to find eachother and elect a leader
sleep 1

args="$client_port $ips"
res=$(deploy 1 "test_mkdir" $args)
resv_numb2=$(echo $res | cut -d " " -f 1)
attach cluster_b

cleanup $resv_numb1 cluster_a
cleanup $resv_numb2 cluster_b
