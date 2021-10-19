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

tmp/target/%: tmp
	$(eval NAME := $(subst tmp/target/,,$@))
	mkdir -p ${SCRATCH}/$@
	ln --force --symbolic -T ${SCRATCH}/$@ $(NAME)/target 

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
client_examples: REBUILD_ALWAYS
client_examples: | tmp/cargo tmp/target/client
	tmp/cargo/bin/cargo build --examples --manifest-path client/Cargo.toml --release
	mkdir -p bin

bin/meta-server: REBUILD_ALWAYS
bin/meta-server: | tmp/cargo tmp/target/meta-server
	tmp/cargo/bin/cargo build --manifest-path meta-server/Cargo.toml --release
	mkdir -p bin
	cp ${PWD}/{meta-server/target/release/,bin/}meta-server

bin/discovery-exchange-id: REBUILD_ALWAYS
bin/discovery-exchange-id: | tmp/cargo tmp/target/discovery
	tmp/cargo/bin/cargo build --examples --manifest-path discovery/Cargo.toml
	mkdir -p bin
	cp ${PWD}/{discovery/target/debug/examples/exchange_id,bin/discovery-exchange-id}

bin/test_mkdir: client_examples
	cp ${PWD}/{client/target/release/examples/mkdir,bin/test_mkdir}

bin/bench_mkdir: client_examples
	cp ${PWD}/{client/target/release/examples/bench_mkdir,bin/bench_mkdir}

bin/bench_ls: client_examples
	cp ${PWD}/{client/target/release/examples,bin}/bench_ls

#----------------------------------------------------------------------------
# Other
#----------------------------------------------------------------------------

deploy: bin/meta-server
	$(info test done)
	bash scripts/deploy_cluster.sh

discover: bin/discovery-exchange-id
	$(info test done)
	bash scripts/deploy_test.sh

test_mkdir: bin/test_mkdir bin/meta-server
	bash scripts/tests/mkdir.sh

bench_mkdir: bin/bench_mkdir bin/meta-server
	bash scripts/bench/mkdir.sh

bench_ls: bin/bench_ls bin/meta-server
	bash scripts/bench/ls.sh

.PHONY: clean REBUILD_ALWAYS
clean: 
	rm -rf tmp/target
REBUILD_ALWAYS:
