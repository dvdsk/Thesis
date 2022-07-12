use color_eyre::{eyre::eyre, Help, Report, Result};
use futures::stream::FuturesUnordered;
use node::WrapErr;
use std::{
    env,
    future::Future,
    process::{Command, Output},
    thread::sleep,
    time::Duration,
};
use tracing::{info, instrument};

use crate::bench::{self, Bench, Partition};

type Node = String;

#[instrument]
pub fn reserve(n_nodes: usize) -> Result<Vec<Node>> {
    cancel_existing()?;
    let ticket = reserve_nodes(n_nodes)?;
    wait_for_allocation(ticket)?;
    let nodes = get_nodes(ticket)?;
    Ok(nodes)
}

#[instrument]
fn cancel_existing() -> Result<()> {
    let output = Command::new("preserve")
        .arg("-llist")
        .output()
        .wrap_err("Could not find command")?
        .wrap()?;

    let tickets = output
        .lines()
        .skip(3)
        .map(|l| l.split_once('\t').unwrap().0)
        .map(|d| d.parse::<usize>().unwrap());

    for ticket in tickets {
        let _ = Command::new("preserve")
            .arg("-c")
            .arg(ticket.to_string())
            .output()
            .wrap_err("Could not find command")?
            .wrap()?;
    }
    Ok(())
}

#[instrument(ret)]
fn get_nodes(ticket: usize) -> Result<Vec<Node>> {
    let list = node_list()?;
    let list: Vec<_> = list
        .lines()
        .find(|line| line.contains(&ticket.to_string()))
        .ok_or_else(|| eyre!("reservation not found"))?
        .split_whitespace()
        .skip(8)
        .map(ToOwned::to_owned)
        .collect();
    Ok(list)
}

#[instrument(err)]
fn reserve_nodes(n: usize) -> Result<usize> {
    let time = "00:15:00";
    let output = Command::new("preserve")
        .arg("-#")
        .arg(n.to_string())
        .arg("-t")
        .arg(time)
        .output()
        .wrap_err("Could not find command")?
        .wrap()?;

    let ticket: usize = output
        .split_whitespace()
        .nth(2)
        .ok_or_else(|| eyre!("Missing whitespace in output"))?
        .split_once(':')
        .ok_or_else(|| eyre!("Missing ':' in output"))?
        .0
        .parse()?;

    info!("reservation: {ticket}");

    Ok(ticket)
}

#[instrument]
fn node_list() -> Result<String> {
    Ok(Command::new("preserve")
        .arg("-long-list")
        .output()
        .wrap_err("Could not find command")?
        .wrap()?)
}

#[instrument(ret)]
fn wait_for_allocation(ticket: usize) -> Result<()> {
    let ticket = ticket.to_string();
    loop {
        let list = node_list().wrap_err("could not get node list")?;
        let status = list
            .lines()
            .find(|line| line.contains(&ticket))
            .ok_or_else(|| eyre!("ticket not in reservation list"))?;

        if status.contains('-') {
            sleep(Duration::from_millis(250));
        } else {
            return Ok(());
        }
    }
}

fn args(id: usize, bench: &Bench) -> String {
    use itertools::Itertools;
    let partitions = bench
        .partitions
        .iter()
        .map(|Partition { subtree, clerks }| format!("--partition {subtree}:{clerks}"));
    let partitions: String = Itertools::intersperse(partitions, " ".to_string()).collect();
    let args = format!("--id {id} --database /tmp/govfs --run 0 {partitions}");
    args
}

#[instrument(skip(bench))]
pub fn start_cluster(
    bench: &bench::Bench,
    nodes: &[Node],
) -> Result<FuturesUnordered<impl Future<Output = Result<Output, std::io::Error>>>> {
    let mut path = env::current_dir()?;
    path.push("bin/node");
    let path = path.to_str().unwrap();

    let nodes: FuturesUnordered<_> = nodes
        .iter()
        .enumerate()
        .map(|(id, node)| {
            let args = args(id, bench);
            tokio::process::Command::new("ssh")
                .arg("-t")
                .arg(node)
                .arg(format!(
                    "rm -rf /tmp/govfs
                mkdir -p /tmp/govfs
                {path} {args}
            "
                ))
                .output()
        })
        .collect();
    Ok(nodes)
}

async fn ssh_client(path: String, node: String, args: String) -> Result<String> {
    tokio::process::Command::new("ssh")
        .arg("-t")
        .arg(node)
        .arg(format!("{path} {args}"))
        .output()
        .await
        .wrap_err("test")?
        .wrap()
}

#[instrument]
pub fn start_clients(
    command: bench::Command,
    nodes: &[Node],
) -> Result<FuturesUnordered<impl Future<Output = Result<String>>>> {
    let mut path = env::current_dir()?;
    path.push("bin/bench_client");
    let path = path.to_str().unwrap();
    let args = command.args();
    let nodes: FuturesUnordered<_> = nodes
        .iter()
        .map(|node| ssh_client(path.to_string(), node.to_string(), args.clone()))
        .collect();
    Ok(nodes)
}

trait WrapOutput {
    fn wrap(self) -> Result<String, Report>;
}

impl WrapOutput for Output {
    fn wrap(self) -> Result<String, Report> {
        if !self.status.success() {
            let stderr = String::from_utf8(self.stderr).unwrap();
            let stdout = String::from_utf8(self.stdout).unwrap();
            Err(eyre!("Failed to reserve nodes")
                .with_note(|| format!("stderr: {stderr}"))
                .with_note(|| format!("stdout: {stdout}")))
        } else {
            let lines = String::from_utf8(self.stdout).unwrap();
            Ok(lines)
        }
    }
}
