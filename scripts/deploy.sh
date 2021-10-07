#!/usr/bin/env bash
set -e

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

function window_cmd()
{
	local node="$1"
	local dir="$2"
	local base_cmd="$3"
	# remain on exit is broken on older tmux (https://github.com/tmux/tmux/issues/1354)
	# for now just using a long sleep to ensure the window stays open
	echo "tmux setw remain-on-exit on; ssh $node \"cd "$dir"; RUST_BACKTRACE=1 ./$base_cmd\"; sleep 99999"
}

function new_session_name()
{
	tmux has-session -t "cluster_a" 2>/dev/null
	if [ $? != 0 ]; then
		echo cluster_a
	else
		echo cluster_b
	fi
}

function run_in_tmux_windows()
{
	local dir="$1"
	local base_cmd="$2"
	local nodes=(${@:3})
	local session_name=$(new_session_name)

	local cmd=$(window_cmd ${nodes[0]} $dir "$base_cmd")
	tmux new-session -s $session_name -n ${nodes[0]} -d "$cmd"
	local len=${#nodes[@]}
	for (( i = 1; i < $len; i++ )); do
		local name=${nodes[$i]}
		local cmd=$(window_cmd ${nodes[i]} $dir "$base_cmd")
		tmux new-window -t "$session_name:$i" -n $name -d "$cmd"
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

	# clear the tmp directory to ensure any state
	# is resest (think databases etc)
	for node in $nodes; do
		ssh -t $node <<- EOF & >&2
		rm -r /tmp/mock-fs
		mkdir /tmp/mock-fs
		cp ${PWD}/bin/$bin /tmp/mock-fs/
EOF
done
	wait

	run_in_tmux_windows /tmp/mock-fs/ "$bin $args" $nodes
	echo $resv_numb $nodes
}

function attach()
{
	local session_name=`[ -z $1 ] && echo "cluster_a" || echo $1`
	tmux attach-session -t $session_name
}

function cleanup()
{
	local resv_numb=$1
	local session_name=`[ -z $2 ] && echo "cluster_a" || echo $2`
	tmux kill-session -t $session_name
	preserve -c $resv_numb #cancel reservation
}
