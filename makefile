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

.PHONY: mete-server-source 
meta-server-source: $(wildcard find meta-server/**/*.rs)

bin/meta-server: meta-server-source
bin/meta-server: | tmp/cargo
	tmp/cargo/bin/cargo build --manifest-path meta-server/Cargo.toml
	mkdir -p bin
	ln -fs ${PWD}/{meta-server/target/debug/,bin/}meta-server

bin/discovery-exchange-id: $(wildcard find discovery/**/*.rs)
bin/discovery-exchange-id: | tmp/cargo
	tmp/cargo/bin/cargo build --examples --manifest-path discovery/Cargo.toml
	mkdir -p bin
	ln -fs ${PWD}/{discovery/target/debug/examples/exchange_id,bin/discovery-exchange-id}

#----------------------------------------------------------------------------
# Other
#----------------------------------------------------------------------------

deploy: bin/meta-server
	$(info test done)
	bash scripts/deploy_cluster.sh

dev: bin/discovery-exchange-id
	$(info test done)
	bash scripts/deploy_test.sh

.PHONY: clean
clean: 
	rm -rf **/target
