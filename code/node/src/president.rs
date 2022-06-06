use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tracing::info;

mod load_balancing;
mod messages;

use crate::Chart;
use crate::raft;
use crate::president::load_balancing::LoadBalancer;
use crate::Term;
pub use raft::subjects;
pub use raft::{Log, Order};
use raft::LogWriter;

use self::load_balancing::LoadNotifier;


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
