use futures::SinkExt;
use futures_util::TryStreamExt;
use tokio::net::TcpListener;
use std::net::SocketAddr;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::sync::Arc;

use client_protocol::{Request, Response, connection};
use crate::consensus::State;
use crate::directory::readserv::Directory;
use crate::server_conn::protocol::{FromRS, ToRs};

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

type RsStream = connection::MsgStream<ToRs, FromRS>;
async fn handle_conn(mut stream: RsStream, source: SocketAddr, state: &'_ State<'_>) {
    use ToRs::*;
    while let Some(msg) = stream.try_next().await.unwrap() {
        match msg {
            HeartBeat(term, change_idx) => {

            },
            RequestVote(term, change_idx) => todo!(),
            DirectoryChange(term, change_idx) => todo!(),
        }
    }
    panic!("empty msgs are not allowed");

}

pub async fn cmd_server(port: u16, state: &'_ State<'_>, dir: &Directory) {
    let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await.unwrap();
    let state = Arc::new(state);

    loop {
        let state = state.clone();
        let (socket, source) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            let stream: RsStream = connection::wrap(socket);
            handle_conn(stream, source, &state).await;
        });
    }
}
