use core::fmt;
use std::net::SocketAddr;
use std::time::Duration;

use color_eyre::{eyre, Result};
use futures::{pin_mut, TryStreamExt, SinkExt};
use protocol::connection;
use rand::{Rng, SeedableRng};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tokio::time::sleep;
use tracing::{debug, instrument, trace, warn, Instrument};

mod log;
mod log_writer;
mod state;
pub mod subjects;
mod succession;

#[cfg(test)]
mod tests;

pub use log::{Log, ObserverLog};
pub use log_writer::LogWriter;
pub use state::LogEntry;
pub use state::State;

use crate::Term;

use self::state::{append, vote};
use super::Chart;
use succession::ElectionResult;

// WARNING these should be synced with protocol
const MUL: u64 = 5; // goverens duration of most timings, five seems to be the minimum
pub(super) const CONN_RETRY_PERIOD: Duration = Duration::from_millis(20 * MUL);
/// heartbeat timeout
pub(super) const HB_TIMEOUT: Duration = Duration::from_millis(20 * MUL);
pub(super) const HB_PERIOD: Duration = Duration::from_millis(15 * MUL);

pub(super) const ELECTION_TIMEOUT: Duration = Duration::from_millis(40 * MUL);
/// elections are only started after after 0 .. SETUP_TIME
pub(super) const MIN_ELECTION_SETUP: Duration = Duration::from_millis(40 * MUL);
pub(super) const MAX_ELECTION_SETUP: Duration = Duration::from_secs(8);
/// time allowed to try and (re-)connect to a node before giving up
pub(super) const SUBJECT_CONN_TIMEOUT: Duration = Duration::from_secs(8); 

pub trait Order:
    Serialize + DeserializeOwned + fmt::Debug + Clone + Send + Sync + Unpin + 'static + PartialEq
{
    fn elected(term: Term) -> Self;
    fn resign() -> Self;
    fn none() -> Self;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Msg<O> {
    RequestVote(vote::RequestVote),
    AppendEntries(append::Request<O>),
    CriticalError(()),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Reply {
    RequestVote(vote::VoteReply),
    AppendEntries(append::Reply),
}

#[instrument(skip(state, stream))]
async fn handle_conn<O: Order>(
    (stream, _source): (TcpStream, SocketAddr),
    state: State<O>,
) -> Result<()> {
    use Msg::*;

    let mut stream: connection::MsgStream<Msg<O>, Reply> = connection::wrap(stream);
    while let Ok(msg) = stream.try_next().await {
        let reply = match msg.clone() {
            None => {
                debug!("Connection closed");
                break
            }
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

#[instrument(skip_all)]
async fn handle_incoming<O: Order>(listener: TcpListener, state: State<O>) {
    let mut tasks = JoinSet::new();
    loop {
        let res = if tasks.is_empty() {
            listener.accept().await
        } else {
            // make sure to panic this task if any error
            // occurs in the handlers
            tokio::select! {
                res = listener.accept() => res,
                res = tasks.join_one() => match res.expect("tasks should not be empty") {
                    Err(err) => panic!("conn handler paniced: {err:?}"),
                    Ok(Err(err)) => panic!("conn handler errored: {err:?}"),
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
        let fut = handle_conn(res.unwrap(), state.clone()).in_current_span(); //, Span::current().clone());
        tasks.build_task().name("handle connection").spawn(fut);
    }
}

#[instrument(skip_all)]
async fn succession<O: Order>(chart: Chart, cluster_size: u16, state: State<O>) {
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
                    debug!("not starting election, valid leader encounterd, our_term: {our_term}");
                    continue 'outer
                }
            }

            if !state.vote_for_self(our_term, chart.our_id()).await {
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
                    debug!("aborting election, valid leader encounterd");
                    continue 'outer
                }
                () = sleep(ELECTION_TIMEOUT) => {
                    debug!("aborting election, timeout reached");
                    continue
                }
                res = run_for_office => match res {
                    ElectionResult::Lost => continue,
                    ElectionResult::Won => {
                        state.order(Order::elected(our_term)).await;
                        break term_increased;
                    }
                }
            }
        };

        term_increased.await;
        debug!("President saw higher term, resigning");
        state.order(Order::resign()).await
    } // outer loop
}
