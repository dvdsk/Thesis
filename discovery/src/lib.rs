use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rand::{Rng, SeedableRng};
use tokio::net::UdpSocket;
use tokio::time::sleep;
use tokio::time::timeout;
use tokio::sync::Mutex;
use tracing::info;
use serde::{Serialize, Deserialize};

pub use dashmap;
use tracing::trace;
type Id = u64;

#[derive(Debug, Clone)]
pub struct Chart {
    id: Id,
    port: u16, // port the node is listening on for work
    map: Arc<dashmap::DashMap<Id, SocketAddr>>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
struct DiscoveryMsg {
    id: Id,
    port: u16,
}

impl Chart {
    pub fn add_response(&self, buf: &[u8], mut addr: SocketAddr) {
        let DiscoveryMsg {port, id} = bincode::deserialize(buf).unwrap();
        if id == self.id {
            return;
        }
        addr.set_port(port);
        let old_key = self.map.insert(id, addr);
        if old_key.is_none() {
            info!("added node: id: {id}, address: {addr:?}, n discoverd: ({})", self.len());
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

    fn discovery_msg(&self) -> DiscoveryMsg {
        DiscoveryMsg {
            id: self.id,
            port: self.port,
        }
    }
}

#[derive(Debug, Clone)]
struct FixedUdpSocket (Arc<Mutex<UdpSocket>>);
impl FixedUdpSocket {
    async fn recv(&self, buf: &mut [u8]) -> Result<(usize, std::net::SocketAddr), std::io::Error> {
        loop {
            sleep(Duration::from_millis(250)).await;
            let sock = self.0.lock().await;
            dbg!("got recv lock");
            let recv = sock.recv_from(buf);

            if let Ok(res) = timeout(Duration::from_millis(250), recv).await {
                dbg!("succesfully recvd");
                break res;
            }
            dbg!("timed out recv");
        }
    }
    async fn send_to(&self, buf: &[u8], addr: impl tokio::net::ToSocketAddrs) -> Result<usize, std::io::Error> {
        dbg!("trying to send msg");
        let sock = loop {
            match self.0.try_lock() {
                Ok(l) => break l,
                Err(_) => sleep(Duration::from_millis(100)).await,
            }
            dbg!("stuck trying to get lock");
        };
        // let sock = self.0.lock().await;
        dbg!("sending msg");
        sock.send_to(&buf, addr).await
    }
}


#[tracing::instrument]
async fn register_incoming(sock: FixedUdpSocket, chart: Chart) {
    let mut rng = rand::rngs::SmallRng::from_entropy();
    let mut buf = [0; 1024];
    loop {
        let random_sleep = rng.gen_range(Duration::from_secs(0)..Duration::from_secs(1));
        sleep(random_sleep).await;
        let (len, addr) = sock.recv(&mut buf).await.unwrap();
        // chart.add_response(&buf[0..len], addr);
    }
}

#[tracing::instrument]
async fn sleep_then_request_responses(sock: FixedUdpSocket, period: Duration, msg: DiscoveryMsg) {
    let mut rng = rand::rngs::SmallRng::from_entropy();

    loop {
        let random_sleep = rng.gen_range(Duration::from_secs(0)..period);
        sleep(random_sleep).await;
        request_respons(&sock, msg).await;
    }
}

#[tracing::instrument]
pub async fn maintain(sock: UdpSocket, chart: Chart) {
    let sock = FixedUdpSocket (Arc::new(Mutex::new(sock)));
    let msg = chart.discovery_msg();
    let f1 = tokio::spawn(register_incoming(sock.clone(), chart));
    let f2 = tokio::spawn(sleep_then_request_responses(sock, Duration::from_secs(5), msg));
    let (_, _) = futures::join!(f1, f2);
    unreachable!("never returns")
}

#[tracing::instrument]
async fn request_respons(sock: &FixedUdpSocket, msg: DiscoveryMsg) {
    let multiaddr = Ipv4Addr::from([224, 0, 0, 251]);
    let buf = bincode::serialize(&msg).unwrap();
    let _len = sock
        .send_to(&buf, (multiaddr, 8080))
        .await
        .unwrap();
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
pub async fn cluster(chart: Chart, full_size: u16) {
    assert!(full_size > 2, "minimal cluster size is 3");

    let cluster_majority = (full_size as f32 * 0.5).ceil() as usize;
    while chart.len() < cluster_majority {
        sleep(Duration::from_millis(100)).await;
    }

    info!("found majority of cluster, ({} nodes)", chart.len());
}

pub async fn setup(id: Id, port: u16) -> (UdpSocket, Chart) {
    let interface = Ipv4Addr::from([0, 0, 0, 0]);
    let multiaddr = Ipv4Addr::from([224, 0, 0, 251]);

    use socket2::{Socket, Domain, Type, SockAddr};
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, None).unwrap();
    sock.set_reuse_port(true).unwrap(); // allow binding to a port already in use
    sock.set_broadcast(true).unwrap(); // enable udp broadcasting
    sock.set_multicast_loop_v4(true).unwrap(); // send broadcast to self

    let address = SocketAddr::from((interface, 8080));
    let address = SockAddr::from(address);
    sock.bind(&address).unwrap();
    sock.join_multicast_v4(&multiaddr, &interface).unwrap();

    let sock = std::net::UdpSocket::from(sock); 
    sock.set_nonblocking(true).unwrap();
    let sock = UdpSocket::from_std(sock).unwrap();

    let chart = Chart {
        port,
        id,
        map: Arc::new(dashmap::DashMap::new()),
    };
    (sock, chart)
}
