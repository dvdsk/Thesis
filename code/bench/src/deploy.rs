use color_eyre::{Result, eyre::eyre};
use node::WrapErr;
use std::{process::Command, thread::sleep, time::Duration};

use crate::bench;

type Node = String;

pub fn reserve(n_nodes: usize) -> Result<Vec<Node>> {
    let (ticket, nodes) = reserve_nodes(n_nodes)?;
    wait_for_allocation(ticket)?;
    Ok(nodes)
}

fn reserve_nodes(n: usize) -> Result<(usize, Vec<Node>)> {
    let time = "00:15:00";
    let output = Command::new("preserve")
        .arg("-#")
        .arg(n.to_string())
        .arg("-t")
        .arg(time)
        .output()?;

    dbg!(output);
    todo!("get reservation number");
}

fn node_list() -> Result<String> {
    let output = Command::new("preserve").arg("-long-list").output()?;
    Ok(String::from_utf8(output.stdout)?)
}

fn wait_for_allocation(ticket: usize) -> Result<()> {
    let ticket = ticket.to_string();
    loop {
        let status = node_list()
            .wrap_err("could not get node list")?
            .lines()
            .find(|line| line.contains(&ticket))
            .ok_or_else(|| eyre!("ticket not in reservation list"));

        sleep(Duration::from_millis(250));
    }
}

pub fn start_cluster(command: bench::Command, nodes: &[Node]) -> Result<()> {
    Ok(())
}

pub(crate) fn start_clients(client_nodes: &[Node]) -> Result<()> {
    Ok(())
}
