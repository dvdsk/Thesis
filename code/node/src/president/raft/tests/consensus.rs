use color_eyre::Result;
use std::collections::HashMap;
use std::mem;
use tokio::time::timeout;
use tracing::info;

use crate::util;

use super::mock::TestNode;
use super::util::CurrPres;
use super::*;

#[tokio::test]
async fn test() -> Result<()> {
    // util::setup_test_tracing("node=trace");
    util::setup_test_tracing("node::president::raft=info,node::president::succession=trace");
    // util::setup_test_tracing("");
    const N: u64 = 4;

    let (_guard, discovery_port) = util::free_udp_port()?;
    let mut curr_pres = CurrPres::default();

    let mut nodes = HashMap::new();
    let mut orders = Vec::new();
    for id in 0..N {
        let (node, queue) = TestNode::new(id, N as u16, curr_pres.clone(), discovery_port)
            .await
            .unwrap();
        orders.push((id, queue));
        nodes.insert(id, node);
    }

    // wait till discovery done
    for node in &mut nodes.values_mut() {
        node.found_majority.recv().await.unwrap();
    }

    // kill another node
    // N - 2 nodes left
    let unlucky = *nodes.keys().next().unwrap();
    mem::drop(nodes.remove(&unlucky).unwrap());
    info!("############### KILLED RANDOM NODE, ID: {unlucky}");
    sleep(TIMEOUT).await;

    // find president
    let president = match timeout(TIMEOUT, curr_pres.wait_for()).await {
        Ok(pres) => pres,
        Err(_) => panic!("timed out waiting for president to be elected"),
    };

    // kill president (on drop tasks abort)
    // N - 1 nodes left
    mem::drop(nodes.remove(&president).unwrap());
    info!("############### KILLED PRESIDENT, ID: {president}");
    sleep(TIMEOUT).await;

    // check there is no president
    // need to first kill random nodes till the cluster is smaller
    // then the full cluster majority. Then kill the president and
    // no more president should be found (is impossible now)
    assert_eq!(
        curr_pres.get(),
        None,
        "cluster to small to have a president"
    );

    // add a node
    // N - 1 nodes left
    let id = N + 1;
    let (node, queue) = TestNode::new(id, N as u16, curr_pres.clone(), discovery_port)
        .await
        .unwrap();
    nodes.insert(id, node);
    orders.push((id, queue));
    info!("############### ADDED BACK ONE NODE, ID: {id}");

    sleep(TIMEOUT).await; // allows some time for the new node to come online
    match timeout(TIMEOUT, curr_pres.wait_for()).await {
        Err(_) => panic!("timed out waiting for president to be elected"),
        Ok(..) => (),
    };
    Ok(())
}
