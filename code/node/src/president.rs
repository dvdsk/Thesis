use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, Notify};
use tracing::info;

use instance_chart::Chart as mChart;
type Chart = mChart<3, u16>;

mod messages;
mod raft;
use crate::{Idx, Term};
pub use raft::subjects;
pub use raft::{Log, Order};

#[derive(Debug, Clone)]
pub struct LogWriter {
    state: raft::State,
    broadcast: broadcast::Sender<Order>,
    notify_tx: mpsc::Sender<(Idx, Arc<Notify>)>,
}

pub struct AppendTicket {
    idx: Idx,
    notify: Arc<Notify>,
}

impl LogWriter {
    // returns notify
    async fn append(&self, order: Order) -> AppendTicket {
        let idx = self.state.append(order.clone());
        let notify = Arc::new(Notify::new());
        self.notify_tx.send((idx, notify.clone())).await.unwrap();
        self.broadcast.send(order).unwrap();
        AppendTicket { idx, notify }
    }
}

pub(super) async fn work(
    log: &mut Log,
    chart: &mut Chart,
    listener: &mut TcpListener,
    term: Term,
) -> crate::Role {
    info!("started work as president: {}", chart.our_id());
    let Log { orders, state, .. } = log;
    let (broadcast, _) = broadcast::channel(16);
    let (tx, notify_rx) = mpsc::channel(16);

    let log_writer = LogWriter {
        state: state.clone(),
        broadcast: broadcast.clone(),
        notify_tx: tx,
    };

    tokio::select! {
        () = subjects::instruct(chart, broadcast.clone(), notify_rx, state.clone(), term) => unreachable!(),
        () = messages::handle_incoming(listener, log_writer) => unreachable!(),
        usurper = orders.recv() => match usurper {
            Some(Order::ResignPres) => crate::Role::Idle,
            Some(_other) => unreachable!("The president should never recieve
                                         an order expect resign, recieved: {_other:?}"),
            None => panic!("channel was dropped,
                           this means another thread panicked. Joining the panic"),
        },
    }
}
