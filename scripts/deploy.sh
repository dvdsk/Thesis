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

function split_cmd()
{
	local node=$1
	local base_cmd="$2"
	echo "ssh $node \\\"$base_cmd\\\"; sleep 90"
}

function run_in_tmux_splits()
{
	local base_cmd="$1"
	local nodes="${@:2}"
	local tmux_cmd="tmux new -s deployed"
	for node in $nodes; do
		local cmd=\"$(split_cmd $node "$base_cmd")\"
		tmux_cmd="$tmux_cmd "$cmd" ';' split"
	done

	eval ${tmux_cmd::-9} # print with last split removed
}

function window_cmd()
{
	local node="$1"
	local dir="$2"
	local base_cmd="$3"
	# remain on exit is broken on older tmux (https://github.com/tmux/tmux/issues/1354)
	# for now just using a long sleep to ensure the window stays open
	echo "tmux setw remain-on-exit on; ssh $node \"cd "$dir"; ./$base_cmd\"; sleep 99999"
}

function run_in_tmux_windows()
{
	local dir="$1"
	local base_cmd="$2"
	local nodes=(${@:3})

	local cmd=$(window_cmd ${nodes[0]} $dir "$base_cmd")
	tmux new-session -s "deployed" -n ${nodes[0]} -d "$cmd"

	local len=${#nodes[@]}
	for (( i = 1; i < $len; i++ )); do
		local name=${nodes[$i]}
		local cmd=$(window_cmd ${nodes[i]} $dir "$base_cmd")
		tmux new-window -t "deployed:$i" -n $name -d "$cmd"
	done
}

resv_numb=""
function deploy()
{
	local numb_nodes=$1
	local bin=$2
	local args="${@:3}"

	local duration=5
	resv_numb=$(preserve -# ${numb_nodes} -t 00:${duration}:05 | head -n 1 | cut -d ' ' -f 3)
	resv_numb=${resv_numb::-1}

	preserve -llist >&2
	wait_for_allocation $resv_numb

	local nodes=$(node_list $resv_numb)

	for node in $nodes; do
		ssh -t $node <<- EOF & >&2 # TODO run in parallel
		mkdir -p /tmp/mock-fs
		cp ${PWD}/bin/$bin /tmp/mock-fs/
EOF
	done
	wait

	# run_in_tmux_splits "/tmp/mock-fs/$bin $args" $nodes
	run_in_tmux_windows /tmp/mock-fs/ "$bin $args" $nodes
	echo $resv_numb $nodes
}

function attach()
{
	tmux attach-session -t "deployed"
}

function cleanup()
{
	local resv_numb=$1
	tmux kill-session -t "deployed" 
	preserve -c $resv_numb #cancel reservation
}
