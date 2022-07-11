use std::{ops::Range, collections::HashSet};
use std::path::PathBuf;
use std::time::Duration;

use protocol::{DirList, Request};

mod lease;
mod map;
mod random_node;
mod request;

use lease::Lease;
use map::{Map, Ministry};
pub use random_node::{ChartNodes, RandomNode};

use request::Connection;
use tokio::time::sleep;
use tracing::instrument;

pub struct Ticket {
    idx: protocol::Idx,
    needs_minister: bool,
    path: PathBuf,
}

pub struct Client<T: RandomNode> {
    pub ticket: Option<Ticket>,
    map: Map,
    nodes: T,
    pub conn: Option<Connection>,
}

pub struct ReadableFile<'a, T: RandomNode> {
    client: &'a mut Client<T>,
    path: PathBuf,
    pos: u64,
}

// speed in bytes per millisec
async fn mock_data_plane_interaction(buf: &[u8], n_done: &mut usize, speed: usize) {
    let block_dur: usize = 50; //milli seconds
    let block_size: usize = speed * block_dur;
    loop {
        let left = buf.len() - *n_done;
        if left == 0 {
            return; // done (simulating) reading
        }

        let block = left.min(block_size);
        let read_dur = block / speed;

        // simulate reading a block by sleeping
        sleep(Duration::from_millis(read_dur as u64)).await;
        // todo simulate reading / writing by copying some bytes around
        *n_done += block;
    }
}

#[instrument(skip(buf))]
async fn mock_read(buf: &mut [u8], n_read: &mut usize) {
    let read_speed: usize = 500_000_000 / 1000; // 500 mb/s in bytes per millisec
    mock_data_plane_interaction(buf, n_read, read_speed).await;
}

#[instrument(skip(buf))]
async fn mock_write(buf: &[u8], n_written: &mut usize) {
    let write_speed: usize = 200_000_000 / 1000; // 500 mb/s in bytes per millisec
    mock_data_plane_interaction(buf, n_written, write_speed).await;
}

impl<'a, T: RandomNode> ReadableFile<'a, T> {
    pub fn seek(&mut self, pos: u64) {
        self.pos = pos;
    }

    pub async fn read(&mut self, buf: &mut [u8]) {
        let mut n_read = 0;
        loop {
            let range = Range {
                start: self.pos + (n_read as u64),
                end: buf.len() as u64,
            };
            let lease = Lease::get_read(self.client, &self.path, range)
                .await
                .unwrap();

            tokio::select! {
                _ = lease.hold() => (),
                _ = mock_read(buf, &mut n_read) => return,
            }
        }
    }
}

pub struct WritableFile<'a, T: RandomNode> {
    client: &'a mut Client<T>,
    path: PathBuf,
    pos: u64,
}

impl<'a, T: RandomNode> WritableFile<'a, T> {
    pub fn seek(&mut self, pos: u64) {
        self.pos = pos;
    }

    pub async fn write(&mut self, buf: &[u8]) {
        let mut n_written = 0;
        loop {
            let range = Range {
                start: self.pos + (n_written as u64),
                end: buf.len() as u64,
            };
            let lease = Lease::get_write(self.client, &self.path, range)
                .await
                .unwrap();

            tokio::select! {
                _ = lease.hold() => (),
                _ = mock_write(buf, &mut n_written) => return,
            }
        }
    }
}

impl<T: RandomNode> Client<T> {
    #[allow(dead_code)]
    pub fn new(nodes: T) -> Self {
        Client {
            ticket: None,
            map: Map::default(),
            nodes,
            conn: None,
        }
    }

    /// # Note
    /// Can be canceld, if any request was in progress but not yet
    /// committed the ticket member will be set to allow future resuming,
    /// in case of node failure.
    #[instrument(skip(self))]
    pub async fn create_file(&mut self, path: PathBuf) {
        self.request(&path, Request::Create(path.clone()), true)
            .await
            .unwrap()
    }

    /// # Note
    /// Can be canceld
    pub async fn list(&mut self, path: impl Into<PathBuf>) -> Vec<PathBuf> {
        let mut res = Vec::new();
        let mut checked = HashSet::new();
        let mut to_check = Vec::new();

        let path = path.into();
        let DirList { local, subtrees } = self
            .request(&path, Request::List(path.clone()), false)
            .await
            .unwrap();
        checked.insert(path);
        res.extend_from_slice(&local);
        to_check.extend(subtrees.into_iter());

        while let Some(tree_path) = to_check.pop() {
            let DirList { local, subtrees } = self
                .request(&tree_path, Request::List(tree_path.clone()), false)
                .await
                .unwrap();
            checked.insert(tree_path);
            res.extend_from_slice(&local);

            let subtrees: HashSet<_> = subtrees.into_iter().collect();
            let add = subtrees.difference(&checked).cloned();
            to_check.extend(add);
        }
        res
    }

    pub async fn open_writeable(&mut self, path: PathBuf) -> WritableFile<'_, T> {
        WritableFile {
            client: self,
            path,
            pos: 0,
        }
    }
    pub async fn open_readable(&mut self, path: PathBuf) -> ReadableFile<'_, T> {
        ReadableFile {
            client: self,
            path,
            pos: 0,
        }
    }
}

#[derive(Debug)]
pub enum RequestError {
    Uncommitted,
    NoTicket,
}
