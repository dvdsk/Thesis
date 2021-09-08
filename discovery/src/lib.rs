use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::time::sleep;

pub async fn discover_cluster() {
}

async fn read(sock: Arc<UdpSocket>) {
    let mut buf = [0; 1024];
    loop {
        let (len, addr) = sock.recv_from(&mut buf).await.unwrap();
        println!("{:?} bytes received from {:?}", len, addr);
    }
}

async fn write(sock: Arc<UdpSocket>) {
    let multiaddr = Ipv4Addr::from([224, 0, 0, 251]);
    loop {
        sleep(Duration::from_secs(1)).await;
        let len = sock.send_to("hello world".as_bytes(), (multiaddr,8080)).await.unwrap();
        println!("{:?} bytes sent", len);
    }
}

pub async fn maintain() {
    let interface = Ipv4Addr::from([0,0,0,0]);
    let multiaddr = Ipv4Addr::from([224, 0, 0, 251]);

    let sock = UdpSocket::bind((interface,8080)).await.unwrap();
    sock.join_multicast_v4(multiaddr, interface).unwrap();
    sock.set_broadcast(true).unwrap();

    sock.connect((multiaddr,8080));
    let sock = Arc::new(sock);

    futures::join!(write(sock.clone()), read(sock));

}
