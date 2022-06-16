use std::ffi::OsStr;
use std::ops::Range;
use std::os::unix::prelude::OsStrExt;
use std::path::{Path, PathBuf};

use slotmap::SlotMap;
use serde::{Deserialize, Serialize};

use crate::{minister, raft};

/// access is explicitly revoked once
/// a client times out
#[derive(Serialize, Deserialize, Debug)]
pub enum Access {
    Reader(Range<u64>),
    Writer(Range<u64>),
}

impl Access {
    fn range(&self) -> &Range<u64> {
        match self {
            Self::Reader(r) => r,
            Self::Writer(r) => r,
        }
    }
}

slotmap::new_key_type! { pub struct AccessKey; }

#[derive(Default, Serialize, Deserialize)]
pub struct Entry {
    size: usize,
    /// list sorted by start position
    areas: SlotMap<AccessKey, Access>,
}

impl Entry {
    fn from_bytes(bytes: &[u8]) -> Self {
        bincode::deserialize(bytes).unwrap()
    }

    fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }

    fn any_overlapping_access(&self, req: &Range<u64>) -> bool {
        use crate::util::Overlap;
        for access in self.areas.values() {
            if access.range().has_overlap_with(req) {
                return true;
            }
        }
        return false;
    }

    fn overlapping_write_access(&self, req: &Range<u64>) -> bool {
        use crate::util::Overlap;
        for access in self.areas.values() {
            match access {
                Access::Writer(range) if range.has_overlap_with(req) => return true,
                _ => (),
            }
        }
        return false;
    }

    fn add_write_access(&mut self, range: &Range<u64>) -> AccessKey {
        self.areas.insert(Access::Writer(range.clone()))
    }

    fn revoke_access(&mut self, key: AccessKey) {
        self.areas.remove(key);
    }
}

#[derive(Debug, Clone)]
pub struct Directory {
    tree: sled::Tree,
}

fn dbkey(path: &Path) -> &[u8] {
    path.as_os_str().as_bytes()
}

impl Directory {
    pub fn add_entry(&mut self, path: &Path) {
        let entry = Entry::default();
        self.tree.insert(dbkey(path), entry.to_bytes()).unwrap();
    }

    pub fn remove_path(&mut self, path: &Path) {
        if path.is_file() {
            self.tree.remove(dbkey(path)).unwrap();
            return;
        }

        // TODO check if mounted/subdir part
        for dir_entry in self.tree.scan_prefix(dbkey(path)).keys().map(Result::unwrap) {
            self.tree.remove(dir_entry).unwrap();
        }
    }

    pub fn update(&mut self, order: minister::Order) {
        use crate::minister::Order::*;

        match order {
            Create(path) => self.add_entry(&path),
            Remove(path) => self.remove_path(&path),
            None => (),
        }
    }

    pub fn list(&mut self, path: &Path) -> Vec<PathBuf> {
        self.tree
            .scan_prefix(dbkey(path))
            .keys()
            .map(Result::unwrap)
            .map(|buf| {
                let str = OsStr::from_bytes(&buf);
                PathBuf::from(str)
            })
            .collect()
    }

    pub fn from_committed(state: &raft::State<minister::Order>, db: &mut sled::Db) -> Self {
        db.drop_tree("directory").unwrap();
        let tree = db.open_tree("directory").unwrap();
        let mut dir = Self { tree };
        for order in state.committed() {
            dir.update(order);
        }

        dir
    }

    pub(crate) fn get_write_access(
        &self,
        path: &Path,
        req_range: &Range<u64>,
    ) -> Result<AccessKey, String> {
        let mut key = None;
        self.tree
            .update_and_fetch(dbkey(path), |bytes| {
                let mut entry = Entry::from_bytes(&bytes?);
                if !entry.overlapping_write_access(&req_range) {
                    key = Some(entry.add_write_access(&req_range));
                    Some(entry.to_bytes())
                } else {
                    bytes.map(Vec::from)
                }
            })
            .unwrap();

        key.ok_or(String::from("could not give access, overlapping writes"))
    }

    pub(crate) fn revoke_access(&self, path: &Path, access: AccessKey) {
        self.tree
            .update_and_fetch(dbkey(path), |bytes| {
                let mut entry = Entry::from_bytes(&bytes?);
                entry.revoke_access(access);
                Some(entry.to_bytes())
            })
            .unwrap();
    }
}
