use std::net::Ipv4Addr;

use tokio::net::UdpSocket;

const MULTIADDR: [u8;4] = [224, 0, 0, 251];

pub async fn discover_cluster() {
}

pub async fn maintain() {
    let interface = Ipv4Addr::from([0,0,0,0]);
    let sock = UdpSocket::bind((interface,8080)).await.unwrap();
    sock.join_multicast_v4(MULTIADDR.into(), interface).unwrap();
    sock.set_broadcast(true).unwrap();

    let mut buf = [0; 1024];
    loop {
        let (len, addr) = sock.recv_from(&mut buf).await.unwrap();
        println!("{:?} bytes received from {:?}", len, addr);

        let len = sock.send_to(&buf[..len], addr).await.unwrap();
        println!("{:?} bytes sent", len);
    }
}



// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[tokio::test]
//     async fn multicast_works() {
//         let f1 = maintain
//         let f2 = maintain
//         futures::join!(f1,f2);
    
//     }
// }
