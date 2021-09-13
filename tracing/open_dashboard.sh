#!/usr/bin/env bash

echo opening ssh to cluster frontend binding to port 8080

cmd="ssh -D 8080 dpsdas \"mock-fs/tracing/jeager-all-in-one\"; sleep 90"
tmux kill-session -t "tracing" 
tmux new-session -s "tracing" -n "tracing" -d "$cmd"

sleep 10
firefox http://127.0.0.1:16686
