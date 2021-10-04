##!/usr/bin/env bash
set -e

source $(dirname "$0")/../deploy.sh

#args: list of node dns names
function to_infiniband_ips()
{
	SITE_IP=10.149.1
	for node in $@; do
		node_id=$(echo ${node:5:6} | sed 's/^0*//') #node102 -> 02
		printf "${SITE_IP}.${node_id} "
	done
	echo ""
}

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
numb_nodes=3
bin="meta-server"
args="
	--client-port $client_port \
	--cluster-size $numb_nodes \
	--control-port 50972 \
	--tracing-endpoint fs1.cm.cluster \
	--run-numb $run_numb"

res=$(deploy $numb_nodes $bin $args)
echo $res
resv_numb1=$(echo $res | cut -d " " -f 1)
nodes=$(echo $res | cut -d " " -f 2-)
ips=$(to_infiniband_ips $nodes)

echo giving fs-cluster time to find eachother and elect a leader
sleep 10

args="$client_port $ips"
res=$(deploy 1 "test_mkdir" $args)
resv_numb2=$(echo $res | cut -d " " -f 1)
attach cluster_b

cleanup $resv_numb1 cluster_a
cleanup $resv_numb2 cluster_b
