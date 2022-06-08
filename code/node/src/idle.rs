use color_eyre::eyre::eyre;
use color_eyre::{Result, Section, SectionExt};

use crate::directory::Node;
use crate::president::{Log, Order};
use crate::{Id, Role};

pub(crate) async fn work(pres_orders: &mut Log, id: Id) -> Result<Role> {
    loop {
        match pres_orders.recv().await {
            Order::AssignMinistry { subtree, staff } => {
                // TODO update cluster_directory
                if staff.minister.id == id {
                    return Ok(Role::Minister {
                        term: staff.term,
                        subtree,
                        clerks: staff.clerks,
                    });
                }

                if staff.clerks.contains(&Node::local(id)) {
                    return Ok(Role::Clerk);
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
