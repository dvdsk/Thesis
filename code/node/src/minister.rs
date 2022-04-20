use tokio::net::TcpListener;

use crate::president::Log;

pub(crate) fn work(pres_orders: &mut Log, socket: &mut TcpListener) -> crate::Role {
    todo!()
}
