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

    pub async fn mkdir(&mut self, path: PathString) {
        self
            .db
            .mkdir(path)
            .await
            .expect("file exists should be caught on write server");
    }
}
