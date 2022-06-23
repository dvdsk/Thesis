use std::ops::Range;
use std::path::PathBuf;
use std::time::Duration;

use protocol::Request;

mod lease;
mod map;
mod random_node;
mod request;

use lease::Lease;
use map::{Map, Ministry};
pub use random_node::{ChartNodes, RandomNode};

use request::Connection;
use tokio::time::{sleep, timeout};
use tracing::instrument;

const CLIENT_TIMEOUT: Duration = Duration::from_millis(500);

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
            let lease = Lease::get_read(self.client, &self.path, range);
            let lease: Lease<_> = timeout(CLIENT_TIMEOUT, lease)
                .await
                .expect("client timed out")
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
            let lease = Lease::get_write(self.client, &self.path, range);
            let lease: Lease<_> = timeout(CLIENT_TIMEOUT, lease)
                .await
                .expect("client timed out")
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
    pub async fn create_file(&mut self, path: PathBuf) {
        let create = self.request(&path, Request::Create(path.clone()), true);
        timeout(CLIENT_TIMEOUT, create)
            .await
            .expect("client timed out")
            .unwrap()
    }

    /// # Note
    /// Can be canceld, if any request was in progress but not yet
    /// committed the ticket member will be set to allow future resuming,
    /// in case of node failure.
    pub async fn list(&mut self, path: PathBuf) -> Vec<PathBuf> {
        let list = self.request(&path, Request::List(path.clone()), false);
        timeout(CLIENT_TIMEOUT, list)
            .await
            .expect("client timed out")
            .unwrap()
    }

    pub async fn open_writeable<'a>(&'a mut self, path: PathBuf) -> WritableFile<'a, T> {
        WritableFile {
            client: self,
            path,
            pos: 0,
        }
    }
    pub async fn open_readable<'a>(&'a mut self, path: PathBuf) -> ReadableFile<'a, T> {
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
