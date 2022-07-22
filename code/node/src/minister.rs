use std::path::PathBuf;
use std::sync::atomic::{self, AtomicU32};
use std::sync::Arc;

use color_eyre::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tracing::{info, instrument, Instrument};

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
    term: AtomicTerm,
) -> Result<Role> {
    use crate::president::Order::*;

    loop {
        let order = pres_orders.recv().await;
        redirectory.update(&order.order).await;
        match order.order.clone() {
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

                if subtree == *our_subtree {
                    register.update(staff.clerks);
                    term.update(staff.term);
                }
            }
            #[cfg(test)]
            Test(_) => todo!(),
        }

        if order.perished() {
            return Err(order.error());
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

use crate::raft::Perishable;
async fn apply_own_order(orders: &mut mpsc::Receiver<Perishable<Order>>) -> color_eyre::Result<()> {
    loop {
        let order = orders
            .recv()
            .await
            .expect("channel was dropped, this means another thread panicked. Joining the panic");

        if order.perished() {
            return Err(order.error());
        }

        // no need to process our own orders, if we become clerk we delete the
        // directory and build it fresh from the log
    }
}

#[derive(Debug, Clone)]
pub struct AtomicTerm {
    current: Arc<AtomicU32>,
    prev: u32,
}

use self::raft::subjects::GetTerm;
impl GetTerm for AtomicTerm {
    fn curr(&mut self) -> Term {
        let curr = self.current.load(atomic::Ordering::SeqCst);
        self.prev = curr;
        curr
    }
    fn prev(&mut self) -> Term {
        self.prev
    }
}

impl AtomicTerm {
    fn new(initial: Term) -> Self {
        Self {
            current: Arc::new(AtomicU32::new(initial)),
            prev: initial,
        }
    }
    fn update(&self, new: Term) {
        self.current.store(new, atomic::Ordering::SeqCst);
    }
}

#[instrument(skip(state))]
pub(crate) async fn work(
    state: &mut super::State,
    our_subtree: PathBuf,
    clerks: Vec<Node>,
    initial_term: Term,
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

    let ObserverLog { state, orders, .. } = min_orders;

    redirectory.set_tree(Some(our_subtree.clone()));
    let directory = Directory::from_committed(state, db);

    let (broadcast, _) = broadcast::channel(16);
    let (tx, notify_rx) = mpsc::channel(16);

    let term = AtomicTerm::new(initial_term);
    let log_writer = LogWriter {
        term: term.clone(),
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
        term.clone(),
    )
    .in_current_span();

    let pres_orders = handle_pres_orders(
        pres_orders,
        *our_id,
        &our_subtree,
        register,
        redirectory.clone(),
        term,
    );

    let (lock_manager, (lock_rx, unlock_rx)) = LockManager::new();
    let update_read_locks = read_locks::maintain_file_locks(clerks_copy, lock_rx, unlock_rx);

    let client_requests = client::handle_requests(
        client_listener,
        log_writer,
        &our_subtree,
        redirectory,
        directory,
        lock_manager,
    );

    // TODO FIXME is there a read own orders?
    tokio::select! {
        new_role = pres_orders => new_role,
        _ = apply_own_order(orders) => unreachable!("orders are not popped from queue fast enough"),
        () = instruct_subjects => unreachable!(),
        () = client_requests => unreachable!(),
        () = update_read_locks => unreachable!(),
    }
}
