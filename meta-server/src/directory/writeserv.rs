use std::sync::Arc;

use client_protocol::PathString;

use super::db::Db;
use super::{readserv, DbError};
use crate::consensus::{HB_TIMEOUT, HbControl, State};
use crate::server_conn::protocol::Change;
use crate::server_conn::to_readserv::PubResult;
use crate::server_conn::to_readserv::ReadServers;

#[derive(Debug, Clone)]
pub struct Directory {
    db: Db,
    servers: ReadServers,
    state: Arc<State>,
    hb_ctrl: HbControl,
}

impl Directory {
    pub fn from(
        dir: readserv::Directory,
        servers: ReadServers,
        state: &Arc<State>,
        hb_ctrl: HbControl,
    ) -> Self {
        Self {
            db: dir.into_db(),
            servers,
            state: state.clone(),
            hb_ctrl,
        }
    }

    pub fn get_change_idx(&self) -> u64 {
        self.db.get_change_idx()
    }

    pub fn serialize(&self) -> Vec<u8> {
        self.db.serialize()
    }

    #[tracing::instrument]
    pub async fn mkdir(&mut self, path: PathString) -> Result<(), DbError> {
        self.hb_ctrl.delay().await;

        let res = self
            .servers
            .publish(&self.state, Change::DirAdded(path.clone()))
            .await;
        match res {
            PubResult::ReachedAll => (),
            PubResult::ReachedMajority => tokio::time::sleep(HB_TIMEOUT).await,
            PubResult::ReachedMinority => panic!("can not reach majority, crashing master"),
        }
        // it is essential we only store to the database AFTER we are sure
        // the change has disseminated through the cluster or consistancy WILL BREAK
        self.db.mkdir(path).await?;
        // self.db.set_change_idx(change_idx).await?; TODO
        Ok(())
    }
}
