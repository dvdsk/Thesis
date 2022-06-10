use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::raft::State;

pub enum Access {
    Reader(Range<usize>),
    Writer(Range<usize>),
}

#[derive(Default)]
pub struct File {
    path: PathBuf,
    size: usize,
    areas: Vec<Access>,
}

#[derive(Default)]
pub struct Folder {
    path: PathBuf,
    size: usize,
    areas: Vec<Access>,
}

impl Entry {
    fn new_file(path: PathBuf) -> Self {
        Self::File(File {
            path, .. Default::default()
        })
    }
    fn new_folder(path: PathBuf) -> Self {
        Self::Folder(Folder {
            path, .. Default::default()
        })
    }
}


pub enum Entry {
    File(File),
    Folder(Folder),
}

pub struct Directory {
    tree: Arc<RwLock<Vec<Entry>>>,
}

impl Directory {
    pub fn add_entry(&mut self, entry: Entry) {}

    pub fn remove_path(&mut self, path: PathBuf) {}

    pub fn from_committed(state: &State) -> Self {
        use crate::minister::Order::*;

        let tree = Vec::new();
        let mut dir = Self {
            tree: Arc::new(RwLock::new(tree)),
        };
        for order in state.committed() {
            CreateFile(path) => dir.add_entry(Entry::new_file(path)),
            CreateFolder(path) => dir.add_entry(Entry::new_folder(path)),
            Remove(path) => dir.remove_path(path),
        }

        dir
    }
}
