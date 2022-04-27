use color_eyre::eyre::{eyre, WrapErr};
use color_eyre::{Result, Section, SectionExt};

use crate::president::{Log, Order};
use crate::Role;

pub(crate) async fn work(pres_orders: &mut Log) -> Result<Role> {
    match pres_orders.recv().await {
        Order::Assigned(role) => Ok(role),
        Order::BecomePres => Ok(Role::President),
        m => {
            Err(eyre!("recieved wrong msg")).with_section(move || format!("{m:?}").header("Msg:"))
        }
    }
}