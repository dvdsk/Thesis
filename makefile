# point this to a directory having at least 10GB of free space
SCRATCH = /var/scratch/${USER}
# when using cargo outside of this makefile you must
# export these variables in the shell you are using
export RUSTUP_HOME=${PWD}/tmp/rustup
export CARGO_HOME=${PWD}/tmp/cargo

#----------------------------------------------------------------------------
# Links/Dir setup
#----------------------------------------------------------------------------
# these should both not be 'recreated' if the dir content changes
# use order-only prerequisite (target: | prerequisite)
data:
	mkdir -p ${SCRATCH}/data
	ln -s ${SCRATCH}/data data
tmp:
	mkdir -p ${SCRATCH}/tmp
	ln -s ${SCRATCH}/tmp tmp

tmp/cache/%: tmp
	$(eval NAME := $(subst tmp/cache/,,$@))
	mkdir -p ${SCRATCH}/tmp/$(NAME)/target
	-ln -s -t $(NAME) ${SCRATCH}/tmp/$(NAME)/target

#----------------------------------------------------------------------------
# Compilers and tools 
#----------------------------------------------------------------------------

tmp/cargo: | tmp
	curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs >> rustup.sh
	chmod +x rustup.sh
	bash rustup.sh -y --no-modify-path --profile minimal
	rm rustup.sh

#----------------------------------------------------------------------------
# Executables 
#----------------------------------------------------------------------------

.PHONY: client_examples
client_examples: $(wildcard find client/**/*.rs)
client_examples: | tmp/cargo tmp/cache/client
	tmp/cargo/bin/cargo build --examples --manifest-path client/Cargo.toml
	mkdir -p bin

bin/meta-server: $(wildcard find meta-server/**/*.rs)
bin/meta-server: | tmp/cargo tmp/cache/meta-server
	tmp/cargo/bin/cargo build --manifest-path meta-server/Cargo.toml
	mkdir -p bin
	cp ${PWD}/{meta-server/target/debug/,bin/}meta-server

bin/discovery-exchange-id: $(wildcard find discovery/**/*.rs)
bin/discovery-exchange-id: | tmp/cargo tmp/cache/discovery
	tmp/cargo/bin/cargo build --examples --manifest-path discovery/Cargo.toml
	mkdir -p bin
	cp ${PWD}/{discovery/target/debug/examples/exchange_id,bin/discovery-exchange-id}

bin/mkdir: client_examples
	cp ${PWD}/{client/target/debug/examples/mkdir,bin/test_mkdir}

#----------------------------------------------------------------------------
# Other
#----------------------------------------------------------------------------

deploy: bin/meta-server
	$(info test done)
	bash scripts/deploy_cluster.sh

discover: bin/discovery-exchange-id
	$(info test done)
	bash scripts/deploy_test.sh

test_mkdir: bin/mkdir bin/meta-server
	bash scripts/tests/mkdir.sh


.PHONY: clean
clean: 
	rm -rf **/target
