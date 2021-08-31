use client_protocol::connection;
use futures::SinkExt;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::server_conn::protocol::{RsMsg,WsMsg,Change};

type RsStream = connection::MsgStream<RsMsg, WsMsg>;
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
    pub async fn publish(&self, change: Change) {
        let mut conns = self.conns.lock().await;
        for conn in &mut *conns {
            conn.send(WsMsg::DirectoryChange(change.clone())).await.unwrap();
        }
    }
}
