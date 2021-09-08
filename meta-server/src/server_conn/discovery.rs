use std::collections::HashSet;
use std::time::Duration;

use crate::server_conn::MDNS_NAME;
use futures::{StreamExt, pin_mut};
use libmdns::Responder;
use tracing::info;

/// newly joining the cluster
/// figure out ips of > 50% of the cluster. If we can not
/// the entire cluster has failed or we have a bad connection.
pub async fn discover_cluster(size: u16) {
    assert!(size > 2, "minimal cluster size is 3");
    let cluster_majority = (size as f32 * 0.5).ceil();
    let mut discovered = HashSet::with_capacity(size as usize);
    while discovered.len() < cluster_majority as usize {
        let query_interval = Duration::from_secs(1);
        let stream = mdns::discover::all(MDNS_NAME, query_interval)
            .unwrap()
            .listen();

        pin_mut!(stream);
        while let Some(Ok(response)) = stream.next().await {
            if let Some(addr) = response.ip_addr() {
                info!("discoverd new cluster member: {:?}", &addr);
                discovered.insert(addr);
            }
        }
    }
}

pub async fn maintain_discovery(port: u16) {
    let (responder, task) = Responder::with_default_handle().unwrap();
    let _srv = responder.register(MDNS_NAME.into(), MDNS_NAME.into(), 5353, &["lol"]);
    task.await;
    dbg!("mmm");
}
