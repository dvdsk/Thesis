use std::net::SocketAddr;
use std::time::Duration;

use color_eyre::{eyre, Result};
use futures::{pin_mut, SinkExt, TryStreamExt};
pub use log::{Log, Order};
use protocol::connection;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tokio::time::sleep;
use tracing::{debug, instrument, trace, warn};

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

const MUL: u64 = 5; // goverens duration of most timings, five seems to be the minimum
pub(super) const CONN_RETRY_PERIOD: Duration = Duration::from_millis(20 * MUL);
/// heartbeat timeout
pub(super) const HB_TIMEOUT: Duration = Duration::from_millis(20 * MUL);
pub(super) const HB_PERIOD: Duration = Duration::from_millis(15 * MUL);

pub(super) const ELECTION_TIMEOUT: Duration = Duration::from_millis(40 * MUL);
/// elections are only started after after 0 .. SETUP_TIME
pub(super) const MIN_ELECTION_SETUP: Duration = Duration::from_millis(40 * MUL);
pub(super) const MAX_ELECTION_SETUP: Duration = Duration::from_secs(8);

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
        let reply = match msg.clone() {
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
                // usually this is not a big problem, a president could have
                // resigned, closing the connection
                warn!(
                    "error sending reply to presidents request: \n{req:?}, \n{e:?}",
                    req = msg.unwrap()
                );
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
        let res = if tasks.is_empty() {
            listener.accept().await
        } else {
            // make sure to panic this task if any error
            // occurs in the handlers
            tokio::select! {
                res = listener.accept() => res,
                res = tasks.join_one() => match res {
                    Err(err) => panic!("{err:?}"),
                    Ok(Some(Err(err))) => panic!("{err:?}"),
                    Ok(..) => {
                        trace!("task joined");
                        continue
                    }
                }
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
    let mut rng = rand::rngs::StdRng::from_entropy();
    'outer: loop {
        succession::president_died(&state).await;
        let mut setup_time = Duration::from_secs(0)..MIN_ELECTION_SETUP;

        let term_increased = loop {
            let our_term = state.increment_term().await;
            // watch out for a valid leader indicated by a valid heartbeat
            // we can safely use the heartbeat Notify as the `president_died` is not using it
            let valid_leader_found = state.heartbeat().notified();
            pin_mut!(valid_leader_found);

            // wait a bit to see if another leader pops up
            setup_time.end = Duration::min(setup_time.end.mul_f32(1.5), MAX_ELECTION_SETUP);
            let setup_time = rng.gen_range(setup_time.clone());
            tokio::select! {
                () = sleep(setup_time) => (),
                () = (&mut valid_leader_found) => {
                    debug!("do not start election, valid leader encounterd, our_term: {our_term}");
                    continue 'outer
                }
            }

            if state.vote_for_self(our_term, chart.our_id()).await {
                sleep(ELECTION_TIMEOUT).await;
                continue 'outer;
            }

            // if we are elected president we want to resign as soon
            // as the term is increased that means starting to watch the term
            // before the election is done
            let term_increased = state.watch_term();
            let run_for_office = succession::run_for_office(&state, &chart, cluster_size, our_term);

            tokio::select! {
                () = (&mut valid_leader_found) => {
                    debug!("abort election, valid leader encounterd");
                    continue 'outer
                }
                () = sleep(ELECTION_TIMEOUT) => {
                    debug!("abort election, timeout reached");
                    continue
                }
                res = run_for_office => match res {
                    ElectionResult::Lost => continue,
                    ElectionResult::Won => {
                        state.order(Order::BecomePres{term: our_term}).await;
                        break term_increased;
                    }
                }
            }
        };

        term_increased.await;
        debug!("President saw higher term, resigning");
        state.order(Order::ResignPres).await
    } // outer loop
}
