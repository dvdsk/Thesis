use std::time::Instant;

use protocol::PathString;
use thiserror::Error;

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

pub struct Directory {
    tree: sled::Db,
}

impl Directory {
    pub fn new() -> Self {
        Self {
            tree: sled::open("writeserv").unwrap(),
        }
    }

    pub async fn publish_mkdir(&self) {
        let flush_tree = self.tree.flush_async();
        let notify_reader_servs = todo!();

        futures::join!(flush_tree, notify_reader_servs);
    }

    pub async fn mkdir(&mut self, path: PathString) -> Result<(), DbError> {
        let res = self.tree
            .compare_and_swap(&path, None as Option<&[u8]>, Some(folder()))
            .unwrap(); // crash on any internal/io db error

        if let Err(e) = res {
            if e.current.unwrap().len() > 0 {
                Err(DbError::FileExists)?;
            } // no error if dir exists
        }

        self.publish_mkdir().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

}

