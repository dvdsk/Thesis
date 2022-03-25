#!/usr/bin/env bash

# call with raft-fs root dir as working dir

set -e

function window_cmd()
{
	local dir="$1"
	local base_cmd="$2"
	# remain on exit is broken on older tmux (https://github.com/tmux/tmux/issues/1354)
	# for now just using a long sleep to ensure the window stays open
	echo "tmux setw remain-on-exit on; cd $dir; RUST_BACKTRACE=1 $base_cmd; sleep 99999"
}

function run_in_tmux_windows()
{
	local size="$1"
	local base_cmd="$2"
	local session_name=raftfs
	echo base_cmd: $base_cmd

	local dir=`mktemp --directory --suffix _raftfs`
	local name="0"
	local cmd=$(window_cmd $dir "$base_cmd")
	echo $cmd
	tmux new-session -s $session_name -n $name -d "${cmd}" # TODO FIXME
	for (( i = 1; i < size; i++ )); do
		local name="$i"
		local dir=`mktemp --directory --suffix raft-fs`
		local cmd=$(window_cmd $dir "$base_cmd")
		tmux new-window -t "$session_name:$i" -n $name -d "$cmd"
	done
}

SIZE=3
cd meta-server
cargo b

args="\
	--cluster-size $SIZE \
	--tracing-endpoint fs1.cm.cluster \
	--run-numb 1"

bin=`pwd`/target/debug/meta-server

run_in_tmux_windows $SIZE "$bin $args"
tmux attach-session -t raftfs
tmux kill-session -t raftfs
