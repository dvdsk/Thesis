#!/usr/bin/env bash

echo opening ssh to cluster frontend binding to port 8080

cmd="ssh -D 8080 dpsdas \"mock-fs/jeager-all-in-one\"; sleep 90"
tmux kill-session -t "tracing" 
tmux new-session -s "tracing" -n "tracing" -d "$cmd"
echo attach and check for password prompt if things dont work
