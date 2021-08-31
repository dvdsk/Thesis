use futures_util::TryStreamExt;
use tokio::net::TcpListener;
use std::net::IpAddr;
use std::net::Ipv4Addr;

use client_protocol::connection;
use crate::server_conn::protocol::{RsMsg, WsMsg};

pub async fn meta_server(port: u16) {
}

pub async fn cmd_server(port: u16) {
    let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await.unwrap();

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            type RsStream = connection::MsgStream<WsMsg, RsMsg>;
            let mut stream: RsStream = connection::wrap(socket);
            match stream.try_next().await.unwrap() {
                Some(WsMsg::HeartBeat) => (),
                Some(WsMsg::GetServerList) => (),
                Some(WsMsg::DirectoryChange(change)) => (),
                Some(msg) => todo!("not yet handling: {:?}",msg),
                None => panic!("should never recieve empty msg"),
            }
        });
    }
}
