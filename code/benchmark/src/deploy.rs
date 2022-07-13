use color_eyre::{eyre::eyre, Help, Report, Result, SectionExt};
use futures::stream::FuturesUnordered;
use node::WrapErr;
use std::{
    env,
    ffi::OsString,
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
        .wrap("preserve failed to list reservations")?;

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
            .wrap("preserve failed to cancel reservation")?;
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
        .wrap("preserve failed to reserve nodes")?;

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
    Command::new("preserve")
        .arg("-long-list")
        .output()
        .wrap_err("Could not find command")?
        .wrap("preserve failed to list reservations")
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
    let n_nodes = bench.fs_nodes();
    let partitions: String = Itertools::intersperse(partitions, " ".to_string()).collect();
    let args = format!("--id {id} --database /tmp/govfs --cluster-size {n_nodes} --run 0 {partitions}");
    args
}

async fn ssh_node(bin: String, node: String, args: String) -> Result<String> {
    let run_on_remote = format!(
        "rm -rf /tmp/govfs
mkdir -p /tmp/govfs
{bin} {args}
"
    );
    tokio::process::Command::new("ssh")
        .arg("-t")
        .arg(node)
        .arg(run_on_remote)
        .output()
        .await
        .wrap_err("could not start ssh")?
        .wrap("error on remote node")
}

/// expects node binary in $PWD/bin/
#[instrument(skip(bench))]
pub fn start_cluster(
    bench: &bench::Bench,
    nodes: &[Node],
) -> Result<FuturesUnordered<impl Future<Output = Result<String>>>> {
    let mut path = env::current_dir()?;
    path.push("bin/node");
    if !path.is_file() {
        return Err(eyre!("node binary missing")
            .suggestion("compile node and place in bin")
            .suggestion("use `make benchmark` from the project root"));
    }
    let path = path.to_str().unwrap();

    let nodes: FuturesUnordered<_> = nodes
        .iter()
        .enumerate()
        .map(|(id, node)| {
            let args = args(id, bench);
            ssh_node(path.to_string(), node.to_string(), args)
        })
        .collect();
    info!("started cluster nodes");
    Ok(nodes)
}

async fn ssh_client(bin: OsString, node: String, args: OsString) -> Result<String> {
    let mut run_on_remote = bin;
    run_on_remote.push(" ");
    run_on_remote.push(args);

    tokio::process::Command::new("ssh")
        .arg("-t")
        .arg(node)
        .arg(run_on_remote)
        .output()
        .await
        .wrap_err("test")?
        .wrap("error on remote node")
}

/// expects bench_client binary in $PWD/bin/
#[instrument]
pub fn start_clients(
    command: bench::Command,
    nodes: &[Node],
) -> Result<FuturesUnordered<impl Future<Output = Result<String>>>> {
    let mut path = env::current_dir()?;
    path.push("bin/bench_client");
    if !path.is_file() {
        return Err(eyre!("bench_client binary missing")
            .suggestion("compile node and place in bin")
            .suggestion("use `make benchmark` from the project root"));
    }
    let path = path.to_str().unwrap();
    let mut args = gethostname::gethostname();
    args.push(" ");
    args.push(command.args());
    let nodes: FuturesUnordered<_> = nodes
        .iter()
        .map(|node| ssh_client(path.into(), node.to_string(), args.clone()))
        .collect();
    Ok(nodes)
}

pub trait WrapOutput {
    fn wrap(self, msg: &'static str) -> Result<String, Report>;
}

impl WrapOutput for Output {
    fn wrap(self, msg: &'static str) -> Result<String, Report> {
        if !self.status.success() {
            let stderr = String::from_utf8(self.stderr).unwrap();
            let stdout = String::from_utf8(self.stdout).unwrap();
            Err(eyre!(msg)
                .with_section(|| stderr.trim().to_string().header("Stderr:"))
                .with_section(|| stdout.trim().to_string().header("Stdout:")))
        } else {
            let lines = String::from_utf8(self.stdout).unwrap();
            Ok(lines)
        }
    }
}
