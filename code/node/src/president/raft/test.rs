use color_eyre::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, Notify};
use tokio::time::timeout;
use tracing::info;

use crate::{util, Term};

use super::*;
use crate::president::Chart;
use instance_chart::{discovery, ChartBuilder, Id};

async fn discoverd_majority(signal: mpsc::Sender<()>, chart: Chart, cluster_size: u16) {
    discovery::found_majority(&chart, cluster_size).await;
    signal.send(()).await.unwrap();
}

async fn wait_till_pres(orders: &mut mpsc::Receiver<Order>, tx: &mut mpsc::Sender<Order>) -> Term {
    loop {
        match orders.recv().await {
            Some(order) => {
                tx.send(order.clone()).await.unwrap();
                if let Order::BecomePres { term } = order {
                    break term;
                }
            }
            None => panic!("testnode dropped mpsc"),
        }
    }
}

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
                Some(Order::ResignPres) => (),
                Some(_other) => unreachable!("The president should never recieve
                                             an order expect resign, recieved: {_other:?}"),
                None => panic!("channel was dropped,
                               this means another thread panicked. Joining the panic"),
            },
        }
        unreachable!(
            "Transport should no be able to fail therefore presidents never lose their post"
        )
    }
}

#[derive(Debug, Clone)]
struct CurrPres {
    state: Arc<Mutex<std::result::Result<Option<Id>, ()>>>,
    notify: Arc<Notify>,
}

impl Default for CurrPres {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(Ok(None))),
            notify: Default::default(),
        }
    }
}

struct PresGuard<'a> {
    curr_pres: &'a mut CurrPres,
    id: Id,
}

impl<'a> Drop for PresGuard<'a> {
    fn drop(&mut self) {
        self.curr_pres.unset(dbg!(self.id));
    }
}

impl CurrPres {
    pub async fn wait_for(&mut self) -> Id {
        self.notify.notified().await;
        self.get().unwrap()
    }

    pub fn get(&mut self) -> Option<Id> {
        self.state
            .lock()
            .unwrap()
            .clone()
            .expect("can only be one president")
    }

    pub fn unset(&mut self, id: Id) {
        let state = &mut self.state.lock().unwrap();
        let pres = state.as_mut().expect("can only be one president");

        match pres {
            Some(curr_id) if *curr_id == id => *pres = None,
            _ => (),
        }
    }

    pub fn set(&mut self, id: Id) -> PresGuard {
        let old = {
            let state = &mut self.state.lock().unwrap();
            state
                .as_mut()
                .expect("can only be one president")
                .replace(id)
        };
        assert_eq!(old, None, "can only be one president");
        self.notify.notify_one();
        PresGuard {
            curr_pres: self,
            id,
        }
    }
}

struct TestNode {
    _tasks: JoinSet<()>,
    pub found_majority: mpsc::Receiver<()>,
    _db: sled::Db, // ensure tree is still availible
}

impl TestNode {
    async fn new(
        id: u64,
        cluster_size: u16,
        curr_pres: CurrPres,
    ) -> Result<(Self, mpsc::Receiver<Order>)> {
        let (listener, port) = util::open_socket(None).await?;
        let chart = ChartBuilder::new()
            .with_id(id)
            .with_service_ports([port, 0, 0])
            .local_discovery(true)
            .finish()?;

        let db = sled::Config::new().temporary(true).open().unwrap();
        let tree = db.open_tree("pres").unwrap();
        let (order_tx, order_rx) = mpsc::channel(16);
        let (debug_tx, debug_rx) = mpsc::channel(16);
        let state = State::new(order_tx, tree, id);

        let (signal, found_majority) = mpsc::channel(1);

        let mut tasks = JoinSet::new();
        tasks.spawn(discoverd_majority(signal, chart.clone(), cluster_size));
        tasks.spawn(discovery::maintain(chart.clone()));

        tasks.spawn(handle_incoming(listener, state.clone()));
        tasks.spawn(succession(chart.clone(), cluster_size, state.clone()));
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
                _db: db,
            },
            debug_rx,
        ))
    }
}

const TIMEOUT: Duration = Duration::from_millis(500);

#[tokio::test]
async fn test_voting() -> Result<()> {
    util::setup_test_tracing("node::president::raft::state::vote=trace");
    const N: u64 = 4;

    for _ in 0..1 {
        let mut curr_pres = CurrPres::default();

        let mut nodes = HashMap::new();
        let mut orders = Vec::new();
        for id in 0..N {
            let (node, queue) = TestNode::new(id, N as u16, curr_pres.clone())
                .await
                .unwrap();
            orders.push((id, queue));
            nodes.insert(id, node);
        }

        // wait till discovery done
        for node in &mut nodes.values_mut() {
            node.found_majority.recv().await.unwrap();
        }

        // find president
        let president = match timeout(TIMEOUT, curr_pres.wait_for()).await {
            Ok(pres) => pres,
            Err(_) => panic!("timed out waiting for president to be elected"),
        };

        // kill president, on drop tasks abort
        nodes.remove(&president).unwrap();
        info!("killed president, id: {president}");
        sleep(TIMEOUT).await;

        // kill another node
        let unlucky = *nodes.keys().next().unwrap();
        nodes.remove(&unlucky).unwrap();
        info!("killed random node, id: {unlucky}");
        sleep(TIMEOUT).await;

        // check there is no president
        assert_eq!(
            curr_pres.get(),
            None,
            "cluster to small to have a president"
        );

        // add a node
        let id = N + 1;
        let (node, queue) = TestNode::new(id, N as u16, curr_pres.clone())
            .await
            .unwrap();
        nodes.insert(id, node);
        orders.push((id, queue));

        match timeout(TIMEOUT, curr_pres.wait_for()).await {
            Err(_) => panic!("timed out waiting for president to be elected"),
            Ok(..) => (),
        };
    }
    Ok(())
}
