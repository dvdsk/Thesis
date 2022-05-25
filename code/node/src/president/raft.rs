use std::net::SocketAddr;
use std::time::Duration;

use futures::{pin_mut, SinkExt, TryStreamExt};
pub use log::{Log, Order};
use protocol::connection;
use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tokio::time::sleep;
use tracing::{debug, instrument, warn};

mod log;
mod state;
pub mod subjects;
mod succession;
#[cfg(test)]
mod tests;

pub use state::State;
pub use state::LogEntry;

use succession::ElectionResult;

use self::state::{append, vote};

use super::Chart;

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Msg {
    RequestVote(vote::RequestVote),
    AppendEntries(append::Request),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Reply {
    RequestVote(vote::VoteReply),
    AppendEntries(append::Reply),
}

#[instrument(skip(state, stream))]
async fn handle_conn((stream, _source): (TcpStream, SocketAddr), state: State) {
    use Msg::*;

    let mut stream: connection::MsgStream<Msg, Reply> = connection::wrap(stream);
    while let Ok(msg) = stream.try_next().await {
        let reply = match msg {
            None => continue,
            Some(RequestVote(req)) => state.vote_req(req).map(Reply::RequestVote),
            Some(AppendEntries(req)) => {
                let append_reply = state.append_req(req).await;
                Some(Reply::AppendEntries(append_reply))
            }
        };

        if let Some(reply) = reply {
            if let Err(e) = stream.send(reply).await {
                warn!("error replying to presidential request: {e:?}");
                return;
            }
        }
    }
}

#[instrument(skip_all, fields(id))]
async fn handle_incoming(listener: TcpListener, state: State) {
    let mut tasks = JoinSet::new();
    loop {
        let res = listener.accept().await;
        if let Err(e) = res {
            warn!("error accepting presidential connection: {e}");
            continue;
        }
        let fut = handle_conn(res.unwrap(), state.clone());
        tasks.build_task().name("handle connection").spawn(fut);
    }
}

pub(super) const HB_TIMEOUT: Duration = Duration::from_millis(200);
pub(super) const HB_PERIOD: Duration = Duration::from_millis(150);
pub(super) const ELECTION_TIMEOUT: Duration = Duration::from_millis(200);

#[instrument(skip_all, fields(id = chart.our_id()))]
async fn succession(chart: Chart, cluster_size: u16, state: State) {
    loop {
        succession::president_died(&state).await;
        let our_term = state.increment_term();

        let meta = state.last_log_meta();
        let campaign = vote::RequestVote {
            term: our_term,
            candidate_id: chart.our_id(),
            last_log_term: meta.term,
            last_log_idx: meta.idx,
        };
        let get_elected = succession::run_for_office(&chart, cluster_size, campaign);
        let election_timeout = sleep(ELECTION_TIMEOUT);
        let term_increased = state.watch_term();
        pin_mut!(term_increased);
        tokio::select! {
            () = (&mut term_increased) => {
                debug!("abort election, recieved higher term");
                continue
            }
            () = election_timeout => {
                debug!("abort election, timeout reached");
                continue
            }
            res = get_elected => match res {
                ElectionResult::Lost => continue,
                ElectionResult::Won => {
                    dbg!("ordering victory");
                    state.order(Order::BecomePres{term: our_term}).await
                }
            }
        }

        term_increased.await; // if we get here we are the
        debug!("President saw higher term, resigning");
        state.order(Order::ResignPres).await
    }
}
