use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::president::Chart;
use crate::Term;
use futures::SinkExt;
use protocol::connection;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tokio::time::{sleep, timeout_at, Instant};
use tracing::{warn, instrument};

use super::state::LogMeta;
use super::{AppendEntries, State, HB_PERIOD};
use super::{Msg, Reply};

async fn manage_subject(
    address: SocketAddr,
    mut broadcast: broadcast::Receiver<()>,
    _next_idx: u32,
    _match_idx: Arc<AtomicU32>,
    base_msg: AppendEntries,
) {
    use std::io::ErrorKind;

    loop {
        let mut stream: connection::MsgStream<Reply, Msg> = loop {
            match TcpStream::connect(address).await {
                Ok(stream) => {
                    // stream.set_nodelay(true).unwrap();
                    let stream = connection::wrap(stream);
                    break stream;
                }
                Err(e) => match e.kind() {
                    ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionRefused
                    | ErrorKind::ConnectionAborted => (),
                    _ => panic!("unrecoverable error while connecting to subject: {e:?}"),
                },
            }
            sleep(super::HB_TIMEOUT).await;
        };

        let next_msg = AppendEntries { ..base_msg.clone() };
        // send empty msg aka heartbeat
        if let Err(e) = stream.send(Msg::AppendEntries(next_msg.clone())).await {
            warn!("could not send to host, error: {e:?}");
            continue;
        }
        let next_hb = Instant::now() + HB_PERIOD;

        loop {
            match timeout_at(next_hb, broadcast.recv()).await {
                Ok(Ok(_order)) => todo!("send order"),
                Ok(Err(_e)) => todo!("handle backorder"),
                Err(..) => {
                    let to_send = next_msg.clone();
                    match stream.send(Msg::AppendEntries(to_send)).await {
                        Ok(..) => continue,
                        Err(e) => {
                            warn!("could not send to host, error: {e:?}");
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// look for new subjects in the chart and register them
#[instrument(skip_all, fields(id = state.id))]
pub async fn instruct(chart: &mut Chart, orders: broadcast::Sender<()>, state: State, _term: Term) {
    // todo slice of len cluster size for match_idxes
    let LogMeta { idx, term } = state.last_log_meta();
    let base_msg = AppendEntries {
        term,
        leader_id: chart.our_id(),
        prev_log_idx: idx,
        prev_log_term: term,
        entries: Vec::new(),
        leader_commit: state.commit_index(),
    };

    let mut subjects = JoinSet::new();
    let mut add_subject = |addr| {
        let rx = orders.subscribe();
        let match_idx = Arc::new(AtomicU32::new(0));
        let next_idx = state.last_applied() + 1;
        let manage = manage_subject(addr, rx, next_idx, match_idx, base_msg.clone());
        subjects.spawn(manage);
    };

    let mut notify = chart.notify();
    let mut adresses: HashSet<_> = chart.nth_addr_vec::<0>().into_iter().collect();
    for addr in adresses.iter().cloned() {
        add_subject(addr)
    }

    while let Ok((_id, addr)) = notify.recv_nth_addr::<0>().await {
        let is_new = adresses.insert(addr);
        if is_new {
            add_subject(addr)
        }
    }
}
