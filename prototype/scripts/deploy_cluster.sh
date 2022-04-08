source $(dirname "$0")/deploy.sh


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
nserver_nodes=23
bin="meta-server"
args="\
	--cluster-size $nserver_nodes \
	--tracing-endpoint fs1.cm.cluster \
	--run-numb $run_numb \
	deploy \
	--client-port 50975 \
	--control-port 50972"
deploy $numb_nodes $bin $args
attach
cleanup
