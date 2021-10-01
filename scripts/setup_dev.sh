#!/usr/bin/env bash
set -e

# Sets up dev env for editing on remote cluster while keeping lsp fast by
# keeping build cache/files local to the machine building

HOST=dpsdas
REMOTE_DIR=mock-fs
BUILD_CACHE_DIR=target

mkdir -p remote
mkdir -p remote/mnt
# fusermount -u remote/mnt ||
# sshfs remote/mnt $HOST:$REMOTE_DIR

dirs_to_mount=()
files_to_mount=($(fd --type f --max-depth 1 . 'remote/mnt'))

function get_dirs() {
	dirs=$(fd --type d --max-depth 1 . $1)
	# if [ -d "$1/.git" ]; then
	# 	dirs="$dirs $1/.git"
	# fi
	echo $dirs
}

dirs=$(get_dirs 'remote/mnt')
for dir in $dirs; do
	if [[ $(fd $BUILD_CACHE_DIR $dir) != "" ]]; then
		files_to_mount=(${files_to_mount[@]} $(fd --type f --max-depth 1 . $dir))
		for d in $(get_dirs $dir); do
			if [[ $d != $BUILD_CACHE_DIR ]]; then
				dirs_to_mount+=($d)
			fi
		done
	else 
		dirs_to_mount+=($dir)
	fi
done

# link dirs
for dir in ${dirs_to_mount[@]}; do
	path=$(echo $dir | cut -d "/" -f 3-)
	parent=$(dirname remote/$path)
	rm -r remote/$path 2> /dev/null || true
	mkdir -p $parent
	ln --relative --symbolic --target-directory=$parent $dir
done

# link files
for file in ${files_to_mount[@]}; do
	path=$(echo $file | cut -d "/" -f 3-)
	parent=$(dirname remote/$path)
	mkdir -p $parent
	rm -r remote/$path 2> /dev/null || true
	ln --relative --symbolic --target-directory=$parent $file
done

