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
tmp:
	mkdir -p ${SCRATCH}/tmp
	ln -s ${SCRATCH}/tmp tmp

tmp/target: tmp
	mkdir -p ${SCRATCH}/target
	ln --force --symbolic -T ${SCRATCH}/target code/target 

bin:
	mkdir -p bin

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

# we let cargo decide if we should rebuild, marking phony ensures
# make always calls cargo
.PHONY: benchmark clean

benchmark: | bin
benchmark: | tmp/cargo tmp/target
	export RUSTFLAGS="--cfg tokio_unstable"; \
	tmp/cargo/bin/cargo build --manifest-path code/Cargo.toml --release --bins
	cp code/target/release/benchmark bin/benchmark
	cp code/target/release/node bin/node
	cp code/target/release/bench_client bin/bench_client

clean:
	rm -rf tmp/target
