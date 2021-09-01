use futures_util::TryStreamExt;
use tokio::net::TcpListener;
use tokio::sync::mpsc::Sender;
use std::net::IpAddr;
use std::net::Ipv4Addr;

use client_protocol::{Request, Response, connection};
use crate::server_conn::protocol::ElectionMsg;
use crate::server_conn::protocol::{ControlMsg, ToWs, ToRs};

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
        DirectoryChange(change) => todo!(),
        other => todo!("not yet handling: {:?}", other),
    }
}


pub async fn cmd_server(port: u16, tx: Sender<ElectionMsg>) {
    let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await.unwrap();

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            type RsStream = connection::MsgStream<ToRs, ToWs>;
            let mut stream: RsStream = connection::wrap(socket);
            loop {
                match stream.try_next().await.unwrap() {
                    Some(ToRs::Election(msg)) => todo!("send to election cycle"),
                    Some(ToRs::Control(msg)) => todo!("handle here"),
                    None => panic!("empty msgs are not allowed"),
                }
            }
        });
    }
}
