use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, Notify};
use tracing::info;

use instance_chart::Chart as mChart;
type Chart = mChart<3, u16>;

mod load_balancing;
mod messages;
pub mod raft;
use crate::president::load_balancing::LoadBalancer;
use crate::{Idx, Term};
pub use raft::subjects;
pub use raft::{Log, Order};

use self::load_balancing::LoadNotifier;

/// interface to append an item to the clusters raft log and
/// return once it is comitted
#[derive(Debug, Clone)]
pub struct LogWriter {
    term: Term,
    state: raft::State,
    broadcast: broadcast::Sender<Order>,
    notify_tx: mpsc::Sender<(Idx, Arc<Notify>)>,
}

pub struct AppendTicket {
    idx: Idx,
    notify: Arc<Notify>,
}

impl AppendTicket {
    async fn committed(self) {
        self.notify.notified().await;
    }
}

impl LogWriter {
    // returns notify
    async fn append(&self, order: Order) -> AppendTicket {
        let idx = self.state.append(order.clone(), self.term);
        let notify = Arc::new(Notify::new());
        self.notify_tx.send((idx, notify.clone())).await.unwrap();
        self.broadcast.send(order).unwrap();
        AppendTicket { idx, notify }
    }
    /// Verify an order was appended correctly, if it was not append it again
    async fn re_append(&self, order: Order, prev_idx: Idx) -> AppendTicket {
        use raft::LogEntry;

        match self.state.entry_at(prev_idx) {
            Some(LogEntry { order, .. }) if order == order => {
                let notify = Arc::new(Notify::new());
                self.notify_tx
                    .send((prev_idx, notify.clone()))
                    .await
                    .unwrap();
                AppendTicket {
                    idx: prev_idx,
                    notify,
                }
            }
            Some(LogEntry { .. }) => self.append(order).await,
            None => self.append(order).await,
        }
    }
}

async fn recieve_own_order(orders: &mut mpsc::Receiver<Order>, load_notifier: LoadNotifier) {
    loop {
        match orders.recv().await {
            Some(Order::ResignPres) => break,
            Some(other) => load_notifier.committed(other).await,
            None => panic!(
                "channel was dropped,
                           this means another thread panicked. Joining the panic"
            ),
        }
    }
}

pub(super) async fn work(
    log: &mut Log,
    chart: &mut Chart,
    listener: &mut TcpListener,
    term: Term,
) -> crate::Role {
    let id = chart.our_id();

    info!("started work as president: {id}");
    let Log { orders, state, .. } = log;
    let (broadcast, _) = broadcast::channel(16);
    let (tx, notify_rx) = mpsc::channel(16);

    let log_writer = LogWriter {
        term,
        state: state.clone(),
        broadcast: broadcast.clone(),
        notify_tx: tx,
    };

    let (load_balancer, load_notifier) = LoadBalancer::new(log_writer.clone(), chart.clone());
    let instruct_subjects = subjects::instruct(
        chart,
        broadcast.clone(),
        notify_rx,
        load_notifier.clone(),
        state.clone(),
        term,
    );

    tokio::select! {
        () = load_balancer.run(&state) => unreachable!(),
        () = instruct_subjects => unreachable!(),
        () = messages::handle_incoming(listener, log_writer) => unreachable!(),
        () = recieve_own_order(orders, load_notifier) => {
            info!("President {id} resigned");
            crate::Role::Idle
        }
    }
}
