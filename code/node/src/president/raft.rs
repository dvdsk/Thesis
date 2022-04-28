use std::net::SocketAddr;
use std::time::Duration;

use futures::{pin_mut, SinkExt, TryStreamExt};
pub use log::{Log, Order};
use protocol::connection;
use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::{self, JoinSet};
use tokio::time::sleep;
use tracing::warn;

mod log;
mod state;
mod succession;
pub(super) use state::State;

use super::Chart;

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

async fn handle_conn((stream, _source): (TcpStream, SocketAddr), state: State) {
    use Msg::*;
    let mut stream: connection::MsgStream<Msg, Reply> = connection::wrap(stream);
    while let Ok(msg) = stream.try_next().await {
        let reply = match msg {
            None => continue,
            Some(RequestVote(req)) => state.vote_req(req).map(Reply::RequestVote),
            Some(AppendEntries(req)) => Some(Reply::AppendEntries(state.append_req(req))),
        };

        if let Some(reply) = reply {
            if let Err(e) = stream.send(reply).await {
                warn!("error replying to presidential request: {e:?}");
                return;
            }
        }
    }
}

async fn handle_incoming(listener: TcpListener, state: State) {
    let mut tasks = JoinSet::new();
    loop {
        let res = listener.accept().await;
        if let Err(e) = res {
            warn!("error accepting presidential connection: {e}");
            continue;
        }
        tasks.spawn(handle_conn(res.unwrap(), state.clone()));
    }
}

async fn succession(chart: Chart, cluster_size: u16, state: State) {
    loop {
        succession::president_died(state.heartbeat()).await;
        let term = state.increment_term();

        let meta = state.last_log_meta();
        let campaign = state::RequestVote {
            term,
            candidate_id: chart.our_id(),
            last_log_term: meta.term,
            last_log_idx: meta.idx,
        };
        let get_elected =
            succession::run_for_office(&chart, cluster_size, campaign);
        let election_timeout = sleep(Duration::from_millis(100));
        let term_increased = state.watch_term();
        pin_mut!(term_increased);
        tokio::select! {
            _n = (&mut term_increased) => continue,
            () = election_timeout => continue,
            () = get_elected => state.order(Order::BecomePres).await,
        }

        term_increased.await; // if we get here we are the president
        state.order(Order::ResignPres).await
    }
}
