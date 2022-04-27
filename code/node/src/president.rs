use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing::info;

use instance_chart::Chart as mChart;
type Chart = mChart<3, u16>;

mod messages;
mod raft;
mod subjects;
pub use raft::{Log, Order};

#[derive(Debug, Clone)]
pub struct LogWriter {
    state: raft::State,
    broadcast: broadcast::Sender<()>,
}

pub(super) async fn work(
    log: &mut Log,
    chart: &mut Chart,
    listener: &mut TcpListener,
) -> crate::Role {
    info!("started work as president: {}", chart.our_id());
    let Log { orders, state, .. } = log;
    let (broadcast, _) = broadcast::channel(16);
    let log_writer = LogWriter {
        state: state.clone(),
        broadcast: broadcast.clone(),
    };

    tokio::select! {
        () = subjects::instruct(chart, broadcast.clone(), state.clone()) => unreachable!(),
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
