use async_trait::async_trait;
use color_eyre::Result;
use futures::SinkExt;
use futures::StreamExt;
use futures::TryStreamExt;
use protocol::connection::MsgStream;
use serde::Deserialize;
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing::debug;
use tracing::info;
use tracing::instrument;
use tracing::warn;
use tracing::Instrument;

use crate::president::FixedTerm;
use crate::president::subjects;
use crate::president::Order;
use crate::raft;
use crate::raft::LogWriter;
use crate::raft::Perishable;
use crate::raft::State;
use crate::util;
use crate::Id;
use crate::Idx;

use super::util as test_util;
use super::util::{wait_till_pres, CurrPres};

use crate::Chart;
use instance_chart::{discovery, ChartBuilder};

#[derive(Clone)]
struct MockStatusNotifier;

#[async_trait]
impl crate::raft::subjects::StatusNotifier for MockStatusNotifier {
    async fn subject_up(&self, _subject_id: Id) {}
    async fn subject_down(&self, _subject_id: Id) {}
}

async fn heartbeat_while_pres(
    mut chart: Chart,
    state: State<Order>,
    mut orders: mpsc::Receiver<Perishable<Order>>,
    mut tx: mpsc::Sender<Order>,
    mut curr_pres: CurrPres,
) {
    loop {
        let term = wait_till_pres(&mut orders, &mut tx).await;
        let _drop_guard = curr_pres.set(chart.our_id());

        let (order_tx, _order_rx) = tokio::sync::broadcast::channel(16);
        let (_commit_notify_tx, commit_notify) = mpsc::channel(16);

        let status_notifier = MockStatusNotifier;
        info!("id: {} became president, term: {term}", chart.our_id());

        let instruct = subjects::instruct(
            &mut chart,
            order_tx,
            commit_notify,
            status_notifier,
            state.clone(),
            FixedTerm(term),
        );

        tokio::select! {
            () = instruct => unreachable!(),
            usurper = orders.recv() => match usurper.map(|o| o.order) {
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
    #[instrument(skip(curr_pres))]
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
        let state: State<Order> = State::new(order_tx, tree.clone());

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

        let handle_incoming = raft::handle_incoming(listener, state.clone()).in_current_span();
        tasks.build_task().name("incoming").spawn(handle_incoming);
        let succesion =
            raft::succession(chart.clone(), cluster_size, state.clone()).in_current_span();
        tasks.build_task().name("succession").spawn(succesion);
        let president = heartbeat_while_pres(chart.clone(), state, order_rx, debug_tx, curr_pres)
            .in_current_span();
        tasks.build_task().name("president").spawn(president);

        Ok((
            Self {
                _tasks: tasks,
                found_majority,
            },
            debug_rx,
        ))
    }
}

async fn test_req(
    n: u8,
    partial: Option<Idx>,
    log: &mut LogWriter<Order>,
    stream: &mut MsgStream<Msg, Reply>,
) -> Option<Reply> {
    debug!("appending Test({n}) to log");
    let ticket = match partial {
        Some(idx) => log.re_append(Order::Test(n), idx).await,
        None => log.append(Order::Test(n)).await,
    };
    stream.send(Reply::Waiting(ticket.idx)).await.ok()?;
    ticket.notify.notified().await;
    Some(Reply::Done)
}

async fn handle_conn(stream: TcpStream, mut _log: LogWriter<Order>) {
    use Msg::*;
    use Reply::*;
    let mut stream: MsgStream<Msg, Reply> = connection::wrap(stream);
    while let Ok(Some(msg)) = stream.try_next().await {
        debug!("president got request: {msg:?}");

        let final_reply = match msg {
            ClientReq(_) => Some(GoAway),
            #[cfg(test)]
            Test {
                n,
                follow_up: partial,
            } => test_req(n, partial, &mut _log, &mut stream).await,
        };

        if let Some(reply) = final_reply {
            if let Err(e) = stream.send(reply).await {
                warn!("error replying to message: {e:?}");
                return;
            }
        }
    }
}

pub async fn handle_incoming(listener: &mut TcpListener, log: LogWriter<Order>) {
    let mut request_handlers = JoinSet::new();
    loop {
        let (conn, _addr) = listener.accept().await.unwrap();
        let handle = handle_conn(conn, log.clone()).in_current_span();
        request_handlers
            .build_task()
            .name("president msg conn")
            .spawn(handle);
    }
}

#[instrument(skip_all)]
pub async fn president(
    mut chart: Chart,
    state: State<Order>,
    mut orders: mpsc::Receiver<Perishable<Order>>,
    mut tx: mpsc::Sender<Order>,
    mut curr_pres: CurrPres,
    mut listener: TcpListener,
) {
    loop {
        let term = wait_till_pres(&mut orders, &mut tx).await;
        let _drop_guard = curr_pres.set(chart.our_id());
        info!("became president, term: {term}");

        let (broadcast, _) = broadcast::channel(16);
        let (tx, commit_notify) = mpsc::channel(16);
        let log_writer = LogWriter {
            term,
            state: state.clone(),
            broadcast: broadcast.clone(),
            notify_tx: tx,
        };

        async fn keep_log_update(orders: &mut mpsc::Receiver<Perishable<Order>>) {
            loop {
                match orders.recv().await.map(|o| o.order) {
                    Some(Order::ResignPres) => break,
                    Some(other) => debug!("order added: {other:?}"),
                    None => panic!(
                        "channel was dropped,
                           this means another thread panicked. Joining the panic"
                    ),
                }
            }
        }

        let load_notify = MockStatusNotifier;

        let instruct = subjects::instruct(
            &mut chart,
            broadcast,
            commit_notify,
            load_notify,
            state.clone(),
            FixedTerm(term),
        );

        tokio::select! {
            () = instruct => unreachable!(),
            () = handle_incoming(&mut listener, log_writer) => unreachable!(),
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
    #[instrument(skip(curr_pres))]
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
        let state = State::new(order_tx, tree.clone());

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

        let handle_incoming = raft::handle_incoming(pres_listener, state.clone()).in_current_span();
        tasks.build_task().name("incoming").spawn(handle_incoming);

        let succesion =
            raft::succession(chart.clone(), cluster_size, state.clone()).in_current_span();
        tasks.build_task().name("succesion").spawn(succesion);

        let president =
            president(chart, state, order_rx, debug_tx, curr_pres, req_listener).in_current_span();
        tasks.build_task().name("president").spawn(president);

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Msg {
    ClientReq(protocol::Request),
    #[cfg(test)]
    Test {
        n: u8,
        follow_up: Option<Idx>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Reply {
    GoAway, // president itself does not assist citizens
    Waiting(Idx),
    Done,
    Error,
}

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
