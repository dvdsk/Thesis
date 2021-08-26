use protocol::connection;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use futures::{SinkExt, TryStreamExt};

enum RsMsg {}

enum WsMsg {
    DirectoryChange(Change),
    Test,
}

pub type ServerStream = connection::MsgStream<RsMsg, WsMsg>;
pub type ConnList = Arc<Mutex<Vec<ServerStream>>>;
pub struct ReadServers {
    conns: ConnList,
}

pub enum Change {}

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
                let mut stream: ServerStream = connection::wrap(socket);
                stream.send(WsMsg::Test);
                conns.lock_owned().await.push(stream);
            });
        }
    }
    pub async fn publish(&self, change: Change) {
        let mut conns = self.conns.lock().await;
        for conn in &mut*conns {
            // conn.send(WsMsg::DirectoryChange(change));
        }
    }
}
