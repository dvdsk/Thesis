//! Simple lightweight local service discovery for testing
//!
//! This crate provides a lightweight alternative to mDNS. It discovers other instances on the
//! same machine or network. There is no need to set up a server no decide on a name.
//!
//! However is something else is causing 
//!
//! ## Usage
//!
//! Add a dependency on `udiscovery` in `Cargo.toml`:
//!
//! ```toml
//! multicast-discovery = "0.1"
//! ```!
//!
//! Now 

use std::io;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rand::{Rng, SeedableRng};
use tokio::net::UdpSocket;
use tokio::time::sleep;
use tracing::info;
use serde::{Serialize, Deserialize};

mod builder;
pub use builder::ChartBuilder;

pub use dashmap;
type Id = u64;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Error setting up bare socket")]
    Construct(io::Error),
    #[error("Error not set Reuse flag on the socket")]
    SetReuse(io::Error),
    #[error("Error not set Broadcast flag on the socket")]
    SetBroadcast(io::Error),
    #[error("Error not set Multicast flag on the socket")]
    SetMulticast(io::Error),
    #[error("Error not set NonBlocking flag on the socket")]
    SetNonBlocking(io::Error),
    #[error("Error binding to socket")]
    Bind(io::Error),
    #[error("Error joining multicast network")]
    JoinMulticast(io::Error),
    #[error("Error transforming to async socket")]
    ToTokio(io::Error),
}

#[derive(Debug, Clone)]
pub struct Chart {
    header: u64,
    service_id: Id,
    service_port: u16,
    sock: Arc<UdpSocket>,
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
    pub fn process_buf(&self, buf: &[u8], mut addr: SocketAddr) {
        let DiscoveryMsg {header, port, id} = dbg!(bincode::deserialize(buf).unwrap());
        if header != self.header {
            return;
        }
        if id == self.service_id {
            return;
        }
        addr.set_port(port);
        let old_key = self.map.insert(id, addr);
        if old_key.is_none() {
            info!("added node: id: {id}, address: {addr:?}, n discoverd: ({})", self.size());
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
}

#[tracing::instrument]
async fn handle_incoming(chart: Chart) {
    let mut buf = [0; 1024];
    loop {
        info!("hi");
        let (len, addr) = chart.sock.recv_from(&mut buf).await.unwrap();
        info!("got msg from: {addr:?}");
        chart.process_buf(&buf[0..len], addr);
    }
}

#[tracing::instrument]
async fn send_periodically(chart: Chart, period: Duration) {
    let mut rng = rand::rngs::SmallRng::from_entropy();
    let msg = chart.discovery_msg();

    loop {
        let random_sleep = rng.gen_range(Duration::from_secs(5)..period);
        sleep(random_sleep).await;
        info!("sending");
        send(&chart.sock, msg).await;
    }
}

#[tracing::instrument]
pub async fn maintain(chart: Chart) {
    let f1 = tokio::spawn(handle_incoming(chart.clone()));
    let f2 = tokio::spawn(send_periodically(chart, Duration::from_secs(10)));
    let (_, _) = futures::join!(f1, f2);
    unreachable!("never returns")
}

#[tracing::instrument]
async fn send(sock: &Arc<UdpSocket>, msg: DiscoveryMsg) {
    let multiaddr = Ipv4Addr::from([224, 0, 0, 251]);
    let buf = bincode::serialize(&msg).unwrap();
    let _len = sock
        .send_to(&buf, (multiaddr, 8080))
        .await
        .unwrap();
}

#[tracing::instrument]
async fn listen_for_response(chart: &Chart, cluster_majority: usize) {
    let mut buf = [0; 1024];
    while chart.size() < cluster_majority {
        let (len, addr) = chart.sock.recv_from(&mut buf).await.unwrap();
        chart.process_buf(&buf[0..len], addr);
    }
}

#[tracing::instrument]
pub async fn found_everyone(chart: Chart, full_size: u16) {
    assert!(full_size > 2, "minimal cluster size is 3");

    while chart.size() < full_size.into() {
        sleep(Duration::from_millis(100)).await;
    }

    info!("found every member of the cluster, ({} nodes)", chart.size());
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
