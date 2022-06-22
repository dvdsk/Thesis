use std::path::PathBuf;
use std::time::Duration;

use protocol::{Lease, Request};

mod map;
mod random_node;
mod request;

use map::{Map, Ministry};
pub use random_node::{ChartNodes, RandomNode};
use request::Connection;
use tokio::time::{sleep, timeout};

pub struct Ticket {
    idx: protocol::Idx,
    needs_minister: bool,
    path: PathBuf,
}

// TODO instance chart for same cluster discovery?
pub struct Client<T: RandomNode> {
    pub ticket: Option<Ticket>,
    map: Map,
    nodes: T,
    conn: Option<Connection>,
}

pub struct ReadableFile<'a, T: RandomNode> {
    client: &'a mut Client<T>,
    path: PathBuf,
    pos: u64,
    n_read: u64,
}

impl<'a, T: RandomNode> ReadableFile<'a, T> {
    pub fn seek(&mut self, pos: u64) {
        self.pos = pos;
    }

    async fn mock_read(&mut self, buf: &mut [u8]) {
        const BLOCK_DUR: u64 = 50; //milli seconds
        const READ_SPEED: u64 = 500_000_000 / 1000; // 500 mb/s in bytes per millisec
        const BLOCK_SIZE: u64 = READ_SPEED * BLOCK_DUR;
        loop {
            // simulate reading a block by sleeping
            let left = buf.len() as u64 - self.n_read;
            let block = left.min(BLOCK_SIZE);
            let read_dur = block / READ_SPEED;

            sleep(Duration::from_millis(read_dur)).await;
            self.n_read += block;
        }
    }

    pub async fn read(&mut self, buf: &mut [u8]) {
        self.n_read = 0;

        loop {
            let lease: Lease = self
                .client
                .request(
                    &self.path,
                    Request::Read {
                        path: self.path.clone(),
                        range: self.pos..buf.len() as u64,
                    },
                    false,
                )
                .await
                .unwrap();

            let time_left = lease.expires_in();
            match timeout(time_left, self.mock_read(buf)).await {
                Err(_) => (),    // timeout
                Ok(_) => return, // read done
            };
        }
    }
}

pub struct WritableFile<'a, T: RandomNode> {
    client: &'a mut Client<T>,
    path: PathBuf,
    pos: u64,
    n_read: u64
}

impl<'a, T: RandomNode> WritableFile<'a, T> {
    pub fn seek(&mut self, pos: u64) {
        self.pos = pos;
    }

    async fn mock_write(&mut self, buf: &[u8]) {
        const BLOCK_DUR: u64 = 50; //milli seconds
        const WRITE_SPEED: u64 = 200_000_000 / 1000; // 200 mb/s in bytes per millisec
        const BLOCK_SIZE: u64 = WRITE_SPEED * BLOCK_DUR;
        loop {
            // simulate reading a block by sleeping
            let left = buf.len() as u64 - self.n_read;
            let block = left.min(BLOCK_SIZE);
            let read_dur = block / WRITE_SPEED;

            sleep(Duration::from_millis(read_dur)).await;
            self.n_read += block;
        }
    }

    pub async fn write(&mut self, buf: &[u8]) {
        loop {
            let lease: Lease = self
                .client
                .request(
                    &self.path,
                    Request::Read {
                        path: self.path.clone(),
                        range: self.pos..buf.len() as u64,
                    },
                    true,
                )
                .await
                .unwrap();

            let time_left = lease.expires_in();
            match timeout(time_left, self.mock_write(buf)).await {
                Err(_) => (),    // timeout
                Ok(_) => return, // read done
            };
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
    pub async fn create_file(&mut self, path: PathBuf) {
        self.request(&path, Request::Create(path.clone()), true)
            .await
            .unwrap()
    }

    /// # Note
    /// Can be canceld, if any request was in progress but not yet
    /// committed the ticket member will be set to allow future resuming,
    /// in case of node failure.
    pub async fn list(&mut self, path: PathBuf) -> Vec<PathBuf> {
        self.request(&path, Request::List(path.clone()), false)
            .await
            .unwrap()
    }

    pub async fn open_writeable<'a>(&'a mut self, path: PathBuf) -> WritableFile<'a, T> {
        WritableFile {
            client: self,
            path,
            pos: 0,
            n_read: 0,
        }
    }
    pub async fn open_readable<'a>(&'a mut self, path: PathBuf) -> ReadableFile<'a, T> {
        ReadableFile {
            client: self,
            path,
            pos: 0,
            n_read: 0,
        }
    }
}

#[derive(Debug)]
pub enum RequestError {
    Uncommitted,
    NoTicket,
}
