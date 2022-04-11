use std::time::Duration;

use tokio::time::sleep;
use tracing::info;

use crate::Chart;
use crate::chart::{handle_incoming, broadcast_periodically};

#[tracing::instrument]
pub async fn maintain(chart: Chart) {
    let f1 = tokio::spawn(handle_incoming(chart.clone()));
    let f2 = tokio::spawn(broadcast_periodically(chart, Duration::from_secs(10)));
    let (_, _) = tokio::join!(f1, f2);
    unreachable!("maintain never returns")
}


#[tracing::instrument]
pub async fn found_everyone(chart: Chart, full_size: u16) {
    assert!(full_size > 2, "minimal cluster size is 3");

    while chart.size() < full_size.into() {
        sleep(Duration::from_millis(100)).await;
    }
    info!(
        "found every member of the cluster, ({} nodes)",
        chart.size()
    );
}


#[tracing::instrument]
pub async fn found_majority(chart: Chart, full_size: u16) {
    assert!(full_size > 2, "minimal cluster size is 3");

    let cluster_majority = (full_size as f32 * 0.5).ceil() as usize;
    while chart.size() < cluster_majority {
        sleep(Duration::from_millis(100)).await;
    }
    info!("found majority of cluster, ({} nodes)", chart.size());
}

