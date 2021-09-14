use futures_util::TryStreamExt;
use tokio::net::TcpListener;
use std::net::SocketAddr;
use tokio::sync::mpsc::Sender;
use std::net::IpAddr;
use std::net::Ipv4Addr;

use client_protocol::{Request, Response, connection};
use crate::server_conn::protocol::ElectionMsg;
use crate::server_conn::protocol::{ControlMsg, FromRS, ToRs};

pub async fn meta_server(port: u16) {
    let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await.unwrap();

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            type ReqStream = connection::MsgStream<Request, Response>;
            let mut stream: ReqStream = connection::wrap(socket);
            loop {
                dbg!(stream.try_next().await.unwrap());
            }
        });
    }
}

async fn handle_connection(msg: ControlMsg) {
    use ControlMsg::*;
    match msg {
        GetServerList => todo!(),
        DirectoryChange(change, idx) => todo!("{:?}", change),
        other => todo!("not yet handling: {:?}", other),
    }
}

pub async fn cmd_server(port: u16, tx: Sender<(SocketAddr, ElectionMsg)>) {
    let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await.unwrap();

    loop {
        let (socket, source) = listener.accept().await.unwrap();
        let tx = tx.clone();
        tokio::spawn(async move {
            type RsStream = connection::MsgStream<ToRs, FromRS>;
            let mut stream: RsStream = connection::wrap(socket);
            loop {
                match stream.try_next().await.unwrap() {
                    Some(ToRs::Election(msg)) => tx.send((source, msg)).await.unwrap(),
                    Some(ToRs::Control(msg)) => todo!("handle {:?} here", msg),
                    None => panic!("empty msgs are not allowed"),
                }
            }
        });
    }
}
