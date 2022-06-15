use std::ops::Range;
use std::os::unix::prelude::OsStrExt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::raft::State;

#[derive(Serialize, Deserialize)]
pub enum Access {
    Reader(Range<usize>),
    Writer(Range<usize>),
}

#[derive(Default, Serialize, Deserialize)]
pub struct File {
    size: usize,
    areas: Vec<Access>,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Folder {
    size: usize,
    areas: Vec<Access>,
}

pub enum Entry {
    File(File),
    Folder(Folder),
}

pub struct Directory {
    tree: sled::Tree,
}

impl Directory {
    pub fn add_entry(&mut self, path: PathBuf) {
        let entry = match path.is_dir() {
            true => Folder::default(),
            false => Folder::default(),
        };
        let key = path.as_os_str().as_bytes();
        let entry = bincode::serialize(&entry).unwrap();
        self.tree.insert(key, entry).unwrap();
    }

    pub fn remove_path(&mut self, path: PathBuf) {
        let key = path.as_os_str().as_bytes();
        if path.is_file() {
            self.tree.remove(key);
            return;
        }

        // TODO check if mounted/subdir part
        for dir_entry in self.tree.scan_prefix(key).keys().map(Result::unwrap) {
            self.tree.remove(dir_entry);
        }
    }

    pub fn from_committed(state: &State, db: sled::Db) -> Self {
        use crate::minister::Order::*;

        db.drop_tree("directory").unwrap();
        let tree = db.open_tree("directory").unwrap();
        let mut dir = Self { tree };
        for order in state.committed() {
            // TODO make raft generic over order
            match order {
                Create(path) => dir.add_entry(path),
                Remove(path) => dir.remove_path(path),
            }
        }

        dir
    }
}
