use client_protocol::connection;
use futures::{SinkExt, TryStreamExt};
use std::net::IpAddr;
use tokio::net::TcpStream;

use super::State;
use crate::directory::readserv::Directory;
use crate::server_conn::protocol::{FromWs, ToWs};

#[tracing::instrument(skip(state, dir))]
pub async fn update(state: &State, dir: &Directory) {
    loop {
        let addr = match state.get_master() {
            None => continue,
            Some(addr) => addr,
        };

        let update = match get_update(addr, state.config.control_port).await {
            Err(_) => continue,
            Ok(update) => update,
        };

        dir.update_from_master(&update).await;
        break
    }
}

#[tracing::instrument]
async fn get_update(master: IpAddr, port: u16) -> Result<Vec<u8>, ()> {
    type Stream = connection::MsgStream<FromWs, ToWs>;
    let socket = TcpStream::connect((master, port)).await.map_err(|_| ())?;
    let mut stream: Stream = connection::wrap(socket);
    stream.send(ToWs::Sync).await.map_err(|_| ())?;

    match stream.try_next().await {
        Ok(Some(FromWs::Directory(i))) => Ok(i),
        _ => return Err(()),
    }
}
