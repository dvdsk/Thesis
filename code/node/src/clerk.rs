use std::path::PathBuf;

use color_eyre::Result;
use tracing::{info, instrument};

use crate::directory::Directory;
use crate::raft::{Log, ObserverLog};
use crate::redirectory::ReDirectory;
use crate::{minister, president, Id, Role};

mod clients;
mod locks;

async fn handle_pres_orders(
    pres_orders: &mut Log<president::Order>,
    our_id: Id,
    our_subtree: PathBuf,
    redirectory: &mut ReDirectory,
) -> Result<Role> {
    loop {
        use president::Order;
        let order = pres_orders.recv().await;
        redirectory.update(&order.order).await;
        match order.order.clone() {
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
                if staff.clerks.iter().any(|clerk| clerk.id == our_id) && subtree != *our_subtree {
                    return Ok(Role::Clerk { subtree });
                }
            }
            #[cfg(test)]
            Order::Test(_) => todo!(),
        }

        if order.perished() {
            return Err(order.error());
        }
    }
}

#[instrument(skip_all)]
async fn handle_minister_orders(
    orders: &mut ObserverLog<minister::Order>,
    mut dir: Directory,
) -> Result<()> {
    loop {
        let order = orders.recv().await;
        dir.update(order.order.clone());

        if order.perished() {
            return Err(order.error());
        }
    }
}

#[instrument(skip(state))]
pub(crate) async fn work(state: &mut super::State, our_subtree: PathBuf) -> Result<Role> {
    let super::State {
        pres_orders,
        min_orders,
        redirectory,
        client_listener,
        id,
        db,
        ..
    } = state;
    info!("started work as clerk");

    let ObserverLog { state, .. } = min_orders;

    redirectory.set_tree(Some(our_subtree.clone()));
    let directory = Directory::from_committed(state, db);

    let client_requests = clients::handle_requests(
        client_listener,
        state.clone(),
        our_subtree.clone(),
        redirectory.clone(),
        directory.clone(),
    );
    let pres_orders = handle_pres_orders(pres_orders, *id, our_subtree, redirectory);
    let minister_orders = handle_minister_orders(min_orders, directory);

    tokio::select! {
        new_role = pres_orders => new_role,
        _ = client_requests => unreachable!(),
        _ = minister_orders => unreachable!(),
    }
}
