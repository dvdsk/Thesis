use color_eyre::Result;
use tokio::net::TcpListener;
use tokio::sync::mpsc::{self, Receiver};
use tokio::task::{self, JoinHandle};
use tracing::{info, instrument, Instrument};

use crate::{Chart, util};

use super::state::{State, Perishable};
use super::{handle_incoming, succession, Order};

/// abstraction over raft that allows us to wait on
/// new committed log entries, while transparently holding elections
/// if we become president we recieve log entry: `Order::BecomePres`
pub struct Log<O> {
    pub orders: Receiver<Perishable<O>>, // commited entries can be recoverd from here
    pub state: State<O>,
    _handle_incoming: util::CancelOnDropTask<()>,
    _succession: util::CancelOnDropTask<()>,
}

impl<O: Order> Log<O> {
    #[instrument(name = "Log::open", skip_all)]
    pub(crate) fn open(
        chart: Chart,
        cluster_size: u16,
        db: sled::Tree,
        listener: TcpListener,
    ) -> Result<Self> {
        let (tx, orders) = mpsc::channel(512);
        let state = State::new(tx, db);
        let handle_incoming = handle_incoming(listener, state.clone()).in_current_span();
        let succession = succession(chart, cluster_size, state.clone()).in_current_span();
        info!("opening a normal raft log");

        Ok(Self {
            state,
            orders,
            _handle_incoming: util::spawn_cancel_on_drop(handle_incoming, "log_handle_incoming"),
            _succession: util::spawn_cancel_on_drop(succession, "succession"),
        })
    }

    pub(crate) async fn recv(&mut self) -> Perishable<O> {
        self.orders
            .recv()
            .await
            .expect("order channel should never be dropped")
    }
}

/// abstraction over raft that allows us to wait on
/// new committed log entries, without taking part in elections
pub struct ObserverLog<O> {
    pub orders: Receiver<Perishable<O>>, // commited entries can be recoverd from here
    pub state: State<O>,
    _handle_incoming: JoinHandle<()>,
}

impl<O: Order> ObserverLog<O> {
    #[instrument(name = "ObeserverLog::open", skip_all)]
    pub(crate) fn open(db: sled::Tree, listener: TcpListener) -> Result<Self> {
        let (tx, orders) = mpsc::channel(512);
        let state = State::new(tx, db);
        let handle_incoming = handle_incoming(listener, state.clone()).in_current_span();
        info!("opening a oberver only raft log");

        Ok(Self {
            state,
            orders,
            _handle_incoming: task::Builder::new()
                .name("log_handle_incoming")
                .spawn(handle_incoming),
        })
    }

    pub(crate) async fn recv(&mut self) -> Perishable<O> {
        self.orders
            .recv()
            .await
            .expect("order channel should never be dropped")
    }
}
