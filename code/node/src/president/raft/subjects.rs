use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::president::Chart;
use crate::{Id, Term};
use color_eyre::eyre::eyre;
use futures::{stream, SinkExt, TryStreamExt};
use protocol::connection::{self, MsgStream};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc};
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

#[instrument(skip_all, fields(president_id = base_msg.base_req.leader_id, subject_id = subject_id))]
async fn manage_subject(
    subject_id: Id,
    address: SocketAddr,
    mut broadcast: broadcast::Receiver<Order>,
    mut appended: mpsc::Sender<u32>,
    mut next_idx: u32,
    mut base_msg: RegGen,
    state: State,
) {
    loop {
        let mut stream = connect(&address).await;

        let next_msg = base_msg.heartbeat();

        // send empty msg aka heartbeat
        if let Err(e) = stream.send(Msg::AppendEntries(next_msg)).await {
            warn!("could not send to host, error: {e:?}");
            continue;
        }

        pass_on_orders(
            &mut broadcast,
            &mut appended,
            &mut base_msg,
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

#[derive(Debug, Clone)]
struct RegGen {
    commit_idx: Arc<AtomicU32>,
    base_req: Request,
}

impl RegGen {
    pub fn heartbeat(&self) -> Request {
        Request {
            leader_commit: self.commit_idx.load(Ordering::Relaxed),
            prev_log_term: self.base_req.term,
            entries: Vec::new(),
            ..self.base_req
        }
    }

    fn append(&self, state: &State, next_idx: u32) -> Request {
        let prev_entry = state.entry_at(next_idx - 1);
        let entry = state.entry_at(next_idx);
        Request {
            leader_commit: self.commit_idx.load(Ordering::Relaxed),
            prev_log_idx: next_idx - 1,
            prev_log_term: prev_entry.term,
            entries: vec![entry.order],
            ..self.base_req
        }
    }
}

async fn recieve_reply(
    stream: &mut MsgStream<Reply, Msg>,
    next_idx: &mut u32,
    reg_gen: &mut RegGen,
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
                reg_gen.base_req.prev_log_term = reg_gen.base_req.term;
                reg_gen.base_req.prev_log_idx = *next_idx;
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

async fn pass_on_orders(
    _broadcast: &mut broadcast::Receiver<Order>,
    appended: &mut mpsc::Sender<u32>,
    req_gen: &mut RegGen,
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

struct Update {
    stream_id: usize,
    appended: u32,
}

struct CommitIdx<'a> {
    streams: stream::SelectAll<stream::BoxStream<'a, Update>>,
    highest: Vec<u32>, // index = stream_id
    pub commit_idx: Arc<AtomicU32>,
}

impl<'a> CommitIdx<'a> {
    fn new(commit_idx: u32) -> Self {
        Self {
            streams: stream::SelectAll::new(),
            highest: Vec::new(),
            commit_idx: Arc::new(AtomicU32::new(commit_idx)),
        }
    }

    fn track_subject(&mut self) -> mpsc::Sender<u32> {
        let (tx, append_updates) = mpsc::channel(1);
        let stream_id = self.streams.len();
        let stream = Box::pin(stream::unfold(append_updates, move |mut rx| async move {
            let appended = rx.recv().await.unwrap();
            let yielded = Update {
                stream_id,
                appended,
            };
            Some((yielded, rx))
        }));
        self.streams.push(stream);
        self.highest.push(0);
        tx
    }

    fn majority_appended(&self) -> u32 {
        let majority = crate::util::div_ceil(self.highest.len(), 2);

        *self
            .highest
            .iter()
            .enumerate()
            .map(|(i, candidate)| {
                let n_higher = self.highest[i..]
                    .iter()
                    .filter(|idx| *idx > candidate)
                    .count();
                (n_higher, candidate)
            })
            .filter(|(n, _)| *n > majority)
            .map(|(_, idx)| idx)
            .max()
            .unwrap()
    }

    async fn updates(&mut self) {
        use stream::StreamExt;
        let update = self.streams.next().await.unwrap();
        self.highest[update.stream_id] = update.appended;
        let new = self.majority_appended();
        self.commit_idx.store(new, Ordering::Relaxed);
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

    let base_req = Request {
        term,
        leader_id: chart.our_id(),
        prev_log_idx: prev_idx,
        prev_log_term: prev_term,
        entries: Vec::new(),
        leader_commit: state.commit_index(),
    };

    let commit_idx = state.commit_index();
    let mut commit_idx = CommitIdx::new(commit_idx);
    let base_msg = RegGen {
        commit_idx: commit_idx.commit_idx.clone(),
        base_req,
    };

    let mut subjects = JoinSet::new();
    let mut add_subject = |id, addr, commit_idx: &mut CommitIdx| {
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
            _ = commit_idx.updates() => unreachable!(),
        };

        let (id, addr) = recoverd.unwrap();
        let is_new = adresses.insert((id, addr));
        if is_new {
            add_subject(id, addr, &mut commit_idx)
        }
    }
}
