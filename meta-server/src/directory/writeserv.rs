use client_protocol::PathString;

use crate::server_conn::protocol::Change;
use crate::server_conn::to_readserv::ReadServers;
use super::{DbError, readserv};
use super::db::Db;

#[derive(Debug, Clone)]
pub struct Directory {
    db: Db,
    servers: ReadServers,
}

impl Directory {
    pub fn from(dir: readserv::Directory, servers: ReadServers) -> Self {
        Self { db: dir.into_db(), servers }
    }

    pub fn get_change_idx(&self) -> u64 {
        self.db.get_change_idx()
    }

    pub async fn mkdir(&mut self, path: PathString) -> Result<(), DbError> {
        self.db.mkdir(path.clone()).await?;
        self.servers.publish(Change::DirAdded(path)).await;
        Ok(())
    }
}
