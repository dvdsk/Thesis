use color_eyre::eyre::WrapErr;
use color_eyre::Result;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{self, Receiver};
use tokio::task::{self, JoinHandle};

use crate::Role;

use super::state::State;
use super::{handle_msg, succession};

// abstraction over raft that allows us to wait on
// new committed log entries.
pub struct Log {
    // commited? entries will appear here
    orders: Receiver<Order>,
    handle_msg: JoinHandle<()>,
    succession: JoinHandle<()>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Order {
    Assigned(Role),
    Elected,
}

impl Log {
    pub(crate) fn open(db: sled::Db, listener: TcpListener) -> Result<Self> {
        let db = db
            .open_tree("president log")
            .wrap_err("Could not open db tree: \"president log\"")?;
        let (tx, orders) = mpsc::channel(100);
        let state = State::new(tx, db);

        Ok(Self {
            orders,
            handle_msg: task::spawn(handle_msg(listener, state.clone())),
            succession: task::spawn(succession(state)),
        })
    }

    pub(crate) async fn recv(&mut self) -> Order {
        self.orders
            .recv()
            .await
            .expect("order channel should never be dropped")
    }
}
