use std::path::PathBuf;

use color_eyre::Result;
use tokio::net::TcpListener;

use crate::president::{Log, Order};
use crate::{Id, Role};

async fn handle_client_requests(socket: &mut TcpListener) {
    for _stream in socket.accept().await {
        todo!();
    }
}

async fn handle_pres_orders(
    pres_orders: &mut Log,
    our_id: Id,
    our_subtree: PathBuf,
) -> Result<Role> {
    loop {
        match pres_orders.recv().await {
            Order::None => todo!(),
            Order::Assigned(_) => todo!(),
            Order::BecomePres { term } => return Ok(Role::President { term }),
            Order::ResignPres => unreachable!(),
            Order::AssignMinistry { subtree, staff } => {
                // TODO update cluster_directory
                if staff.minister == our_id && subtree != our_subtree {
                    return Ok(Role::Minister {
                        subtree,
                        clerks: staff.clerks,
                    });
                }

                if staff.clerks.contains(&our_id) {
                    return Ok(Role::Clerk);
                }
            }
            #[cfg(test)]
            Order::Test(_) => todo!(),
        }
    }
}

pub(crate) async fn work(
    pres_orders: &mut Log,
    socket: &mut TcpListener,
    our_id: Id,
    our_subtree: PathBuf,
    _clerks: Vec<Id>,
) -> Result<Role> {

    let client_requests = handle_client_requests(socket);
    let pres_orders = handle_pres_orders(pres_orders, our_id, our_subtree);

    tokio::select! {
        new_role = pres_orders => return new_role,
        _ = client_requests => unreachable!(),
    };
}
