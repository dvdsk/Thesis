use client_protocol::connection;
use futures::SinkExt;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::consensus::State;
use crate::server_conn::protocol::{FromRS,ToRs,Change};

type RsStream = connection::MsgStream<FromRS, ToRs>;
type ConnList = Arc<Mutex<Vec<RsStream>>>;

#[derive(Clone, Debug)]
pub struct ReadServers {
    pub conns: ConnList,
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
    pub async fn publish(&self, state: &State, change: Change) {
        let mut conns = self.conns.lock().await;
        let msg = ToRs::DirectoryChange(state.term(), state.change_idx(), change);
        for conn in &mut *conns {
            conn.send(msg.clone()).await.unwrap();
        }
    }
}
