use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;
use tracing::debug;

mod interval;
use interval::Interval;
use tracing::trace;

use crate::Id;
mod builder;
pub use builder::ChartBuilder;

use self::interval::Until;

#[derive(Debug, Clone)]
pub struct Chart {
    header: u64,
    service_id: Id,
    service_port: u16,
    sock: Arc<UdpSocket>,
    interval: Interval,
    map: Arc<dashmap::DashMap<Id, SocketAddr>>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
struct DiscoveryMsg {
    header: u64,
    id: Id,
    port: u16,
}

impl Chart {
    #[tracing::instrument]
    fn process_buf(&self, buf: &[u8], mut addr: SocketAddr) -> bool {
        let DiscoveryMsg { header, port, id } = bincode::deserialize(buf).unwrap();
        if header != self.header {
            return false;
        }
        if id == self.service_id {
            return false;
        }
        addr.set_port(port);
        let old_key = self.map.insert(id, addr);
        if old_key.is_none() {
            debug!(
                "added node: id: {id}, address: {addr:?}, n discoverd: ({})",
                self.size()
            );
            true
        } else {
            false
        }
    }
    pub fn adresses(&self) -> Vec<SocketAddr> {
        self.map.iter().map(|m| *m.value()).collect()
    }

    /// members discoverd including self
    pub fn size(&self) -> usize {
        self.map.len() + 1
    }

    pub fn our_id(&self) -> u64 {
        self.service_id
    }

    pub fn discovery_port(&self) -> u16 {
        self.sock.local_addr().unwrap().port()
    }

    fn discovery_msg(&self) -> DiscoveryMsg {
        DiscoveryMsg {
            header: self.header,
            id: self.service_id,
            port: self.service_port,
        }
    }
    fn discovery_buf(&self) -> Vec<u8> {
        let msg = self.discovery_msg();
        bincode::serialize(&msg).unwrap()
    }

    fn broadcast_soon(&mut self) -> bool {
        let next = self.interval.next();
        next.until() < Duration::from_millis(100)
    }
}

#[tracing::instrument]
pub async fn handle_incoming(mut chart: Chart) {
    let mut buf = [0; 1024];
    loop {
        let (len, addr) = chart.sock.recv_from(&mut buf).await.unwrap();
        trace!("got msg from: {addr:?}");
        let was_uncharted = chart.process_buf(&buf[0..len], addr);
        if was_uncharted && !chart.broadcast_soon() {
            chart
                .sock
                .send_to(&chart.discovery_buf(), addr)
                .await
                .unwrap();
        }
    }
}

#[tracing::instrument]
pub async fn broadcast_periodically(mut chart: Chart, period: Duration) {
    loop {
        chart.interval.sleep_till_next().await;
        trace!("sending discovery msg");
        broadcast(&chart.sock, &chart.discovery_buf()).await;
    }
}

#[tracing::instrument]
async fn broadcast(sock: &Arc<UdpSocket>, msg: &[u8]) {
    let multiaddr = Ipv4Addr::from([224, 0, 0, 251]);
    let _len = sock.send_to(msg, (multiaddr, 8080)).await.unwrap();
}

#[tracing::instrument]
async fn listen_for_response(chart: &Chart, cluster_majority: usize) {
    let mut buf = [0; 1024];
    while chart.size() < cluster_majority {
        let (len, addr) = chart.sock.recv_from(&mut buf).await.unwrap();
        chart.process_buf(&buf[0..len], addr);
    }
}
