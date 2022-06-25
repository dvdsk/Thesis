use std::path::PathBuf;

use color_eyre::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tracing::{info, Instrument, instrument};

use crate::directory::Directory;
use crate::minister::read_locks::LockManager;
use crate::raft::LogWriter;
use crate::raft::{self, subjects};
use crate::raft::{Log, ObserverLog};
use crate::redirectory::{Node, ReDirectory};
use crate::{president, Id, Role, Term};

mod clerks;
mod client;
mod read_locks;

async fn handle_pres_orders(
    pres_orders: &mut Log<president::Order>,
    our_id: Id,
    our_subtree: &PathBuf,
    mut register: clerks::Register,
    mut redirectory: ReDirectory,
) -> Result<Role> {
    use crate::president::Order::*;

    loop {
        let order = pres_orders.recv().await;
        redirectory.update(&order).await;
        match order {
            None => todo!(),
            Assigned(_) => todo!(),
            BecomePres { term } => return Ok(Role::President { term }),
            ResignPres => unreachable!(),
            AssignMinistry { subtree, staff } => {
                if staff.minister.id == our_id && subtree != *our_subtree {
                    return Ok(Role::Minister {
                        subtree,
                        clerks: staff.clerks,
                        term: staff.term,
                    });
                }
                if staff.clerks.iter().any(|clerk| clerk.id == our_id) {
                    return Ok(Role::Clerk { subtree });
                }

                register.update(staff.clerks);
            }
            #[cfg(test)]
            Test(_) => todo!(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Order {
    Create(PathBuf),
    Remove(PathBuf),
    None,
}

impl raft::Order for Order {
    fn none() -> Self {
        Self::None
    }

    fn elected(_: Term) -> Self {
        unreachable!("Ministers can not be elected")
    }

    fn resign() -> Self {
        unreachable!("Ministers can not be risigned by ministers")
    }
}

#[instrument(skip(state))]
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
        db,
        id: our_id,
        ..
    } = state;
    info!("started work as minister: {our_id}");

    let (register, mut clerks, clerks_copy) = clerks::RaftMap::new(clerks, *our_id);

    let ObserverLog { state, .. } = min_orders;

    redirectory.set_tree(Some(our_subtree.clone()));
    let directory = Directory::from_committed(state, db);

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
    )
    .in_current_span();

    let pres_orders = handle_pres_orders(
        pres_orders,
        *our_id,
        &our_subtree,
        register,
        redirectory.clone(),
    );

    let (lock_manager, rx) = LockManager::new();
    let update_read_locks = read_locks::maintain_file_locks(clerks_copy, rx);

    let client_requests = client::handle_requests(
        client_listener,
        log_writer,
        &our_subtree,
        redirectory,
        directory,
        lock_manager,
    );

    tokio::select! {
        new_role = pres_orders => new_role,
        () = instruct_subjects => unreachable!(),
        () = client_requests => unreachable!(),
        () = update_read_locks => unreachable!(),
    }
}
