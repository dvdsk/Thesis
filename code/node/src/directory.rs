use std::ffi::OsStr;
use std::ops::Range;
use std::os::unix::prelude::OsStrExt;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, Context};
use color_eyre::Result;
use tracing::instrument;

use crate::{minister, raft};

mod entry;
use entry::Entry;
use protocol::AccessKey;

#[derive(Debug, Clone)]
pub struct Directory {
    tree: sled::Tree,
}

fn dbkey(path: &Path) -> &[u8] {
    path.as_os_str().as_bytes()
}

pub struct LeaseGuard<'a> {
    pub dir: &'a Directory,
    path: &'a Path,
    pub key: AccessKey,
}

impl<'a> LeaseGuard<'a> {
    pub fn key(&self) -> AccessKey {
        self.key
    }
}

impl<'a> Drop for LeaseGuard<'a> {
    fn drop(self: &mut LeaseGuard<'a>) {
        self.dir.revoke_access(&self.path, self.key)
    }
}

impl Directory {
    fn lease_guard<'a>(&'a self, path: &'a Path, key: AccessKey) -> LeaseGuard<'a> {
        LeaseGuard {
            dir: self,
            path,
            key,
        }
    }

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
        for dir_entry in self
            .tree
            .scan_prefix(dbkey(path))
            .keys()
            .map(Result::unwrap)
        {
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

    #[instrument(skip(self), err)]
    pub(crate) fn get_read_access<'a>(
        &'a self,
        path: &'a Path,
        req_range: &Range<u64>,
    ) -> Result<LeaseGuard<'a>> {
        let mut key = None;
        self.tree
            .update_and_fetch(dbkey(path), |bytes| {
                let mut entry = Entry::from_bytes(&bytes?);
                if !entry.overlapping_write_access(&req_range) {
                    key = Some(entry.add_read_access(&req_range));
                    Some(entry.to_bytes())
                } else {
                    bytes.map(Vec::from)
                }
            })
            .wrap_err("internal db error")?;

        let key = key.ok_or_else(|| eyre!("could not give access, overlapping writes"))?;
        Ok(self.lease_guard(path, key))
    }

    #[instrument(skip(self), err)]
    pub(crate) fn get_exclusive_access<'a>(
        &'a self,
        path: &'a Path,
        req_range: &Range<u64>,
    ) -> Result<LeaseGuard<'a>> {
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
            .wrap_err("internal db error")?;

        let key = key.ok_or_else(|| eyre!("could not give access, overlapping writes"))?;
        Ok(self.lease_guard(path, key))
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

    #[instrument(skip(self), err)]
    pub(crate) fn get_overlapping(
        &self,
        path: &PathBuf,
        range: &Range<u64>,
    ) -> Result<Vec<AccessKey>> {
        let bytes = self
            .tree
            .get(dbkey(path))
            .wrap_err("internal db error")?
            .ok_or_else(|| eyre!("no entry for path"))?;
        let entry = Entry::from_bytes(&bytes);
        Ok(entry.overlapping(range))
    }
}
