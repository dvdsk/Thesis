source $(dirname "$0")/deploy.sh

## deploy cluster
numb_nodes=3
bin="meta-server"
args="--client-port 50978 --cluster-size $numb_nodes --control-port 50979 --tracing-endpoint fs1" 
deploy $numb_nodes $bin $args
