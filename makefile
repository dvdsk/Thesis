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

tmp/target/%: tmp
	$(eval NAME := $(subst tmp/target/,,$@))
	mkdir -p ${SCRATCH}/$@
	ln --force --symbolic -T ${SCRATCH}/$@ $(NAME)/target 

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

.PHONY: client_examples
benchmark: REBUILD_ALWAYS
benchmark: | bin
benchmark: | tmp/cargo tmp/target/benchmark
	tmp/cargo/bin/cargo build --examples --manifest-path code/benchmark/Cargo.toml --release
	cp tmp/target/release/benchmark bin/benchmark

