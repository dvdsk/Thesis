use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::raft::{CONN_RETRY_PERIOD, SUBJECT_CONN_TIMEOUT};
use crate::{Id, Idx, Term};
use async_trait::async_trait;
use futures::{SinkExt, TryStreamExt};
use protocol::connection::{self, MsgStream};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, Notify};
use tokio::task::JoinSet;
use tokio::time::{sleep, sleep_until, timeout, timeout_at, Instant};
use tracing::{debug, info, instrument, trace, warn, Instrument};

use super::state::append;
use super::{Msg, Order, Reply};
use super::{State, HB_PERIOD};

mod source;
pub use source::{Source, SourceNotify};
mod committed;
use committed::Committed;
mod request_gen;
use request_gen::RequestGen;

#[async_trait]
pub trait StatusNotifier: Clone + Sync + Send {
    async fn subject_up(&self, subject: Id);
    async fn subject_down(&self, subject: Id);
}

/// does nothing
#[derive(Clone)]
pub struct EmptyNotifier;

#[async_trait]
impl StatusNotifier for EmptyNotifier {
    async fn subject_up(&self, _: Id) {}
    async fn subject_down(&self, _: Id) {}
}

async fn connect<O: Order>(address: &SocketAddr) -> MsgStream<Reply, Msg<O>> {
    use std::io::ErrorKind;
    loop {
        match TcpStream::connect(address).await {
            Ok(stream) => {
                let stream = connection::wrap(stream);
                break stream;
            }
            Err(e) => match e.kind() {
                ErrorKind::ConnectionReset
                | ErrorKind::ConnectionRefused
                | ErrorKind::ConnectionAborted => {
                    sleep(CONN_RETRY_PERIOD).await;
                    continue;
                }
                _ => panic!("unrecoverable error while connecting to subject: {e:?}"),
            },
        }
    }
}

/// replicates orders to a subject (a raft follower), returns its own id
/// when done. The id is used to inform node discovery to let us know
/// if this node comes back online
#[instrument(skip_all, fields(subject_id = subject_id))]
async fn manage_subject<O: Order>(
    subject_id: Id,
    address: SocketAddr,
    mut broadcast: broadcast::Receiver<O>,
    mut appended: mpsc::Sender<u32>,
    mut req_gen: RequestGen<O>,
    status_notify: impl StatusNotifier,
) -> Id {
    while let Ok(mut stream) = timeout(SUBJECT_CONN_TIMEOUT, connect(&address)).await {
        let init_msg = req_gen.heartbeat();

        // send empty msg aka heartbeat
        if let Err(e) = stream.send(Msg::AppendEntries(init_msg)).await {
            warn!("could not send to host, error: {e:?}");
            continue;
        }

        info!("subject up");
        status_notify.subject_up(subject_id).await;
        replicate_orders(&mut broadcast, &mut appended, &mut req_gen, &mut stream).await;
        status_notify.subject_down(subject_id).await;
        warn!("subject down");
    }
    warn!("giving up on subject!");
    subject_id
}

#[derive(Debug)]
enum RecieveErr {
    ConnectionClosed,
    ConnectionError(std::io::Error),
}

async fn recieve_reply<O: Order>(
    stream: &mut MsgStream<Reply, Msg<O>>,
    req_gen: &mut RequestGen<O>,
    appended: &mut mpsc::Sender<u32>,
) -> Result<(), RecieveErr> {
    loop {
        match stream.try_next().await {
            Ok(None) => return Err(RecieveErr::ConnectionClosed),
            Err(e) => return Err(RecieveErr::ConnectionError(e)),
            Ok(Some(Reply::RequestVote(..))) => {
                unreachable!("no vote request is ever send on this connection")
            }

            Ok(Some(Reply::AppendEntries(append::Reply::HeartBeatOk))) => (),
            Ok(Some(Reply::AppendEntries(append::Reply::AppendOk))) => {
                appended.try_send(req_gen.next_idx).unwrap();
                req_gen.increment_idx();
            }
            Ok(Some(Reply::AppendEntries(append::Reply::InconsistentLog))) => {
                req_gen.decrement_idx()
            }
            Ok(Some(Reply::AppendEntries(append::Reply::ExPresident(new_term)))) => {
                warn!("we are not the current president, new president has term: {new_term}")
                // president will be taken down by the term watcher it is selected on
            }
        }
    }
}

/// replicate orders untill connection is lost
#[instrument(skip_all)]
async fn replicate_orders<O: Order>(
    _broadcast: &mut broadcast::Receiver<O>,
    appended: &mut mpsc::Sender<u32>,
    req_gen: &mut RequestGen<O>,
    stream: &mut MsgStream<Reply, Msg<O>>,
) {
    let mut next_hb = Instant::now() + HB_PERIOD;
    sleep_until(next_hb).await;

    loop {
        next_hb += HB_PERIOD;

        let to_send = if req_gen.misses_logs() {
            debug!("sending missing logs at idx: {}", req_gen.next_idx);
            req_gen.append()
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
        match timeout_at(next_hb, recieve_reply(stream, req_gen, appended)).await {
            Err(..) => (), // not a problem reply can arrive later
            Ok(Ok(..)) => unreachable!("we always keep recieving"),
            Ok(Err(e)) => {
                warn!("did not recieve reply, error: {e:?}");
                return;
            }
        }
    }
}

struct Subjects<O: Order, N: StatusNotifier> {
    subject_added: Notify,
    subjects: JoinSet<Id>,
    orders: broadcast::Sender<O>,
    base_msg: RequestGen<O>,
    status_notify: N,
}

impl<O: Order, N: StatusNotifier + 'static> Subjects<O, N> {
    fn add(&mut self, id: Id, addr: SocketAddr, commit_idx: &mut Committed<'_, O>) {
        let broadcast_rx = self.orders.subscribe();
        let append_updates = commit_idx.track_subject();

        let manage = manage_subject(
            id,
            addr,
            broadcast_rx,
            append_updates,
            self.base_msg.clone(),
            self.status_notify.clone(),
        )
        .in_current_span();

        self.subjects
            .build_task()
            .name("manage_subject")
            .spawn(manage);
        self.subject_added.notify_one();
    }

    async fn join_one(&mut self) -> Result<Id, Box<dyn std::any::Any + std::marker::Send>> {
        if self.subjects.is_empty() {
            self.subject_added.notified().await;
        };

        self.subjects
            .join_one()
            .await
            .unwrap()
            .map_err(|e| e.into_panic())
    }
}

/// look for new subjects in the chart and register them
#[instrument(skip_all)]
pub async fn instruct<O: Order>(
    members: &mut impl Source,
    orders: broadcast::Sender<O>,
    commit_notify: mpsc::Receiver<(Idx, Arc<Notify>)>,
    status_notify: impl StatusNotifier + Clone + Sync + Send + 'static,
    state: State<O>,
    term: Term,
) {
    let mut commit_idx = Committed::new(commit_notify, &state);
    let mut subjects = Subjects {
        subjects: JoinSet::new(),
        subject_added: Notify::new(),
        orders,
        base_msg: RequestGen::new(state.clone(), term, members),
        status_notify,
    };

    let mut notify = members.notify();
    let mut adresses: HashSet<_> = members.adresses().into_iter().collect();
    for (id, addr) in adresses.iter().cloned() {
        subjects.add(id, addr, &mut commit_idx);
    }

    loop {
        tokio::select! {
            recoverd = notify.recv_new() => {
                let (id, addr) = recoverd.unwrap();
                let is_new = adresses.insert((id, addr));
                if is_new {
                    subjects.add(id, addr, &mut commit_idx);
                }
            },
            down = subjects.join_one() => {
                let down = down.expect("manage subject panicked!");
                members.forget(down);
            }
            _ = commit_idx.maintain() => unreachable!(),
        }
    }
}
