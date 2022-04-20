use std::net::SocketAddr;

use futures::{SinkExt, TryStreamExt};
pub use log::{Log, Order};
use protocol::connection;
use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task;
use tracing::warn;

mod log;
mod state;
use state::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Msg {
    RequestVote(state::RequestVote),
    AppendEntries(state::AppendEntries),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Reply {
    RequestVote(state::VoteReply),
    AppendEntries(state::AppendReply),
}

async fn conn((stream, _source): (TcpStream, SocketAddr), state: State) {
    use Msg::*;
    let mut stream: connection::MsgStream<Msg, Reply> = connection::wrap(stream);
    while let Ok(msg) = stream.try_next().await {
        let reply = match msg {
            None => continue,
            Some(RequestVote(req)) => state.vote_req(req).map(Reply::RequestVote),
            Some(AppendEntries(req)) => state.append_req(req).map(Reply::AppendEntries),
        };

        if let Some(reply) = reply {
            if let Err(e) = stream.send(reply).await {
                warn!("error replying to presidential request");
                return;
            }
        }
    }
}

async fn handle_msg(listener: TcpListener, state: State) {
    loop {
        let res = listener.accept().await;
        if let Err(e) = res {
            warn!("error accepting presidential connection: {e}");
            continue;
        }

        task::spawn(conn(res.unwrap(), state.clone()));
    }
}

async fn succession(state: State) {
    loop {
        // await hb timeout
        // select on:
        //      term incr -> continue
        //      got elected
        //
        // select on:
        //      term incr
        //      newer leader
    }
}
