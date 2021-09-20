#!/usr/bin/env bash

echo opening ssh to cluster frontend binding to port 8080

cmd="ssh -D 8080 dpsdas \"mock-fs/tracing/jeager-all-in-one\"; sleep 90"
tmux kill-session -t "tracing" 
tmux new-session -s "tracing" -n "tracing" -d "$cmd"
