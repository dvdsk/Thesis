use std::time::Instant;

use client_protocol::PathString;

use crate::server_conn::protocol::Change;
use crate::server_conn::to_readserv::ReadServers;

#[derive(Debug)]
pub enum DbError {
    FileExists,
}

type ChunkId = u64;

pub struct File {
    lease: Option<Instant>,
    chunks: Vec<ChunkId>,
}

pub fn folder() -> sled::IVec {
    sled::IVec::default()
}

#[derive(Debug, Clone)]
pub struct Directory {
    tree: sled::Db,
    servers: ReadServers,
}

impl Directory {
    pub fn new(servers: ReadServers) -> Self {
        Self {
            tree: sled::open("writeserv").unwrap(),
            servers,
        }
    }

    pub async fn publish_mkdir(&self, path: PathString) {
        let flush_tree = self.tree.flush_async();
        let notify_reader_servs = self.servers.publish(Change::DirAdded(path));

        let (res_a, _) = futures::join!(flush_tree, notify_reader_servs);
        res_a.unwrap();
    }

    pub async fn mkdir(&mut self, path: PathString) -> Result<(), DbError> {
        let res = self
            .tree
            .compare_and_swap(&path, None as Option<&[u8]>, Some(folder()))
            .unwrap(); // crash on any internal/io db error

        if let Err(e) = res {
            if e.current.unwrap().len() > 0 {
                Err(DbError::FileExists)?;
            } // no error if dir exists
        }

        self.publish_mkdir(path).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
}
