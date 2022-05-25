use color_eyre::Result;
use futures::stream;
use std::collections::HashMap;
use stream::StreamExt;
use tokio::time::timeout;

use crate::util;

use super::mock::TestAppendNode;
use super::util::CurrPres;
use super::*;

async fn order_cluster(curr_pres: &mut CurrPres, nodes: &mut HashMap<u64, TestAppendNode>, n: u8) {
    let mut incomplete_order = loop { // first try
        let pres_id = match timeout(TIMEOUT, curr_pres.wait_for()).await {
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
        let pres_id = match timeout(TIMEOUT, curr_pres.wait_for()).await {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn kill_president_mid_order() -> Result<()> {
    // util::setup_test_tracing("node=warn,node::president=trace,node::president::raft::subjects=trace,node::president::raft=info");
    // util::setup_test_tracing("node=warn,node::president::raft::test::consensus=trace,node::president::raft::state::append=info");
    util::setup_test_tracing("node=trace,node::util=warn");
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

    let order_stream = orders
        .into_iter()
        .map(|(_, queue)| queue)
        .map(|queue| {
            stream::unfold(queue, move |mut rx| async move {
                let order = rx.recv().await.unwrap();
                Some((order, rx))
            })
        })
        .map(Box::pin);
    let mut order_stream = stream::select_all(order_stream);

    for i in 1..u8::MAX {
        order_cluster(&mut curr_pres, &mut nodes, i).await;

        let mut n_recieved = 0;
        for order in order_stream.next().await {
            match order {
                Order::ResignPres | Order::BecomePres { .. } => continue,
                Order::Test(j) if j < i => continue,
                Order::Test(j) if j == i => n_recieved += 1,
                _ => panic!("recieved unhandled order: {order:?}"),
            }
            let majority = util::div_ceil(N as usize, 2);
            if n_recieved >= majority {
                break;
            }
        }
    }
    Ok(())
}
