use futures_util::{pin_mut, StreamExt};
use tokio::net::TcpStream;
use std::net::IpAddr;
use std::time::Duration;

use crate::server_conn::{MDNS_NAME, write::WriteServer};

pub async fn discover_ws() -> IpAddr {
    loop {
        let query_interval = Duration::from_secs(1);
        let stream = mdns::discover::all(MDNS_NAME, query_interval)
            .unwrap()
            .listen();

        pin_mut!(stream);
        while let Some(Ok(response)) = stream.next().await {
            if let Some(addr) = response.ip_addr() {
                return addr;
            }
        }
    }
}

pub async fn server(port: u16) {
    let addr = discover_ws().await;
    let ws = WriteServer::from_addr((addr, port)).await;
}
