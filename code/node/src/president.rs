use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing::info;

use instance_chart::Chart as mChart;
type Chart = mChart<3, u16>;

mod messages;
mod raft;
pub use raft::subjects;
pub use raft::{Log, Order};
use crate::Term;

#[derive(Debug, Clone)]
pub struct LogWriter {
    state: raft::State,
    broadcast: broadcast::Sender<Order>,
}

impl LogWriter {
    fn append(&self, order: Order) -> Result<u32> {
        self.state.append(order.clone());
        self.broadcast.send(order).unwrap();
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
    let log_writer = LogWriter {
        state: state.clone(),
        broadcast: broadcast.clone(),
    };

    tokio::select! {
        () = subjects::instruct(chart, broadcast.clone(), state.clone(), term) => unreachable!(),
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
