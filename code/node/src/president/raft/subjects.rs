use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::president::Chart;
use crate::{Id, Term, Idx};
use color_eyre::eyre::eyre;
use futures::{SinkExt, TryStreamExt};
use protocol::connection::{self, MsgStream};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, Notify};
use tokio::task::JoinSet;
use tokio::time::{sleep, sleep_until, timeout_at, Instant};
use tracing::{debug, instrument, trace, warn};

use super::state::append;
use super::{Msg, Order, Reply};
use super::{State, HB_PERIOD};

mod commited;
use commited::Commited;
mod request_gen;
use request_gen::RequestGen;

async fn connect(address: &SocketAddr) -> MsgStream<Reply, Msg> {
    use std::io::ErrorKind;
    loop {
        match TcpStream::connect(address).await {
            Ok(stream) => {
                // stream.set_nodelay(true).unwrap();
                let stream = connection::wrap(stream);
                break stream;
            }
            Err(e) => match e.kind() {
                ErrorKind::ConnectionReset
                | ErrorKind::ConnectionRefused
                | ErrorKind::ConnectionAborted => {
                    sleep(Duration::from_millis(20)).await;
                    continue;
                }
                _ => panic!("unrecoverable error while connecting to subject: {e:?}"),
            },
        }
    }
}

#[instrument(skip_all, fields(president_id = req_gen.base.leader_id, subject_id = subject_id))]
async fn manage_subject(
    subject_id: Id,
    address: SocketAddr,
    mut broadcast: broadcast::Receiver<Order>,
    mut appended: mpsc::Sender<u32>,
    mut next_idx: u32,
    mut req_gen: RequestGen,
    state: State,
) {
    loop {
        let mut stream = connect(&address).await;

        let next_msg = req_gen.heartbeat();

        // send empty msg aka heartbeat
        if let Err(e) = stream.send(Msg::AppendEntries(next_msg)).await {
            warn!("could not send to host, error: {e:?}");
            continue;
        }

        replicate_orders(
            &mut broadcast,
            &mut appended,
            &mut req_gen,
            &mut next_idx,
            &mut stream,
            &state,
        )
        .await;
    }
}

fn missing_logs(state: &State, next_idx: u32) -> bool {
    state.last_log_meta().idx >= next_idx
}

async fn recieve_reply(
    stream: &mut MsgStream<Reply, Msg>,
    next_idx: &mut u32,
    req_gen: &mut RequestGen,
    appended: &mut mpsc::Sender<u32>,
) -> color_eyre::Result<()> {
    loop {
        match stream.try_next().await {
            Ok(None) => return Err(eyre!("did not recieve reply from host")),
            Err(e) => return Err(eyre!("did not recieve reply from host, error: {e:?}")),
            Ok(Some(Reply::RequestVote(..))) => {
                unreachable!("no vote request is ever send on this connection")
            }

            Ok(Some(Reply::AppendEntries(append::Reply::HeartBeatOk))) => (),
            Ok(Some(Reply::AppendEntries(append::Reply::AppendOk))) => {
                req_gen.base.prev_log_term = req_gen.base.term;
                req_gen.base.prev_log_idx = *next_idx;
                appended.send(*next_idx).await.unwrap();
                *next_idx += 1;
            }

            Ok(Some(Reply::AppendEntries(append::Reply::InconsistentLog))) => *next_idx -= 1,
            Ok(Some(Reply::AppendEntries(append::Reply::ExPresident(new_term)))) => {
                warn!("we are not the current president, new president has term: {new_term}")
            }
        }
    }
}

async fn replicate_orders(
    _broadcast: &mut broadcast::Receiver<Order>,
    appended: &mut mpsc::Sender<u32>,
    req_gen: &mut RequestGen,
    next_idx: &mut u32,
    stream: &mut MsgStream<Reply, Msg>,
    state: &State,
) {
    let mut next_hb = Instant::now() + HB_PERIOD;
    sleep_until(next_hb).await;

    loop {
        next_hb = next_hb + HB_PERIOD;

        let to_send = if missing_logs(state, *next_idx) {
            debug!("sending missing logs at idx: {next_idx}");
            req_gen.append(state, *next_idx)
        } else {
            trace!("sending heartbeat");
            req_gen.heartbeat()
        };
        trace!("sending msg: {to_send:?}");

        if let Err(e) = stream.send(Msg::AppendEntries(to_send)).await {
            warn!("did not send to host, error: {e:?}");
            return;
        }

        // recieve for as long as possible,
        match timeout_at(next_hb, recieve_reply(stream, next_idx, req_gen, appended)).await {
            Err(..) => (), // not a problem reply can arrive later
            Ok(Ok(..)) => unreachable!("we always keep recieving"),
            Ok(Err(e)) => {
                warn!("did not recieve reply, error: {e:?}");
                sleep_until(next_hb).await;
            }
        }
    }
}


/// look for new subjects in the chart and register them
#[instrument(skip_all, fields(id = state.id))]
pub async fn instruct(
    chart: &mut Chart,
    orders: broadcast::Sender<Order>,
    notify_rx: mpsc::Receiver<(Idx, Arc<Notify>)>,
    state: State,
    term: Term,
) {
    let commit_idx = state.commit_index();
    let mut commit_idx = Commited::new(commit_idx, notify_rx);
    let base_msg = RequestGen::new(&state, &commit_idx, term, chart);

    let mut subjects = JoinSet::new();
    let mut add_subject = |id, addr, commit_idx: &mut Commited| {
        let broadcast_rx = orders.subscribe();
        let append_updates = commit_idx.track_subject();

        let next_idx = state.last_applied() + 1;
        let manage = manage_subject(
            id,
            addr,
            broadcast_rx,
            append_updates,
            next_idx,
            base_msg.clone(),
            state.clone(),
        );
        subjects.spawn(manage);
    };

    let mut notify = chart.notify();
    let mut adresses: HashSet<_> = chart.nth_addr_vec::<0>().into_iter().collect();
    for (id, addr) in adresses.iter().cloned() {
        add_subject(id, addr, &mut commit_idx)
    }

    loop {
        let recoverd = tokio::select! {
            res = notify.recv_nth_addr::<0>() => res,
            _ = commit_idx.maintain() => unreachable!(),
        };

        let (id, addr) = recoverd.unwrap();
        let is_new = adresses.insert((id, addr));
        if is_new {
            add_subject(id, addr, &mut commit_idx)
        }
    }
}
