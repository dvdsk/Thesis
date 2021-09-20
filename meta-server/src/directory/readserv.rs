use crate::server_conn::protocol::Change;

use super::db::Db;
use client_protocol::PathString;

#[derive(Debug, Clone)]
pub struct Directory {
    db: Db,
}

impl Directory {
    pub fn new() -> Self {
        Self { db: Db::new() }
    }

    pub fn into_db(self) -> Db {
        self.db
    }

    pub fn get_change_idx(&self) -> u64 {
        self.db.get_change_idx()
    }

    pub async fn mkdir(&self, path: PathString) {
        self
            .db
            .mkdir(path)
            .await
            .expect("file exists should be caught on write server");
    }

    pub async fn apply(&self, change: Change) {
        match change {
            Change::DirAdded(path) => self.mkdir(path).await,
        }
    }

    pub async fn update_from_master(&self, update: &[u8]) {
        self.db.replace_with_deserialized(update).await;
    }
}
