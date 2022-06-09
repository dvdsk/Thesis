use color_eyre::eyre::eyre;
use color_eyre::{Result, Section, SectionExt};

use crate::directory::{Node, ReDirectory};
use crate::president::{Log, Order};
use crate::{Id, Role};

async fn handle_pres_orders(
    pres_orders: &mut Log,
    redirectory: &mut ReDirectory,
    id: Id,
) -> Result<Role> {
    loop {
        let order = pres_orders.recv().await;
        redirectory.update(&order).await;
        match order {
            Order::AssignMinistry { subtree, staff } => {
                if staff.minister.id == id {
                    return Ok(Role::Minister {
                        term: staff.term,
                        subtree,
                        clerks: staff.clerks,
                    });
                }

                if staff.clerks.contains(&Node::local(id)) {
                    return Ok(Role::Clerk { subtree });
                }
            }
            Order::Assigned(role) => return Ok(role),
            Order::BecomePres { term } => return Ok(Role::President { term }),
            m => {
                return Err(eyre!("recieved wrong msg"))
                    .with_section(move || format!("{m:?}").header("Msg:"))
            }
        }
    }
}

pub(crate) async fn work(state: &mut super::State) -> Result<Role> {
    let super::State {
        pres_orders,
        redirectory,
        id,
        ..
    } = state;

    redirectory.set_tree(None);
    handle_pres_orders(pres_orders, redirectory, *id).await
}
