use std::path::PathBuf;

use color_eyre::Result;
use tokio::time::timeout;
use tracing::{info, instrument};

use crate::directory::Directory;
use crate::raft::{Log, ObserverLog, HB_TIMEOUT};
use crate::redirectory::ReDirectory;
use crate::{minister, president, Id, Role};

mod clients;
mod locks;

/// # Note:
/// This function is NOT CANCEL SAFE. It can be canceld in redirectory.update
/// leading to the order never being processed
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
async fn keep_dir_updated(
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

#[instrument(skip_all)]
async fn update_dir(orders: &mut ObserverLog<minister::Order>, dir: &mut Directory) -> Result<()> {
    loop {
        let order = match timeout(HB_TIMEOUT, orders.recv()).await {
            Ok(order) => order,
            // timed out, either we are up to date (no orders recieved for a while)
            // or we lost connection to the minister
            Err(_time_out) => return Ok(()),
        };
        dir.update(order.order.clone());

        // reached fresh orders => now up to date
        if !order.perished() {
            return Ok(());
        }
    }
}

async fn update_then_handle_requests(
    client_requests: impl futures::Future<Output = ()>,
    min_orders: &mut ObserverLog<minister::Order>,
    mut directory: Directory,
) {
    info!("updating dir");
    let update_dir = update_dir(min_orders, &mut directory);
    update_dir.await.unwrap();
    info!("dir updated");

    let keep_dir_updated = keep_dir_updated(min_orders, directory);
    tokio::select! {
        res = client_requests => unreachable!("no error should happen: {res:?}"),
        res = keep_dir_updated => unreachable!("no error should happen: {res:?}"),
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
    let update_then_requests = update_then_handle_requests(client_requests, min_orders, directory);
    tokio::select! {
        new_role = pres_orders => new_role,
        res = update_then_requests => unreachable!("no error should happen: {res:?}"),
    }
}
