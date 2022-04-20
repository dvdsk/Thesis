use tokio::net::TcpListener;

use crate::president::Log;

pub(crate) fn work(_pres_orders: &mut Log, _socket: &mut TcpListener) -> crate::Role {
    todo!()
}
