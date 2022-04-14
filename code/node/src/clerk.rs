use tokio::net::TcpSocket;

use crate::president::Log;

pub(crate) fn work(_pres_orders: &mut Log, _socket: &mut TcpSocket) -> crate::Role {
    todo!()
}
