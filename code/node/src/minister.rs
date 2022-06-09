use std::path::PathBuf;

use color_eyre::Result;
use tokio::sync::{broadcast, mpsc};
use tracing::info;

use crate::directory::{Node, ReDirectory};
use crate::president::subjects;
use crate::president::{Log, Order};
use crate::raft::LogWriter;
use crate::{Id, Role, Term};

mod clerks;
mod client;

async fn handle_pres_orders(
    pres_orders: &mut Log,
    our_id: Id,
    our_subtree: &PathBuf,
    mut register: clerks::Register,
    mut redirectory: ReDirectory,
) -> Result<Role> {
    loop {
        let order = pres_orders.recv().await;
        redirectory.update(&order).await;
        match order {
            Order::None => todo!(),
            Order::Assigned(_) => todo!(),
            Order::BecomePres { term } => return Ok(Role::President { term }),
            Order::ResignPres => unreachable!(),
            Order::AssignMinistry { subtree, staff } => {
                if staff.minister.id == our_id && subtree != *our_subtree {
                    return Ok(Role::Minister {
                        subtree,
                        clerks: staff.clerks,
                        term: staff.term,
                    });
                }

                if staff.clerks.contains(&Node::local(our_id)) {
                    return Ok(Role::Clerk { subtree });
                }

                register.update(staff.clerks);
            }
            #[cfg(test)]
            Order::Test(_) => todo!(),
        }
    }
}

pub(crate) async fn work(
    state: &mut super::State,
    our_subtree: PathBuf,
    clerks: Vec<Node>,
    term: Term,
) -> Result<Role> {
    let super::State {
        pres_orders,
        min_orders,
        redirectory,
        client_listener,
        id: our_id,
        ..
    } = state;
    info!("started work as minister: {our_id}");

    let (register, mut clerks) = clerks::Map::new(clerks, *our_id);

    let Log { state, .. } = min_orders;

    redirectory.set_tree(Some(our_subtree.clone()));

    let (broadcast, _) = broadcast::channel(16);
    let (tx, notify_rx) = mpsc::channel(16);
    let log_writer = LogWriter {
        term,
        state: state.clone(),
        broadcast: broadcast.clone(),
        notify_tx: tx,
    };

    let instruct_subjects = subjects::instruct(
        &mut clerks,
        broadcast.clone(),
        notify_rx,
        subjects::EmptyNotifier,
        state.clone(),
        term,
    );

    let pres_orders = handle_pres_orders(
        pres_orders,
        *our_id,
        &our_subtree,
        register,
        redirectory.clone(),
    );
    let client_requests =
        client::handle_requests(client_listener, log_writer, &our_subtree, redirectory);

    tokio::select! {
        new_role = pres_orders => return new_role,
        () = instruct_subjects => unreachable!(),
        () = client_requests => unreachable!(),
    };
}
