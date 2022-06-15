use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tracing::info;

mod load_balancing;
mod messages;

use crate::president::load_balancing::LoadBalancer;
use crate::raft;
use crate::redirectory::Staff;
use crate::Chart;
use crate::{Role, Term};
pub use raft::subjects;
pub use raft::Log;
use raft::LogWriter;

use self::load_balancing::LoadNotifier;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Order {
    /// used as placeholder for the first entry in the log
    None,
    /// assigns a node a role
    Assigned(Role),
    BecomePres {
        term: Term,
    },
    ResignPres,
    #[cfg(test)]
    Test(u8),

    /// assigns a file tree a ministry, the load balancer
    /// will try to uphold its policy that each ministry has
    /// a minister and clecks
    AssignMinistry {
        subtree: PathBuf,
        staff: Staff,
    },
}

impl raft::Order for Order {
    fn elected(term: Term) -> Self {
        Self::BecomePres { term }
    }

    fn resign() -> Self {
        Self::ResignPres
    }

    fn none() -> Self {
        Self::None
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

pub(super) async fn work(state: &mut super::State, chart: &mut Chart, term: Term) -> crate::Role {
    let super::State {
        id,
        pres_orders,
        client_listener,
        ..
    } = state;

    info!("started work as president: {id}");
    let Log { orders, state, .. } = pres_orders;
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
        () = messages::handle_incoming(client_listener, log_writer) => unreachable!(),
        () = recieve_own_order(orders, load_notifier) => {
            info!("President {id} resigned");
            crate::Role::Idle
        }
    }
}
