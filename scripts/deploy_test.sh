source $(dirname "$0")/deploy.sh

## deploy cluster
numb_nodes=3
bin="discovery-exchange-id"
args="5" 
deploy $numb_nodes $bin $args
