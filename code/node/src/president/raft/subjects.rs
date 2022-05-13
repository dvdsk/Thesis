use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Duration;

use crate::president::Chart;
use crate::{Id, Term};
use futures::SinkExt;
use protocol::connection::{self, MsgStream};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tokio::time::{sleep, timeout_at, Instant, Sleep};
use tracing::{instrument, trace, warn};

use super::state::append::Request;
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
    _next_idx: u32,
    _match_idx: Arc<AtomicU32>,
    base_msg: Request,
) {
    loop {
        let mut stream = connect(&address).await;

        let mut next_msg = Request { ..base_msg.clone() };
        // send empty msg aka heartbeat
        if let Err(e) = stream.send(Msg::AppendEntries(next_msg.clone())).await {
            warn!("could not send to host, error: {e:?}");
            continue;
        }

        pass_on_orders(&mut broadcast, &mut next_msg, &mut stream).await;
    }
}

async fn pass_on_orders(
    broadcast: &mut broadcast::Receiver<Order>,
    next_msg: &mut Request,
    stream: &mut MsgStream<Reply, Msg>,
) {
    loop {
        let next_hb = Instant::now() + HB_PERIOD;
        let to_send = match timeout_at(next_hb, broadcast.recv()).await {
            Ok(Ok(order)) => Request {
                entries: vec![order],
                ..next_msg.clone()
            },
            Ok(Err(_e)) => todo!("broadcast overflow when the subject is too slow"),
            Err(..) => next_msg.clone(),
        };

        trace!("sending AppendEntries: {to_send:?}");
        match stream.send(Msg::AppendEntries(to_send)).await {
            Ok(..) => continue,
            Err(e) => {
                warn!("could not send to host, error: {e:?}");
                return;
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
        prev_log_idx: prev_idx,
        prev_log_term: prev_term,
        entries: Vec::new(),
        leader_commit: state.commit_index(),
    };

    let mut subjects = JoinSet::new();
    let mut add_subject = |id, addr| {
        let rx = orders.subscribe();
        let match_idx = Arc::new(AtomicU32::new(0));
        let next_idx = state.last_applied() + 1;
        let manage = manage_subject(id, addr, rx, next_idx, match_idx, base_msg.clone());
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
