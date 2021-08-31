use crate::server_conn::protocol::{Change, RsMsg, WsMsg};
use client_protocol::connection;
use futures::{SinkExt, TryStreamExt};
use tokio::net::{TcpStream, ToSocketAddrs};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Write meta server disconnected")]
    Io(#[from] std::io::Error),
}

type WsStream = connection::MsgStream<WsMsg, RsMsg>;
pub struct WriteServer {
    conn: WsStream,
}

impl WriteServer {
    pub async fn from_addr(addr: impl ToSocketAddrs) -> Self {
        let stream = TcpStream::connect(addr).await.unwrap();
        Self {
            conn: connection::wrap(stream),
        }
    }

    pub async fn maintain(&mut self) -> Result<(), Error> {
        loop {
            let msg = self.conn.try_next().await?.expect("the write meta server never sends empty msg");

            match msg {
                WsMsg::DirectoryChange(e) => todo!(),
                _ => todo!(),
            }
        }
    }
}
