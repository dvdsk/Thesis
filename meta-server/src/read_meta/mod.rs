use futures::SinkExt;
use futures_util::TryStreamExt;
use tracing::info;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

use crate::consensus::State;
use crate::directory::readserv::Directory;
use crate::server_conn::protocol::{FromRS, ToRs};
use client_protocol::{connection, Request, Response};
use tracing_futures::Instrument as _;

pub async fn meta_server(port: u16) {
    let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr)
        .await
        .expect("can not listen for client meta requests");

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
#[tracing::instrument]
async fn handle_conn(mut stream: RsStream, source: SocketAddr, state: &State, dir: &Directory) {
    use ToRs::*;
    while let Some(msg) = stream.try_next().await.unwrap() {
        info!("got msg");
        match msg {
            HeartBeat(term, change_idx) => state.handle_heartbeat(term, change_idx, source),
            RequestVote(term, change_idx) => {
                let reply = state.handle_votereq(term, change_idx);
                let _ignore_res = stream.send(reply).await;
            }
            DirectoryChange(term, change_idx, change) => {
                if let Err(_) = state.handle_dirchange(term, change_idx, source) {
                    let _ignore_res = stream.send(FromRS::Error).await;
                }
                dir.apply(change).await;
            }
        }
    }
    panic!("empty msgs are not allowed");
}

pub async fn cmd_server(port: u16, state: Arc<State>, dir: &Directory) {
    let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await.unwrap();

    loop {
        let state = state.clone();
        let dir = dir.clone();
        let (socket, source) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            info!("accepted connection from: {:?}", source);
            let stream: RsStream = connection::wrap(socket);
            handle_conn(stream, source, &state, &dir).await;
        });
    }
}
