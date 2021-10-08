use std::sync::Arc;

use client_protocol::PathString;

use super::db::Db;
use super::{readserv, DbError};
use crate::consensus::{HbControl, State, HB_TIMEOUT};
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

    async fn consistent_change(
        &mut self,
        change: Change,
        apply_db: impl FnOnce(&mut Db) -> Result<(), DbError>,
    ) -> Result<(), DbError> {
        self.hb_ctrl.delay().await;

        let res = self.servers.publish(&self.state, change).await;
        let change_idx = match res {
            PubResult::ReachedAll(idx) => idx,
            PubResult::ReachedMajority(idx) => {
                tokio::time::sleep(HB_TIMEOUT).await;
                idx
            }
            PubResult::ReachedMinority => panic!("can not reach majority, crashing master"),
        };
        // it is essential we only store to the database AFTER we are sure
        // the change has disseminated through the cluster or consistancy WILL BREAK
        // as we can not roll back the change must now either succeed or we must crash
        // giving up control of the cluster
        apply_db(&mut self.db).unwrap();
        self.db.update(change_idx);
        self.db.flush().await;
        Ok(())
    }

    #[tracing::instrument]
    pub async fn rmdir(&mut self, path: PathString) -> Result<(), DbError> {
        let change = Change::DirRemoved(path.clone());
        let apply_to_db = |db: &mut Db| db.rmdir(path);
        self.consistent_change(change, apply_to_db).await
    }

    #[tracing::instrument]
    pub async fn mkdir(&mut self, path: PathString) -> Result<(), DbError> {
        let change = Change::DirAdded(path.clone());
        let apply_to_db = |db: &mut Db| db.mkdir(path);
        self.consistent_change(change, apply_to_db).await;
        Ok(())
    }
}
