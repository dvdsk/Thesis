use color_eyre::Result;
use futures::stream;
use std::collections::HashMap;
use std::net;
use std::sync::Arc;
use stream::StreamExt;
use tokio::sync::mpsc::Receiver;
use tokio::sync::Mutex;
use tokio::time::timeout;

use crate::{util, Id};

use super::mock::TestAppendNode;
use super::util::CurrPres;
use super::*;

async fn order_cluster(curr_pres: &mut CurrPres, nodes: &mut HashMap<u64, TestAppendNode>, n: u8) {
    let mut incomplete_order = loop {
        // first try
        let pres_id = match timeout(TEST_TIMEOUT, curr_pres.wait_for()).await {
            Ok(pres) => pres,
            Err(_) => panic!("timed out waiting for president to be elected"),
        };
        let president = nodes.get_mut(&pres_id).unwrap();
        match president.order(Order::Test(n), None).await {
            Ok(incomplete) => break incomplete,
            Err(e) => {
                warn!(e);
                continue;
            }
        }
    };

    match incomplete_order.completed().await {
        Ok(..) => return,
        Err(e) => warn!(e),
    }

    // finish incomplete order
    loop {
        warn!("order incomplete trying to finish");
        // president can fail before change is comitted therefore keep retrying
        let pres_id = match timeout(TEST_TIMEOUT, curr_pres.wait_for()).await {
            Ok(pres) => pres,
            Err(_) => panic!("timed out waiting for president to be elected"),
        };

        let president = nodes.get_mut(&pres_id).unwrap();
        let follow_up = incomplete_order.idx;
        let mut incomplete_order = match president.order(Order::Test(n), Some(follow_up)).await {
            Ok(incomplete) => incomplete,
            Err(e) => {
                warn!(e);
                continue;
            }
        };

        match incomplete_order.completed().await {
            Ok(..) => break,
            Err(e) => warn!(e),
        }
    }
}

#[tokio::test]
async fn spread_order() -> Result<()> {
    // util::setup_test_tracing("node=warn,node::president=trace,node::president::raft::subjects=trace,node::president::raft=info");
    // util::setup_test_tracing("");
    const N: u64 = 4;

    let (_guard, discovery_port) = util::free_udp_port()?;
    let mut curr_pres = CurrPres::default();

    let mut nodes = HashMap::new();
    let mut orders = Vec::new();
    for id in 0..N {
        let (node, queue) = TestAppendNode::new(id, N as u16, curr_pres.clone(), discovery_port)
            .await
            .unwrap();
        orders.push((id, queue));
        nodes.insert(id, node);
    }

    for node in &mut nodes.values_mut() {
        node.found_majority.recv().await.unwrap();
    }

    order_cluster(&mut curr_pres, &mut nodes, 1).await;
    sleep(Duration::from_millis(100)).await;

    for queue in orders.iter_mut().map(|(_, orders)| orders) {
        loop {
            let order = queue.recv().await.expect("queue was dropped");
            match order {
                Order::ResignPres => continue,
                Order::BecomePres { .. } => break,
                _ => {
                    assert_eq!(order, Order::Test(1));
                    break;
                }
            }
        }
    }

    Ok(())
}

type Queue = Arc<Mutex<Receiver<Order>>>;

async fn setup(
    n: u64,
) -> (
    HashMap<Id, TestAppendNode>,
    Vec<(Id, Queue)>,
    CurrPres,
    net::UdpSocket,
) {
    let (guard, discovery_port) = util::free_udp_port().unwrap();
    let curr_pres = CurrPres::default();

    let mut nodes = HashMap::new();
    let mut orders = Vec::new();
    for id in 0..n {
        let (node, queue) = TestAppendNode::new(id, n as u16, curr_pres.clone(), discovery_port)
            .await
            .unwrap();
        let queue = Arc::new(Mutex::new(queue));
        orders.push((id, queue));
        nodes.insert(id, node);
    }

    for node in &mut nodes.values_mut() {
        node.found_majority.recv().await.unwrap();
    }
    (nodes, orders, curr_pres, guard)
}

async fn add_new_node(
    id: Id,
    nodes: &mut HashMap<u64, TestAppendNode>,
    orders: &mut Vec<(Id, Queue)>,
    curr_pres: &mut CurrPres,
    discovery_port: u16,
    cluster_size: u16,
) -> Queue {
    let (node, queue) = TestAppendNode::new(id, cluster_size, curr_pres.clone(), discovery_port)
        .await
        .unwrap();
    let queue = Arc::new(Mutex::new(queue));
    orders.push((id, queue.clone()));
    nodes.insert(id, node);
    queue
}

#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn kill_president_mid_order() -> Result<()> {
    // util::setup_test_tracing("node=warn,node::president=trace,node::president::raft::subjects=trace,node::president::raft=info");
    // util::setup_test_tracing("node=warn,node::president::raft::test::consensus=trace,node::president::raft::state::append=info");
    util::setup_test_tracing("node=trace,node::util=warn,node::president::raft::succession=debug");
    const CLUSTER_SIZE: u64 = 4;
    let (mut nodes, mut orders, mut curr_pres, guard) = setup(CLUSTER_SIZE).await;

    let to_stream = |queue: Queue| {
        let stream = stream::unfold(queue, |rx: Queue| async move {
            let order = rx.lock().await.recv().await;
            order.map(|order| (order, rx))
        });
        Box::pin(stream)
    };

    let mut order_stream = stream::SelectAll::new();
    for queue in orders.iter().map(|(_, queue)| queue).cloned() {
        order_stream.push(to_stream(queue));
    }

    for i in 1..=10 {
        order_cluster(&mut curr_pres, &mut nodes, i).await;

        if i == 5 {
            curr_pres.kill(&mut nodes).await;
            let queue = add_new_node(
                CLUSTER_SIZE,
                &mut nodes,
                &mut orders,
                &mut curr_pres,
                guard.local_addr().unwrap().port(),
                CLUSTER_SIZE as u16,
            )
            .await;
            order_stream.push(to_stream(queue));
        }

        let mut n_up_to_date = 0;
        loop {
            let order = order_stream.next().await.unwrap();
            match order {
                Order::ResignPres | Order::BecomePres { .. } => continue,
                Order::Test(j) if j < i => continue,
                Order::Test(j) if j == i => n_up_to_date += 1,
                _ => panic!("recieved unhandled order: {order:?}"),
            }
            if n_up_to_date == CLUSTER_SIZE - 1 {
                dbg!();
                break;
            }
        }
    }
    std::mem::drop(nodes); // cant return without this for some reason
    Ok(())
}
