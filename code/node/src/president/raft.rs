use std::net::SocketAddr;
use std::time::Duration;

use color_eyre::{eyre, Result};
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

pub use state::LogEntry;
pub use state::State;

use self::state::{append, vote};
use super::Chart;
use succession::ElectionResult;

const MUL: u64 = 5;
pub(super) const CONN_RETRY_PERIOD: Duration = Duration::from_millis(20 * MUL);
pub(super) const HB_TIMEOUT: Duration = Duration::from_millis(20 * MUL);
pub(super) const HB_PERIOD: Duration = Duration::from_millis(15 * MUL);
pub(super) const ELECTION_TIMEOUT: Duration = Duration::from_millis(20 * MUL);

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Msg {
    RequestVote(vote::RequestVote),
    AppendEntries(append::Request),
    CriticalError(()),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Reply {
    RequestVote(vote::VoteReply),
    AppendEntries(append::Reply),
}

#[instrument(skip(state, stream), fields(id=state.id))]
async fn handle_conn((stream, _source): (TcpStream, SocketAddr), state: State) -> Result<()> {
    use Msg::*;

    let mut stream: connection::MsgStream<Msg, Reply> = connection::wrap(stream);
    while let Ok(msg) = stream.try_next().await {
        let reply = match msg {
            None => continue,
            Some(CriticalError(err)) => {
                return Err(eyre::eyre!(
                    "critical error happend elsewhere in the cluster: {err:?}"
                ))
            }
            Some(RequestVote(req)) => state.vote_req(req).await.map(Reply::RequestVote),
            Some(AppendEntries(req)) => {
                let append_reply = state.append_req(req).await?;
                Some(Reply::AppendEntries(append_reply))
            }
        };

        if let Some(reply) = reply {
            if let Err(e) = stream.send(reply).await {
                warn!("error replying to presidential request: {e:?}");
                return Ok(());
            }
        }
    }
    Ok(())
}

#[instrument(skip_all, fields(id=state.id))]
async fn handle_incoming(listener: TcpListener, state: State) {
    let mut tasks = JoinSet::new();
    loop {
        // make sure to panic this task if any error
        // occurs in the handlers
        let res = tokio::select! {
            res = listener.accept() => res,
            res = tasks.join_one() => match res {
                Err(err) => panic!("{err:?}"),
                Ok(Some(Err(err))) => panic!("{err:?}"),
                Ok(..) => continue,
            }
        };

        if let Err(e) = res {
            warn!("error accepting presidential connection: {e}");
            continue;
        }
        let fut = handle_conn(res.unwrap(), state.clone());
        tasks.build_task().name("handle connection").spawn(fut);
    }
}

#[instrument(skip_all, fields(id = chart.our_id()))]
async fn succession(chart: Chart, cluster_size: u16, state: State) {
    loop {
        succession::president_died(&state).await;
        let our_term = state.increment_term().await;

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
        // at this point we can safely use the
        // heartbeat Notify as the president_died is not using it
        let term_matched = state.heartbeat().notified();
        pin_mut!(term_matched);

        tokio::select! {
            () = (&mut term_matched) => {
                debug!("abort election, valid leader encounterd");
                continue
            }
            () = election_timeout => {
                debug!("abort election, timeout reached");
                continue
            }
            res = get_elected => match res {
                ElectionResult::Lost => continue,
                ElectionResult::Won => {
                    state.order(Order::BecomePres{term: our_term}).await
                }
            }
        }

        term_increased.await;
        debug!("President saw higher term, resigning");
        state.order(Order::ResignPres).await
    }
}
