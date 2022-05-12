use color_eyre::Result;
use std::collections::HashMap;
use tokio::time::timeout;

use crate::util;

use super::mock::TestAppendNode;
use super::util::CurrPres;
use super::*;

#[tokio::test]
async fn spread_order() -> Result<()> {
    util::setup_test_tracing("node=warn,node::president=trace,node::president::raft::subjects=trace,node::president::raft=warn");
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

    let pres_id = match timeout(TIMEOUT, curr_pres.wait_for()).await {
        Ok(pres) => pres,
        Err(_) => panic!("timed out waiting for president to be elected"),
    };

    let president = nodes.get_mut(&pres_id).unwrap();
    president.order(Order::Test(1)).await;

    sleep(Duration::from_millis(100)).await;

    for queue in orders
        .iter_mut()
        .filter(|(id, _)| *id != pres_id)
        .map(|(_, orders)| orders)
    {
        let order = queue.recv().await.expect("queue was dropped");
        assert_eq!(order, Order::Test(1));
    }

    Ok(())
}
