use std::path::PathBuf;

use color_eyre::Result;

use crate::redirectory::{Node, ReDirectory};
use crate::raft::Log;
use crate::{Id, Role, president};

mod clients;

async fn handle_pres_orders(
    pres_orders: &mut Log<president::Order>,
    our_id: Id,
    our_subtree: PathBuf,
    redirectory: &mut ReDirectory,
) -> Result<Role> {
    loop {
        use president::Order;
        let order = pres_orders.recv().await;
        redirectory.update(&order).await;
        match order {
            Order::None => todo!(),
            Order::Assigned(_) => todo!(),
            Order::BecomePres { term } => return Ok(Role::President { term }),
            Order::ResignPres => unreachable!(),
            Order::AssignMinistry { subtree, staff } => {
                if staff.minister.id == our_id {
                    return Ok(Role::Minister {
                        subtree,
                        clerks: staff.clerks,
                        term: staff.term,
                    });
                }
                if staff.clerks.contains(&Node::local(our_id)) && subtree != *our_subtree {
                    return Ok(Role::Clerk { subtree });
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
    state: &mut super::State,
    our_tree: PathBuf,
) -> Result<Role> {
    let super::State {
        pres_orders,
        redirectory,
        client_listener,
        id,
        ..
    } = state;

    redirectory.set_tree(Some(our_tree.clone()));
    let client_requests = clients::handle_requests(client_listener, our_tree.clone(), redirectory.clone());
    let pres_orders = handle_pres_orders(pres_orders, *id, our_tree, redirectory);
    // let minister_orders = handle_minister_orders(); // Q how about minister change...

    tokio::select! {
        new_role = pres_orders => return new_role,
        _ = client_requests => unreachable!(),
        // _ = minister_orders => unreachable!(),
    };
}
