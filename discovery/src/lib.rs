use std::convert::TryInto;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::time::sleep;
use tracing::info;

pub use dashmap;
type Id = u64;

#[derive(Debug, Clone)]
pub struct Chart {
    id: Id,
    map: Arc<dashmap::DashMap<Id, SocketAddr>>,
    port_to_store: u16,
}

impl Chart {
    pub fn add_response(&self, buf: &[u8], mut addr: SocketAddr) {
        let id = u64::from_be_bytes(buf.try_into().expect("incorrect length"));
        if id == self.id {
            return;
        }
        addr.set_port(self.port_to_store);
        let old_key = self.map.insert(id, addr);
        if old_key.is_none() {
            info!("added new address: {:?}, total: ({})", addr, self.len());
        }
    }
    pub fn adresses(&self) -> Vec<SocketAddr> {
        self.map.iter().map(|m| m.value().clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn our_id(&self) -> u64 {
        self.id
    }
}

#[tracing::instrument]
async fn awnser_incoming(sock: &UdpSocket, chart: &Chart) {
    let mut buf = [0; 1024];
    loop {
        let (len, addr) = sock.recv_from(&mut buf).await.unwrap();
        sock.send_to(&chart.id.to_ne_bytes(), addr).await.unwrap();
        chart.add_response(&buf[0..len], addr);
    }
}

#[tracing::instrument]
async fn sleep_then_request_responses(sock: &UdpSocket, period: Duration, id: Id) {
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::SmallRng::from_entropy();

    loop {
        let random_sleep = rng.gen_range(Duration::from_secs(0)..period);
        sleep(random_sleep).await;
        request_responses(&sock, period, id).await;
    }
}

#[tracing::instrument]
pub async fn maintain(sock: UdpSocket, chart: Chart) {
    let f1 = awnser_incoming(&sock, &chart);
    let f2 = sleep_then_request_responses(&sock, Duration::from_secs(5), chart.id);
    futures::join!(f1, f2);
}

#[tracing::instrument]
async fn request_responses(sock: &UdpSocket, period: Duration, id: Id) {
    let multiaddr = Ipv4Addr::from([224, 0, 0, 251]);
    loop {
        sleep(period).await;
        let _len = sock
            .send_to(&id.to_ne_bytes(), (multiaddr, 8080))
            .await
            .unwrap();
    }
}

#[tracing::instrument]
async fn listen_for_response(sock: &UdpSocket, chart: &Chart, cluster_majority: usize) {
    let mut buf = [0; 1024];
    while chart.len() < cluster_majority {
        let (len, addr) = sock.recv_from(&mut buf).await.unwrap();
        chart.add_response(&buf[0..len], addr);
    }
}

#[tracing::instrument]
pub async fn cluster(chart: &Chart, full_size: u16) {
    assert!(full_size > 2, "minimal cluster size is 3");

    let cluster_majority = (full_size as f32 * 0.5).ceil() as usize;
    while chart.len() < cluster_majority {
        sleep(Duration::from_millis(100)).await;
    }

    info!("found majority of cluster, ({} nodes)", chart.len());
}

pub async fn setup(id: Id, port_to_store: u16) -> (UdpSocket, Chart) {
    let interface = Ipv4Addr::from([0, 0, 0, 0]);
    let multiaddr = Ipv4Addr::from([224, 0, 0, 251]);

    let sock = UdpSocket::bind((interface, 8080)).await.unwrap();
    sock.join_multicast_v4(multiaddr, interface).unwrap();
    sock.set_broadcast(true).unwrap();

    let chart = Chart {
        port_to_store,
        id,
        map: Arc::new(dashmap::DashMap::new()),
    };
    (sock, chart)
}
