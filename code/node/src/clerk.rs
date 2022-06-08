use tokio::net::TcpListener;
use color_eyre::Result;

use crate::{Id, Role};
use crate::president::{Log, Order};

async fn handle_client_requests(socket: &mut TcpListener) {
    for _stream in socket.accept().await {
        todo!();
    }
}

async fn handle_pres_orders(
    pres_orders: &mut Log,
    our_id: Id,
) -> Result<Role> {
    loop {
        match pres_orders.recv().await {
            Order::None => todo!(),
            Order::Assigned(_) => todo!(),
            Order::BecomePres { term } => return Ok(Role::President { term }),
            Order::ResignPres => unreachable!(),
            Order::AssignMinistry { subtree, staff } => {
                // TODO update cluster_directory
                if staff.minister.id == our_id {
                    return Ok(Role::Minister {
                        subtree,
                        clerks: staff.clerks,
                        term: staff.term,
                    });
                }
            }
            #[cfg(test)]
            Order::Test(_) => todo!(),
        }
    }
}

// async fn handle_minister_orders() {
//     loop {
//     }
// }

pub(crate) async fn work(
    pres_orders: &mut Log,
    _min_orders: &mut Log,
    socket: &mut TcpListener,
    our_id: Id,
) -> Result<Role> {

    let client_requests = handle_client_requests(socket);
    let pres_orders = handle_pres_orders(pres_orders, our_id);
    // let minister_orders = handle_minister_orders(); // Q how about minister change...

    tokio::select! {
        new_role = pres_orders => return new_role,
        _ = client_requests => unreachable!(),
        // _ = minister_orders => unreachable!(),
    };
}
