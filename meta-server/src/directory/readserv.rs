use crate::server_conn::protocol::Change;

use super::db::Db;
use client_protocol::{FsEntry, PathString};

#[derive(Debug, Clone)]
pub struct Directory {
    db: Db,
}

impl Directory {
    pub fn new() -> (Self, u64) {
        let (db, chance_idx) = Db::new();
        (Self { db }, chance_idx)
    }

    pub fn into_db(self) -> Db {
        self.db
    }

    pub fn get_change_idx(&self) -> u64 {
        self.db.get_change_idx()
    }

    pub fn mkdir(&self, path: PathString) {
        self
            .db
            .mkdir(path)
            .expect("file exists should be caught on write server");
    }

    pub fn ls(&self, path: PathString) -> Vec<FsEntry> {
        self.db.ls(path)
    }

    pub async fn apply(&self, change: Change, change_idx: u64) {
        match change {
            Change::DirAdded(path) => self.mkdir(path),
        }
        self.db.update(change_idx);
        self.db.flush().await;
        tracing::warn!("dir added");
    }

    pub async fn update_from_master(&self, update: &[u8]) {
        self.db.replace_with_deserialized(update).await;
    }
}
