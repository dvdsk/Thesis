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

    for node in &mut nodes.values_mut() {
        node.found_majority.recv().await.unwrap();
    }

    let unlucky = *nodes.keys().next().unwrap();
    mem::drop(nodes.remove(&unlucky).unwrap());
    info!("############### KILLED RANDOM NODE, ID: {unlucky}");
    sleep(TIMEOUT).await;

    let president = match timeout(TIMEOUT, curr_pres.wait_for()).await {
        Ok(pres) => pres,
        Err(_) => panic!("timed out waiting for president to be elected"),
    };

    mem::drop(nodes.remove(&president).unwrap());
    info!("############### KILLED PRESIDENT, ID: {president}");
    sleep(TIMEOUT).await;

    assert_eq!(
        curr_pres.get(),
        None,
        "cluster to small to have a president"
    );

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
