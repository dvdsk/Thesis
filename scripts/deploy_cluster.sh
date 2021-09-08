source $(dirname "$0")/deploy.sh

## deploy cluster
numb_nodes=3
bin="meta-server"
args="--client-port 8080 --cluster-size $numb_nodes --control-port 8081" 
deploy $numb_nodes $bin $args
