use std::path::PathBuf;

use protocol::Request;

mod random_node;
mod request;
mod map;

pub use random_node::{RandomNode, ChartNodes};
use request::Connection;
use map::{Map, Ministry};


pub struct Ticket {
    idx: u64,
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
        self.request(&path, Request::CreateFile(path.clone()), true)
            .await
            .unwrap();
    }

}

#[derive(Debug)]
pub enum RequestError {
    Uncommitted,
    NoTicket,
}

