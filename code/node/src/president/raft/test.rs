use color_eyre::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::info;

use crate::{util, Term};

use super::*;
use crate::president::Chart;
use instance_chart::{discovery, ChartBuilder, Id};

struct TestNode {
    tasks: JoinSet<()>,
    pub found_majority: mpsc::Receiver<()>,
    _db: sled::Db, // ensure tree is still availible
}

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
    curr_pres: Arc<Mutex<Option<Id>>>,
) {
    let (placeholder, _) = tokio::sync::broadcast::channel(16);
    loop {
        let term = wait_till_pres(&mut orders, &mut tx).await;
        {
            // scope ensures lock is dropped before .await point
            curr_pres.lock().unwrap().replace(chart.our_id());
        }

        info!("became president, id: {}", chart.our_id());

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

impl TestNode {
    async fn new(id: u64, cluster_size: u16) -> Result<(Self, mpsc::Receiver<Order>)> {
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
        let curr_pres = Arc::new(Mutex::new(None));

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
            curr_pres
        ));

        Ok((
            Self {
                tasks,
                found_majority,
                _db: db,
            },
            debug_rx,
        ))
    }
}

async fn wait_for_pres(orders: &mut Vec<(Id, mpsc::Receiver<Order>)>) -> Id {
    use futures::{stream, StreamExt};

    let streams: Vec<_> = orders
        .iter_mut()
        .map(|(id, rx)| {
            let id = *id;
            let s = stream::unfold(rx, move |rx| async move {
                match rx.recv().await {
                    Some(order) => Some(((order, id), rx)),
                    None => {
                        info!("closing stream for {id}");
                        None
                    }
                }
            });
            Box::pin(s)
        })
        .collect();

    let mut order_stream = stream::select_all(streams);

    while let Some((order, id)) = order_stream.next().await {
        match order {
            Order::BecomePres { .. } => return id,
            _ => continue,
        }
    }
    unreachable!()
}

const TIMEOUT: Duration = Duration::from_millis(500);

#[tokio::test]
async fn test_voting() -> Result<()> {
    util::setup_test_tracing();

    const N: u64 = 4;
    let mut nodes = HashMap::new();
    let mut orders = Vec::new();
    for id in 0..N {
        let (node, queue) = TestNode::new(id, N as u16).await.unwrap();
        orders.push((id, queue));
        nodes.insert(id, node);
    }

    for _ in 0..1 {
        dbg!("waiting for discovery");
        // wait till discovery done
        for node in &mut nodes.values_mut() {
            node.found_majority.recv().await.unwrap();
        }

        // find president
        let president = wait_for_pres(&mut orders);
        let president = match timeout(TIMEOUT, president).await {
            Err(_) => panic!("timed out waiting for president to be elected"),
            Ok(p) => p,
        };

        // kill president
        nodes.remove(&president).unwrap().tasks.abort_all();
        info!("killed president, id: {president}");
        sleep(TIMEOUT).await;

        // kill another node
        let unlucky = *nodes.keys().next().unwrap();
        nodes.remove(&unlucky).unwrap().tasks.abort_all();
        info!("killed random node, id: {unlucky}");
        sleep(TIMEOUT).await;

        // check there is no president
        let res = timeout(TIMEOUT, wait_for_pres(&mut orders)).await;
        assert!(res.is_err(), "cluster to small to have a president");

        // add a node
        let id = N + 1;
        let (node, queue) = TestNode::new(id, N as u16).await.unwrap();
        nodes.insert(id, node);
        orders.push((id, queue));

        let res = timeout(TIMEOUT, wait_for_pres(&mut orders)).await;
        assert!(res.is_ok(), "timed out waiting for president to be elected");
    }
    Ok(())
}
