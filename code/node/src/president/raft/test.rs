use std::collections::HashMap;

use tokio::sync::{mpsc, oneshot};

use crate::util;

use super::*;
use color_eyre::Result;
use instance_chart::{discovery, ChartBuilder};

struct TestNode {
    tasks: JoinSet<()>,
    pub orders: mpsc::Receiver<Order>,
    pub found_majority: oneshot::Receiver<()>,
    _db: sled::Db, // ensure tree is still availible
    id: u64,
}

impl TestNode {
    async fn new(id: u64, cluster_size: u16) -> Result<Self> {
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
        let (signal, found_majority) = oneshot::channel();
        tasks.spawn(discovery::maintain(chart.clone()));
        discovery::found_majority(&chart, cluster_size).await;
        signal.send(()).unwrap();

        tasks.spawn(handle_incoming(listener, state.clone()));
        tasks.spawn(succession(chart, cluster_size, state));

        Ok(Self {
            tasks,
            orders,
            found_majority,
            _db: db,
            id,
        })
    }

    fn is_president(&mut self) -> bool {
        let mut res = false;
        loop {
            match self.orders.try_recv() {
                Ok(Order::BecomePres { .. }) => res = true,
                Ok(Order::ResignPres { .. }) => res = false,
                Ok(_) => continue,
                Err(mpsc::error::TryRecvError::Empty) => return res,
                Err(e) => panic!("{e:?}"),
            }
        }
    }
}

#[tokio::test]
async fn test_voting() -> Result<()> {
    const N: u64 = 4;
    let mut nodes = HashMap::new();
    for id in 0..N {
        let node = TestNode::new(id, N as u16).await.unwrap();
        nodes.insert(id, node);
    }

    for _ in 0..1 {
        // wait till discovery done
        for node in &mut nodes.values_mut() {
            node.found_majority.try_recv().unwrap();
        }

        // find president and ensure no other presidents exist
        let mut search = nodes
            .values_mut()
            .map(|n| (n.is_president(), n))
            .filter(|(res, _)| *res)
            .map(|(_, node)| node.id);
        let president = search.nth(0).expect("there should be a president");
        assert!(
            search.next().is_none(),
            "there should only be one president"
        );

        // kill president
        nodes.remove(&president).unwrap().tasks.abort_all();

        // kill another node
        let unlucky = *nodes.keys().next().unwrap();
        nodes.remove(&unlucky).unwrap().tasks.abort_all();

        // check there is no president (impossible cluster to small)
        let n_presidents = nodes
            .values_mut()
            .map(|n| (n.is_president(), n))
            .filter(|(res, _)| *res)
            .map(|(_, node)| node.id)
            .count();
        assert_eq!(n_presidents, 0);
        
        // add a node
        let id = N+1;
        let node = TestNode::new(id, N as u16).await.unwrap();
        nodes.insert(id, node);

        // check there is one president
        let n_presidents = nodes
            .values_mut()
            .map(|n| (n.is_president(), n))
            .filter(|(res, _)| *res)
            .map(|(_, node)| node.id)
            .count();
        assert_eq!(n_presidents, 1);
    }
    Ok(())
}
