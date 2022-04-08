source $(dirname "$0")/deploy.sh

## deploy cluster
numb_nodes=23
bin="discovery-exchange-id"
args="$numb_nodes" 
deploy $numb_nodes $bin $args
