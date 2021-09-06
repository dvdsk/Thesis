#!/usr/bin/env bash

function node_list()
{
	local resv_numb=$1
	preserve -long-list \
		| grep $resv_numb \
		| cut -f 9-
}

function wait_for_allocation()
{
	local resv_numb=$1
	printf "waiting for nodes " >&2
	while [ "$(node_list $resv_numb)" == "-" ]
	do
		sleep 0.25
		printf "." >&2
	done
	echo "" >&2
}

function cmd()
{
	local node=$1
	local base_cmd="$2"
	echo "ssh $node \\\"$base_cmd\\\"; sleep 90"
}

function run_in_tmux_splits()
{
	local nodes="${@:2}"
	local base_cmd="$1"
	local tmux_cmd="tmux new -s run_in_splits"
	for node in $nodes; do
		tmux_cmd="$tmux_cmd \"$(cmd $node "$base_cmd")\" ';' split"
	done

	eval ${tmux_cmd::-9} # print with last split removed
}

function deploy()
{
	local duration=0
	local numb_nodes=3
	local resv_numb=$(preserve -# ${numb_nodes} -t 00:${duration}:05 | head -n 1 | cut -d ' ' -f 3)
	local resv_numb=${resv_numb::-1}

	node_list $resv_numb
	wait_for_allocation $resv_numb

	local nodes=$(node_list $resv_numb)
	echo "got nodes: $nodes"

	for node in $nodes; do
		ssh -t $node <<- EOF # TODO run in parallel
		mkdir -p /tmp/mock-fs
		cp ${PWD}/bin/meta-server /tmp/mock-fs/
EOF
	done

	local base_cmd="/tmp/mock-fs/meta-server --client-port 8080 --cluster-size $numb_nodes --control-port 8081" 
	run_in_tmux_splits "$base_cmd" $nodes
	tmux kill-session -t "run_in_splits" 
}

deploy
