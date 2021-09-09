use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::time::sleep;
use tracing::info;
use tracing_futures::Instrument;

pub use dashmap;
type Id = String;

#[derive(Debug)]
pub struct Chart {
    id: Id,
    map: dashmap::DashMap<Id, SocketAddr>,
}

impl Chart {
    pub fn add_response(&self, buf: &[u8], addr: SocketAddr) {
        let id = std::str::from_utf8(buf)
            .expect("Id should be printable utf8")
            .to_string();
        if id == self.id {
            return;
        }
        let old_key = self.map.insert(id, addr);
        if old_key.is_none() {
            info!("added new address: {:?}, total: ({})", addr, self.len());
        }
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
}

#[tracing::instrument]
async fn awnser_incoming(sock: &UdpSocket, chart: &Chart) {
    let mut buf = [0; 1024];
    loop {
        let (len, addr) = sock.recv_from(&mut buf).await.unwrap();
        sock.send_to(chart.id.as_bytes(), addr).await.unwrap();
        chart.add_response(&buf[0..len], addr);
    }
}

#[tracing::instrument]
async fn sleep_then_request_responses(sock: &UdpSocket, period: Duration, id: &str) {
    use rand::{Rng, SeedableRng};
    let mut rng = rand::rngs::SmallRng::from_entropy();
    let random_sleep = rng.gen_range(Duration::from_secs(0)..period);

    sleep(random_sleep).await;
    request_responses(&sock, period, id).await;
}

#[tracing::instrument]
pub async fn maintain(sock: &UdpSocket, chart: &Chart) {
    let f1 = awnser_incoming(sock, &chart);
    let f2 = sleep_then_request_responses(sock, Duration::from_secs(5), &chart.id);
    futures::join!(f1, f2);
}

#[tracing::instrument]
async fn request_responses(sock: &UdpSocket, period: Duration, id: &str) {
    let multiaddr = Ipv4Addr::from([224, 0, 0, 251]);
    loop {
        sleep(period).await;
        let len = sock
            .send_to(id.as_bytes(), (multiaddr, 8080))
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
pub async fn cluster(sock: &UdpSocket, chart: &Chart, full_size: u16) {
    assert!(full_size > 2, "minimal cluster size is 3");
    let cluster_majority = (full_size as f32 * 0.5).ceil() as usize;

    let f1 = request_responses(&sock, Duration::from_millis(500), &chart.id);
    let f2 = listen_for_response(&sock, &chart, cluster_majority);

    use futures::FutureExt;
    futures::select!(
        _ = f1.fuse() => (),
        _ = f2.fuse() => (),
    );
    info!("found majority of cluster, ({} nodes)", chart.len());
}

pub async fn setup(id: Id) -> (UdpSocket, Chart) {
    let interface = Ipv4Addr::from([0, 0, 0, 0]);
    let multiaddr = Ipv4Addr::from([224, 0, 0, 251]);

    let sock = UdpSocket::bind((interface, 8080)).await.unwrap();
    sock.join_multicast_v4(multiaddr, interface).unwrap();
    sock.set_broadcast(true).unwrap();

    let chart = Chart {
        id,
        map: dashmap::DashMap::new(),
    };
    (sock, chart)
}
