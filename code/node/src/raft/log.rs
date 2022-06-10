use std::path::PathBuf;

use color_eyre::Result;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{self, Receiver};
use tokio::task::{self, JoinHandle};
use tracing::{instrument, info};

use crate::redirectory::Staff;
use crate::Chart;
use crate::{Role, Term};

use super::state::State;
use super::{handle_incoming, succession};


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

/// abstraction over raft that allows us to wait on
/// new committed log entries, while transparently holding elections
/// if we become president we recieve log entry: `Order::BecomePres`
pub struct Log {
    pub orders: Receiver<Order>, // commited entries can be recoverd from here
    pub state: State,
    _handle_incoming: JoinHandle<()>,
    _succession: JoinHandle<()>,
}

impl Log {
    #[instrument(skip_all)]
    pub(crate) fn open(
        chart: Chart,
        cluster_size: u16,
        db: sled::Tree,
        listener: TcpListener,
    ) -> Result<Self> {
        let (tx, orders) = mpsc::channel(8);
        let state = State::new(tx, db, chart.our_id());
        info!("opening raft log");

        Ok(Self {
            state: state.clone(),
            orders,
            _handle_incoming: task::Builder::new()
                .name("log_handle_incoming")
                .spawn(handle_incoming(listener, state.clone())),
            _succession: task::Builder::new().name("succesion").spawn(succession(
                chart,
                cluster_size,
                state,
            )),
        })
    }

    pub(crate) async fn recv(&mut self) -> Order {
        self.orders
            .recv()
            .await
            .expect("order channel should never be dropped")
    }
}

/// abstraction over raft that allows us to wait on
/// new committed log entries, without taking part in elections
pub struct ObserverLog {
    pub orders: Receiver<Order>, // commited entries can be recoverd from here
    pub state: State,
    _handle_incoming: JoinHandle<()>,
}

impl ObserverLog {
    #[instrument(skip_all)]
    pub(crate) fn open(
        chart: Chart,
        db: sled::Tree,
        listener: TcpListener,
    ) -> Result<Self> {
        let (tx, orders) = mpsc::channel(8);
        let state = State::new(tx, db, chart.our_id());
        info!("opening raft log");

        Ok(Self {
            state: state.clone(),
            orders,
            _handle_incoming: task::Builder::new()
                .name("log_handle_incoming")
                .spawn(handle_incoming(listener, state.clone())),
        })
    }

    pub(crate) async fn recv(&mut self) -> Order {
        self.orders
            .recv()
            .await
            .expect("order channel should never be dropped")
    }
}
