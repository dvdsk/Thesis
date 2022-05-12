use color_eyre::Result;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing::info;

use crate::president::messages;
use crate::president::raft;
use crate::president::raft::State;
use crate::president::subjects;
use crate::president::LogWriter;
use crate::president::Order;
use crate::util;

use super::util as test_util;
use super::util::{wait_till_pres, CurrPres};

use crate::president::Chart;
use instance_chart::{discovery, ChartBuilder};

async fn heartbeat_while_pres(
    mut chart: Chart,
    state: State,
    mut orders: mpsc::Receiver<Order>,
    mut tx: mpsc::Sender<Order>,
    mut curr_pres: CurrPres,
) {
    let (placeholder, _) = tokio::sync::broadcast::channel(16);
    loop {
        let term = wait_till_pres(&mut orders, &mut tx).await;
        let _drop_guard = curr_pres.set(chart.our_id());

        info!("id: {} became president, term: {term}", chart.our_id());

        tokio::select! {
            () = subjects::instruct(&mut chart, placeholder.clone(), state.clone(), term) => unreachable!(),
            usurper = orders.recv() => match usurper {
                Some(Order::ResignPres) => info!("president {} resigned", chart.our_id()),
                Some(_other) => unreachable!("The president should never recieve
                                             an order expect resign, recieved: {_other:?}"),
                None => panic!("channel was dropped,
                               this means another thread panicked. Joining the panic"),
            },
        }
    }
}

pub struct TestVoteNode {
    _tasks: JoinSet<()>,
    pub found_majority: mpsc::Receiver<()>,
}

impl TestVoteNode {
    pub async fn new(
        id: u64,
        cluster_size: u16,
        curr_pres: CurrPres,
        disc_port: u16,
    ) -> Result<(Self, mpsc::Receiver<Order>)> {
        let (listener, port) = util::open_socket(None).await?;
        let chart = ChartBuilder::new()
            .with_id(id)
            .with_service_ports([port, 0, 0])
            .with_discovery_port(disc_port)
            .local_discovery(true)
            .finish()?;

        let db = sled::Config::new().temporary(true).open().unwrap();
        let tree = db.open_tree("pres").unwrap();
        let (order_tx, order_rx) = mpsc::channel(16);
        let (debug_tx, debug_rx) = mpsc::channel(16);
        let state = State::new(order_tx, tree.clone(), id);

        let (signal, found_majority) = mpsc::channel(1);

        let mut tasks = JoinSet::new();
        tasks.spawn(test_util::discoverd_majority(
            signal,
            chart.clone(),
            cluster_size,
        ));
        tasks.spawn(discovery::maintain(chart.clone()));

        tasks.spawn(raft::handle_incoming(listener, state.clone()));
        tasks.spawn(raft::succession(chart.clone(), cluster_size, state.clone()));
        tasks.spawn(heartbeat_while_pres(
            chart.clone(),
            state,
            order_rx,
            debug_tx,
            curr_pres,
        ));

        Ok((
            Self {
                _tasks: tasks,
                found_majority,
            },
            debug_rx,
        ))
    }
}

pub async fn president(
    mut chart: Chart,
    state: State,
    mut orders: mpsc::Receiver<Order>,
    mut tx: mpsc::Sender<Order>,
    mut curr_pres: CurrPres,
    mut listener: TcpListener,
) {
    loop {
        let term = wait_till_pres(&mut orders, &mut tx).await;
        let _drop_guard = curr_pres.set(chart.our_id());

        info!("id: {} became president, term: {term}", chart.our_id());

        let (broadcast, _) = broadcast::channel(16);
        let log_writer = LogWriter {
            _state: state.clone(),
            broadcast: broadcast.clone(),
        };

        tokio::select! {
            () = subjects::instruct(&mut chart, broadcast.clone(), state.clone(), term) => unreachable!(),
            () = messages::handle_incoming(&mut listener, log_writer) => unreachable!(),
            usurper = orders.recv() => match usurper {
                Some(Order::ResignPres) => info!("president {} resigned", chart.our_id()),
                Some(_other) => unreachable!("The president should never recieve
                                             an order expect resign, recieved: {_other:?}"),
                None => panic!("channel was dropped,
                               this means another thread panicked. Joining the panic"),
            },
        }
    }
}

pub struct TestAppendNode {
    _tasks: JoinSet<()>,
    pub found_majority: mpsc::Receiver<()>,
    pub req_port: u16,
}

impl TestAppendNode {
    pub async fn new(
        id: u64,
        cluster_size: u16,
        curr_pres: CurrPres,
        disc_port: u16,
    ) -> Result<(Self, mpsc::Receiver<Order>)> {
        let (pres_listener, pres_port) = util::open_socket(None).await?;
        let (req_listener, req_port) = util::open_socket(None).await?;
        let chart = ChartBuilder::new()
            .with_id(id)
            .with_service_ports([pres_port, 0, req_port])
            .with_discovery_port(disc_port)
            .local_discovery(true)
            .finish()?;

        let db = sled::Config::new().temporary(true).open().unwrap();
        let tree = db.open_tree("pres").unwrap();
        let (order_tx, order_rx) = mpsc::channel(16);
        let (debug_tx, debug_rx) = mpsc::channel(16);
        let state = State::new(order_tx, tree.clone(), id);

        let (signal, found_majority) = mpsc::channel(1);

        let mut tasks = JoinSet::new();
        tasks.spawn(test_util::discoverd_majority(
            signal,
            chart.clone(),
            cluster_size,
        ));
        tasks.spawn(discovery::maintain(chart.clone()));
        tasks.spawn(raft::handle_incoming(pres_listener, state.clone()));
        tasks.spawn(raft::succession(chart.clone(), cluster_size, state.clone()));
        tasks.spawn(president(
            chart.clone(),
            state,
            order_rx,
            debug_tx,
            curr_pres,
            req_listener,
        ));

        Ok((
            Self {
                _tasks: tasks,
                found_majority,
                req_port,
            },
            debug_rx,
        ))
    }

    pub async fn order(&mut self, order: Order) {
        use crate::president::messages::{Msg, Reply};
        use futures::SinkExt;
        use protocol::connection;

        let stream = TcpStream::connect(("127.0.0.1", self.req_port))
            .await
            .unwrap();
        if let Order::Test(i) = order {
            let mut stream: connection::MsgStream<Reply, Msg> = connection::wrap(stream);
            stream.send(Msg::ClientReq(protocol::Request::Test(i))).await.unwrap();
        } else {
            unreachable!("only Order::Test should be send during testing");
        }
    }
}
