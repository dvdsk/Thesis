use color_eyre::Result;
use futures::StreamExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing::debug;
use tracing::info;
use tracing::instrument;

use crate::president::messages;
use crate::president::raft;
use crate::president::raft::State;
use crate::president::subjects;
use crate::president::LogWriter;
use crate::president::Order;
use crate::util;
use crate::Idx;

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
    loop {
        let term = wait_till_pres(&mut orders, &mut tx).await;
        let _drop_guard = curr_pres.set(chart.our_id());

        let (order_tx, _order_rx) = tokio::sync::broadcast::channel(16);
        let (_notify_tx, notify_rx) = mpsc::channel(16);
        info!("id: {} became president, term: {term}", chart.our_id());

        tokio::select! {
            () = subjects::instruct(&mut chart, order_tx, notify_rx, state.clone(), term) => unreachable!(),
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
        tasks.build_task().name("discoverd_majority").spawn(test_util::discoverd_majority(
            signal,
            chart.clone(),
            cluster_size,
        ));
        tasks.build_task().name("discovery").spawn(discovery::maintain(chart.clone()));

        tasks.build_task().name("incoming").spawn(raft::handle_incoming(listener, state.clone()));
        tasks.build_task().name("succession").spawn(raft::succession(chart.clone(), cluster_size, state.clone()));
        tasks.build_task().name("president").spawn(heartbeat_while_pres(
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

#[instrument(skip_all, fields(id = state.id))]
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
        let (tx, notify_rx) = mpsc::channel(16);
        let log_writer = LogWriter {
            term,
            state: state.clone(),
            broadcast: broadcast.clone(),
            notify_tx: tx,
        };

        async fn keep_log_update(orders: &mut mpsc::Receiver<Order>) {
            loop {
                match orders.recv().await {
                    Some(Order::ResignPres) => break,
                    Some(other) => debug!("order added: {other:?}"),
                    None => panic!(
                        "channel was dropped,
                           this means another thread panicked. Joining the panic"
                    ),
                }
            }
        }

        tokio::select! {
            () = subjects::instruct(&mut chart, broadcast.clone(), notify_rx, state.clone(), term) => unreachable!(),
            () = messages::handle_incoming(&mut listener, log_writer) => unreachable!(),
            () = keep_log_update(&mut orders) => (), // resigned as president
        }
    }
}

pub struct TestAppendNode {
    _tasks: JoinSet<()>,
    pub found_majority: mpsc::Receiver<()>,
    pub req_port: u16,
}

impl TestAppendNode {
    /// the returned mpsc::tx must be recv-ed from actively
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
        tasks
            .build_task()
            .name("discoverd_majority")
            .spawn(test_util::discoverd_majority(
                signal,
                chart.clone(),
                cluster_size,
            ));
        tasks
            .build_task()
            .name("discovery")
            .spawn(discovery::maintain(chart.clone()));
        tasks
            .build_task()
            .name("incoming")
            .spawn(raft::handle_incoming(pres_listener, state.clone()));
        tasks.build_task().name("succesion").spawn(raft::succession(
            chart.clone(),
            cluster_size,
            state.clone(),
        ));
        tasks.build_task().name("president").spawn(president(
            chart,
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
}

use crate::president::messages::{Msg, Reply};
use protocol::connection;
pub struct IncompleteOrder {
    stream: connection::MsgStream<Reply, Msg>,
    pub idx: Idx,
}

impl IncompleteOrder {
    pub async fn completed(&mut self) -> Result<(), &'static str> {
        let res = self
            .stream
            .next()
            .await
            .ok_or("Not committed, recieved None")?
            .map_err(|_| "Not committed, connection failed")?;
        match res {
            Reply::Done => Ok(()),
            _ => Err("Not committed, incorrect reply send"),
        }
    }
}

impl TestAppendNode {
    pub async fn order(
        &mut self,
        order: Order,
        partial: Option<Idx>,
    ) -> Result<IncompleteOrder, &'static str> {
        use futures::SinkExt;

        let stream = TcpStream::connect(("127.0.0.1", self.req_port))
            .await
            .unwrap();
        let i = match order {
            Order::Test(i) => i,
            _ => unreachable!("only Order::Test should be send during testing"),
        };

        let mut stream: connection::MsgStream<Reply, Msg> = connection::wrap(stream);
        stream
            .send(Msg::Test {
                n: i,
                follow_up: partial,
            })
            .await
            .unwrap();
        let reply = stream
            .next()
            .await
            .unwrap()
            .map_err(|_| "Connection was reset, president probably resigned")?;

        match reply {
            Reply::Waiting(idx) => Ok(IncompleteOrder { idx, stream }),
            _ => unreachable!("Got unexpected reply: {reply:?}"),
        }
    }
}
