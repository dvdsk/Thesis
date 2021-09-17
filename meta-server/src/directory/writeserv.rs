use std::sync::Arc;

use client_protocol::PathString;

use crate::consensus::State;
use crate::server_conn::protocol::Change;
use crate::server_conn::to_readserv::ReadServers;
use super::{DbError, readserv};
use super::db::Db;

#[derive(Debug, Clone)]
pub struct Directory {
    db: Db,
    servers: ReadServers,
    state: Arc<State>,
}

impl Directory {
    pub fn from(dir: readserv::Directory, servers: ReadServers, state: &Arc<State>) -> Self {
        Self { db: dir.into_db(), servers, state: state.clone() }
    }

    pub fn get_change_idx(&self) -> u64 {
        self.db.get_change_idx()
    }

    pub async fn mkdir(&mut self, path: PathString) -> Result<(), DbError> {
        self.db.mkdir(path.clone()).await?;
        self.servers.publish(&self.state, Change::DirAdded(path)).await;
        Ok(())
    }
}
