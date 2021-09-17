use client_protocol::connection;
use futures::{SinkExt, TryStreamExt};
use std::net::SocketAddr;
use tokio::net::TcpStream;

use super::State;
use crate::directory::readserv::Directory;
use crate::server_conn::protocol::{ChangeIdx, FromRS, ToRs};

// type RsStream = connection::MsgStream<FromRS, ToRs>;
// async fn request_ci(addr: SocketAddr, change_idx: u64) -> Option<ChangeIdx> {
//     let socket = TcpStream::connect(addr).await.ok()?;
//     let mut stream: RsStream = connection::wrap(socket);
//     stream.send(ToRs::GetChangeIdx).await.ok()?;

//     match stream.try_next().await {
//         Ok(Some(FromRS::ChangeIdx(i))) => Some(i),
//         _ => return None,
//     }
// }

pub async fn update(_state: &State, _dir: &Directory) {
    // let geather_change_idx = chart
    //     .map
    //     .iter()
    //     .map(|m| m.value().clone())
    //     .map(|addr| request_ci(addr, state.change_idx));

    // let cluster_idices: HashMap<ChangeIdx, usize> = futures::future::join_all(geather_change_idx)
    //     .await
    //     .into_iter()
    //     .filter_map(|e| e)
    //     .fold(HashMap::new(), |mut m, idx| {
    //         *m.entry(idx).or_default() += 1;
    //         m
    //     });
}

pub async fn handle_update(state: &State, dir: &mut Directory) {}
