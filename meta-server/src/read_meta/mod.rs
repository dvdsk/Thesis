use client_protocol::ServerList;
use discovery::Chart;
use futures::SinkExt;
use futures_util::TryStreamExt;
use tracing::error;
use tracing::trace;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

use crate::consensus::State;
use crate::directory::readserv::Directory;
use crate::server_conn::protocol::{FromRS, ToRs};
use client_protocol::{connection, Request, Response};

fn serverlist(chart: &Chart, state: &State) -> ServerList {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let addresses: Vec<IpAddr> = chart
        .adresses()
        .into_iter()
        .map(|sock| sock.ip())
        .collect();

    let idx = rng.gen_range(0..addresses.len());

    ServerList {
        port: state.config.client_port,
        write_serv: state.get_master(),
        read_serv: Some(addresses[idx]),
        fallback: addresses,
    }
}

type ReqStream = connection::MsgStream<Request, Response>;
#[tracing::instrument(level = "debug", skip(stream, dir, chart, state))]
async fn client_msg(
    stream: &mut ReqStream,
    msg: Request,
    dir: &Directory,
    chart: &Chart,
    state: &State,
) {
    use Request::*;
    match msg {
        Ls(path) => {
            let resp = Response::Ls(dir.ls(path));
            let _res = stream.send(resp).await;
        }
        AddDir(_) | OpenAppend(_, _) | OpenReadWrite(_, _) => {
            let list = serverlist(chart, state);
            let resp = Response::NotWriteServ(list);
            let _res = stream.send(resp).await;
            trace!("send serverlist to client asking for write server");
        }
        _req => todo!("req: {:?}", _req),
    }
}

#[tracing::instrument(level = "debug", skip(stream, dir, chart, state))]
async fn client_conn(mut stream: ReqStream, dir: &Directory, chart: &Chart, state: &State) {
    while let Ok(msg) = stream.try_next().await {
        let msg = match msg {
            None => continue,
            Some(msg) => msg,
        };
        client_msg(&mut stream, msg, dir, chart, state).await;
    }
}

pub async fn meta_server(port: u16, dir: &Directory, chart: &Chart, state: &Arc<State>) {
    let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr)
        .await
        .expect("can not listen for client meta requests");

    loop {
        let dir = dir.clone();
        let chart = chart.clone();
        let state = state.clone();
        let (socket, _) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            let stream: ReqStream = connection::wrap(socket);
            client_conn(stream, &dir, &chart, &state).await;
        });
    }
}

type RsStream = connection::MsgStream<ToRs, FromRS>;
#[tracing::instrument(level = "debug", skip(stream, dir, state))]
async fn cmd_msg(
    stream: &mut RsStream,
    source: SocketAddr,
    msg: ToRs,
    dir: &Directory,
    state: &State,
) {
    use ToRs::*;
    match msg {
        HeartBeat(term, change_idx) => state.handle_heartbeat(term, change_idx, source),
        RequestVote(term, change_idx, id) => {
            let reply = state.handle_votereq(term, change_idx, id);
            let _ignore_res = stream.send(reply).await;
        }
        DirectoryChange(term, change_idx, change) => {
            if let Err(e) = state.handle_dirchange(term, change_idx, source) {
                error!("error applying dir change: {:?}",e);
                let _ignore_res = stream.send(FromRS::Error).await;
            }
            // consensus: this crashes the readserv on fail never leaving us in an invalide state
            dir.apply(change, change_idx).await; 
            let _ignore_res = stream.send(FromRS::Awk).await;
        }
    }
}

#[tracing::instrument(level = "debug", skip(stream, dir, state))]
async fn cmd_conn(mut stream: RsStream, source: SocketAddr, state: &State, dir: &Directory) {
    while let Ok(msg) = stream.try_next().await {
        let msg = match msg {
            None => continue,
            Some(msg) => msg,
        };
        cmd_msg(&mut stream, source, msg, dir, state).await;
    }
}

pub async fn cmd_server(port: u16, state: Arc<State>, dir: &Directory) {
    let addr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = TcpListener::bind(addr).await.unwrap();

    loop {
        let state = state.clone();
        let dir = dir.clone();
        let (socket, source) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            let stream: RsStream = connection::wrap(socket);
            cmd_conn(stream, source, &state, &dir).await;
        });
    }
}
