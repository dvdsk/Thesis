use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Duration;

use crate::president::Chart;
use crate::{Id, Term};
use color_eyre::eyre::eyre;
use futures::{SinkExt, TryStreamExt};
use protocol::connection::{self, MsgStream};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tokio::time::{sleep, sleep_until, timeout_at, Instant};
use tracing::{debug, instrument, trace, warn};

use super::state::append::{self, Request};
use super::state::LogMeta;
use super::{Msg, Order, Reply};
use super::{State, HB_PERIOD};

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

#[instrument(skip_all, fields(president_id = base_msg.leader_id, subject_id = subject_id))]
async fn manage_subject(
    subject_id: Id,
    address: SocketAddr,
    mut broadcast: broadcast::Receiver<Order>,
    mut next_idx: u32,
    _match_idx: Arc<AtomicU32>,
    base_msg: Request,
    state: State,
) {
    loop {
        let mut stream = connect(&address).await;

        let mut next_msg = Request { ..base_msg.clone() };
        // send empty msg aka heartbeat
        if let Err(e) = stream.send(Msg::AppendEntries(next_msg.clone())).await {
            warn!("could not send to host, error: {e:?}");
            continue;
        }

        pass_on_orders(
            &mut broadcast,
            &mut next_msg,
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

fn append(base_msg: Request, state: &State, next_idx: u32) -> Request {
    let prev_entry = state.entry_at(next_idx - 1);
    let entry = state.entry_at(next_idx);
    Request {
        prev_log_idx: next_idx - 1,
        prev_log_term: prev_entry.term,
        entries: vec![entry.order],
        ..base_msg
    }
}

fn heartbeat(base_msg: Request) -> Request {
    Request {
        prev_log_term: base_msg.term,
        entries: Vec::new(),
        ..base_msg
    }
}

async fn recieve_reply(
    stream: &mut MsgStream<Reply, Msg>,
    next_idx: &mut u32,
    base_msg: &mut Request,
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
                base_msg.prev_log_term = base_msg.term;
                base_msg.prev_log_idx = *next_idx;
                *next_idx += 1;
            }

            Ok(Some(Reply::AppendEntries(append::Reply::InconsistentLog))) => *next_idx -= 1,
            Ok(Some(Reply::AppendEntries(append::Reply::ExPresident(new_term)))) => {
                warn!("we are not the current president, new president has term: {new_term}")
            }
        }
    }
}

async fn pass_on_orders(
    _broadcast: &mut broadcast::Receiver<Order>,
    base_msg: &mut Request,
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
            append(base_msg.clone(), state, *next_idx)
        } else {
            trace!("sending heartbeat");
            heartbeat(base_msg.clone())
        };
        trace!("sending msg: {base_msg:?}");

        if let Err(e) = stream.send(Msg::AppendEntries(to_send)).await {
            warn!("did not send to host, error: {e:?}");
            return;
        }

        // recieve for as long as possible,
        match timeout_at(next_hb, recieve_reply(stream, next_idx, base_msg)).await {
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
    state: State,
    term: Term,
) {
    // todo slice of len cluster size for match_idxes
    let LogMeta {
        idx: prev_idx,
        term: prev_term,
    } = state.last_log_meta();

    let base_msg = Request {
        term,
        leader_id: chart.our_id(),
        prev_log_idx: dbg!(prev_idx),
        prev_log_term: dbg!(prev_term),
        entries: Vec::new(),
        leader_commit: state.commit_index(),
    };

    let mut subjects = JoinSet::new();
    let mut add_subject = |id, addr| {
        let rx = orders.subscribe();
        let match_idx = Arc::new(AtomicU32::new(0));
        let next_idx = state.last_applied() + 1;
        let manage = manage_subject(
            id,
            addr,
            rx,
            next_idx,
            match_idx,
            base_msg.clone(),
            state.clone(),
        );
        subjects.spawn(manage);
    };

    let mut notify = chart.notify();
    let mut adresses: HashSet<_> = chart.nth_addr_vec::<0>().into_iter().collect();
    for (id, addr) in adresses.iter().cloned() {
        add_subject(id, addr)
    }

    while let Ok((id, addr)) = notify.recv_nth_addr::<0>().await {
        let is_new = adresses.insert((id, addr));
        if is_new {
            add_subject(id, addr)
        }
    }
}
