use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::redirectory::Node;
use crate::raft::CONN_RETRY_PERIOD;
use crate::{Id, Idx, Term};
use async_trait::async_trait;
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

mod source;
pub use source::{Source, SourceNotify};
mod commited;
use commited::Commited;
mod request_gen;
use request_gen::RequestGen;

#[async_trait]
pub trait StatusNotifier: Clone + Sync + Send {
    async fn subject_up(&self, subject: Node);
    async fn subject_down(&self, subject: Node);
}

/// does nothing
#[derive(Clone)]
pub struct EmptyNotifier;

#[async_trait]
impl StatusNotifier for EmptyNotifier {
    async fn subject_up(&self, _: Node) {}
    async fn subject_down(&self, _: Node) {}
}

async fn connect(address: &SocketAddr) -> MsgStream<Reply, Msg> {
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

#[instrument(skip_all, fields(president_id = req_gen.base.leader_id, subject_id = subject_id))]
async fn manage_subject(
    subject_id: Id,
    address: SocketAddr,
    mut broadcast: broadcast::Receiver<Order>,
    mut appended: mpsc::Sender<u32>,
    mut req_gen: RequestGen,
    status_notify: impl StatusNotifier,
) {
    loop {
        let mut stream = connect(&address).await;

        let init_msg = req_gen.heartbeat();

        // send empty msg aka heartbeat
        if let Err(e) = stream.send(Msg::AppendEntries(init_msg)).await {
            warn!("could not send to host, error: {e:?}");
            continue;
        }

        status_notify
            .subject_up(Node {
                id: subject_id,
                addr: address,
            })
            .await;
        replicate_orders(&mut broadcast, &mut appended, &mut req_gen, &mut stream).await;
        status_notify
            .subject_down(Node {
                id: subject_id,
                addr: address,
            })
            .await;
    }
}

async fn recieve_reply(
    stream: &mut MsgStream<Reply, Msg>,
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
async fn replicate_orders(
    _broadcast: &mut broadcast::Receiver<Order>,
    appended: &mut mpsc::Sender<u32>,
    req_gen: &mut RequestGen,
    stream: &mut MsgStream<Reply, Msg>,
) {
    let mut next_hb = Instant::now() + HB_PERIOD;
    sleep_until(next_hb).await;

    loop {
        next_hb = next_hb + HB_PERIOD;

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
                sleep_until(next_hb).await;
            }
        }
    }
}

/// look for new subjects in the chart and register them
#[instrument(skip_all, fields(president_id = state.id))]
pub async fn instruct(
    members: &mut impl Source,
    orders: broadcast::Sender<Order>,
    commit_notify: mpsc::Receiver<(Idx, Arc<Notify>)>,
    status_notify: impl StatusNotifier + Clone + Sync + Send + 'static,
    state: State,
    term: Term,
) {
    let mut commit_idx = Commited::new(commit_notify, &state);
    let base_msg = RequestGen::new(state.clone(), term, members);

    let mut subjects = JoinSet::new();
    let mut add_subject = |id, addr, commit_idx: &mut Commited| {
        let broadcast_rx = orders.subscribe();
        let append_updates = commit_idx.track_subject();

        let manage = manage_subject(
            id,
            addr,
            broadcast_rx,
            append_updates,
            base_msg.clone(),
            status_notify.clone(),
        );
        subjects.build_task().name("manage_subject").spawn(manage);
    };

    let mut notify = members.notify();
    let mut adresses: HashSet<_> = members.adresses().into_iter().collect();
    for (id, addr) in adresses.iter().cloned() {
        add_subject(id, addr, &mut commit_idx)
    }

    loop {
        let recoverd = tokio::select! {
            res = notify.recv_new() => res,
            _ = commit_idx.maintain() => unreachable!(),
        };

        let (id, addr) = recoverd.unwrap();
        let is_new = adresses.insert((id, addr));
        if is_new {
            add_subject(id, addr, &mut commit_idx)
        }
    }
}
