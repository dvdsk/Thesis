#!/usr/bin/env bash

function node_list()
{
	preserve -long-list \
		| grep ${resv_numb} \
		| cut -f 9-
}

function wait_for_allocation()
{
	printf "waiting for nodes " >&2
	while [ "$(node_list)" == "-" ]
	do
		sleep 0.25
		printf "." >&2
	done
	echo "" >&2
}

function deploy()
{
	duration=3
	resv_numb=$(preserve -# ${numb_nodes} -t 00:${duration}:05 | head -n 1 | cut -d ' ' -f 3)
	wait_for_allocation
	nodes=$(node_list)

	for node in $nodes; do
		ssh_output=$(ssh $node <<- EOF
		mkdir /tmp/mock-fs
		cp ${PWD}/meta-server/target/debug/meta-server /tmp/mock-fs
		./tmp/mock-fs
EOF
)
	done
)
	echo $resv_numb
}
