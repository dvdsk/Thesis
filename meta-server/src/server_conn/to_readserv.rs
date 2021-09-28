use client_protocol::connection;
use futures::SinkExt;
use futures::future::join_all;
use tokio::time::timeout;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::consensus::{State, HB_TIMEOUT};
use crate::server_conn::protocol::{Change, FromRS, ToRs};

type RsStream = connection::MsgStream<FromRS, ToRs>;
type ConnList = Arc<Mutex<Vec<RsStream>>>;

#[derive(Clone, Debug)]
pub struct ReadServers {
    pub conns: ConnList,
}

pub enum PubResult {
    ReachedAll,
    ReachedMajority,
    ReachedMinority,
}

impl ReadServers {
    pub fn new() -> Self {
        Self {
            conns: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn maintain(conns: ConnList, port: u16) {
        let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
        let listener = TcpListener::bind(addr).await.unwrap();

        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let conns = conns.clone();
            tokio::spawn(async move {
                let stream: RsStream = connection::wrap(socket);
                conns.lock_owned().await.push(stream);
            });
        }
    }
    pub async fn publish(&self, state: &State, change: Change) -> PubResult {
        let mut conns = self.conns.lock().await;
        let msg = ToRs::DirectoryChange(state.term(), state.increase_change_idx(), change);
        let requests: Vec<_> = conns
            .iter_mut()
            .map(|c| c.send(msg.clone()))
            .map(|r| timeout(HB_TIMEOUT, r))
            .collect();

        let reached = join_all(requests).await.into_iter()
            .filter_map(Result::ok)
            .filter_map(Result::ok)
            .count();

        let majority = conns.len() / 2;
        if reached == conns.len() {
            PubResult::ReachedAll
        } else if reached > majority {
            PubResult::ReachedMajority
        } else {
            PubResult::ReachedMinority
        }
    }
}
