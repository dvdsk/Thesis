use std::path::PathBuf;

use color_eyre::eyre::WrapErr;
use color_eyre::Result;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{self, Receiver};
use tokio::task::{self, JoinHandle};
use tracing::instrument;

use crate::directory::Staff;
use crate::president::Chart;
use crate::{Role, Term};

use super::state::State;
use super::{handle_incoming, succession};

// abstraction over raft that allows us to wait on
// new committed log entries.
pub struct Log {
    pub orders: Receiver<Order>, // commited entries can be recoverd from here
    pub state: State,
    _handle_incoming: JoinHandle<()>,
    _succession: JoinHandle<()>,
}

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

impl Log {
    #[instrument(skip_all)]
    pub(crate) fn open(
        chart: Chart,
        cluster_size: u16,
        db: sled::Db,
        listener: TcpListener,
    ) -> Result<Self> {
        let db = db
            .open_tree("president log")
            .wrap_err("Could not open db tree: \"president log\"")?;
        let (tx, orders) = mpsc::channel(8);
        let state = State::new(tx, db, chart.our_id());

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
