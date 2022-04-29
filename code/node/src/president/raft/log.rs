use color_eyre::eyre::WrapErr;
use color_eyre::Result;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{self, Receiver};
use tokio::task::{self, JoinHandle};

use crate::president::Chart;
use crate::{Role, Term};

use super::state::State;
use super::{handle_incoming, succession};

// abstraction over raft that allows us to wait on
// new committed log entries.
pub struct Log {
    pub orders: Receiver<Order>, // commited entries can be recoverd from here
    pub state: State,
    handle_incoming: JoinHandle<()>,
    succession: JoinHandle<()>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Order {
    Assigned(Role),
    BecomePres { term: Term },
    ResignPres,
}

impl Log {
    pub(crate) fn open(
        chart: Chart,
        cluster_size: u16,
        db: sled::Db,
        listener: TcpListener,
    ) -> Result<Self> {
        let db = db
            .open_tree("president log")
            .wrap_err("Could not open db tree: \"president log\"")?;
        let (tx, orders) = mpsc::channel(100);
        let state = State::new(tx, db);

        Ok(Self {
            state: state.clone(),
            orders,
            handle_incoming: task::spawn(handle_incoming(listener, state.clone())),
            succession: task::spawn(succession(chart, cluster_size, state)),
        })
    }

    pub(crate) async fn recv(&mut self) -> Order {
        self.orders
            .recv()
            .await
            .expect("order channel should never be dropped")
    }
}
