use color_eyre::Result;
use tokio::time::timeout;
use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::util;

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
        let (tx, orders) = mpsc::channel(16);
        let state = State::new(tx, tree, id);

        let mut tasks = JoinSet::new();
        let (signal, found_majority) = mpsc::channel(1);

        tasks.spawn(discoverd_majority(signal, chart.clone(), cluster_size));
        tasks.spawn(discovery::maintain(chart.clone()));

        tasks.spawn(handle_incoming(listener, state.clone()));
        tasks.spawn(succession(chart, cluster_size, state));

        Ok((
            Self {
                tasks,
                found_majority,
                _db: db,
            },
            orders,
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
                let yielded = rx.recv().await.map(|o| (o, id));
                let state = rx;
                Some((yielded, state))
            });
            Box::pin(s)
        })
        .collect();

    let mut order_stream = stream::select_all(streams);

    while let Some((order, id)) = order_stream.next().await.flatten() {
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

        // kill another node
        let unlucky = *nodes.keys().next().unwrap();
        nodes.remove(&unlucky).unwrap().tasks.abort_all();

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
