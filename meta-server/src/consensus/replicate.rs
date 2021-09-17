use client_protocol::connection;
use futures::{SinkExt, TryStreamExt};
use std::net::SocketAddr;
use tokio::net::TcpStream;

use super::State;
use crate::directory::readserv::Directory;
use crate::server_conn::protocol::{FromWs, ToWs};

pub async fn update(state: &State, dir: &Directory) {
    loop {
        let addr = match state.get_master() {
            None => continue,
            Some(addr) => addr,
        };

        let update = match get_update(addr).await {
            Err(_) => continue,
            Ok(update) => update,
        };


    }
}

async fn get_update(master: SocketAddr) -> Result<(), ()> {
    type Stream = connection::MsgStream<FromWs, ToWs>;
    let socket = TcpStream::connect(master).await.map_err(|_| ())?;
    let mut stream: Stream = connection::wrap(socket);
    stream.send(ToWs::Sync).await.map_err(|_| ())?;

    match stream.try_next().await {
        Ok(Some(FromWs::Directory(i))) => Ok(i),
        _ => return Err(()),
    }
}
