use std::sync::Arc;

use client_protocol::PathString;

use crate::consensus::{State, HB_TIMEOUT};
use crate::server_conn::protocol::Change;
use crate::server_conn::to_readserv::PubResult;
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

    pub fn serialize(&self) -> Vec<u8> {
        self.db.serialize()
    }

    #[tracing::instrument]
    pub async fn mkdir(&mut self, path: PathString) -> Result<(), DbError> {
        self.db.mkdir(path.clone()).await?;
        let res = self.servers.publish(&self.state, Change::DirAdded(path)).await;
        match res {
            PubResult::ReachedAll => (),
            PubResult::ReachedMajority => tokio::time::sleep(HB_TIMEOUT).await,
            PubResult::ReachedMinority => panic!("can not reach majority, crashing master"),
        }
        Ok(())
    }
}
