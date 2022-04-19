use tokio::time::sleep;
use std::time::Duration;

use tokio::net::TcpSocket;

use crate::president::Log;

pub(crate) async fn work(pres_orders: &mut Log, socket: &mut TcpSocket) -> crate::Role {
    sleep(Duration::from_secs(20)).await;
    todo!()
}
