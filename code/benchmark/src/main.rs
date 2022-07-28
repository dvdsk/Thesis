use std::{
    net::{SocketAddr, ToSocketAddrs},
    ops::AddAssign,
    path::PathBuf,
    time::Duration,
};

use bench::Bench;
use benchmark::{bench, deploy, sync};
use clap::Parser;
use client::RandomNode;
use color_eyre::{eyre::eyre, eyre::WrapErr, Help, Report, Result, SectionExt};
use futures::{
    stream::{FuturesUnordered, StreamExt},
    Future,
};
use rand::prelude::*;
use tokio::time::{sleep, timeout};
use tracing::{info, warn};

#[derive(clap::Subcommand, Clone, Debug)]
pub enum Command {
    Ls,
    RangeVsRowlen,
    RangeVsNWriters,
    Touch,
}

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    #[clap(subcommand)]
    command: Command,
}

async fn watch_nodes(
    cluster: &mut FuturesUnordered<impl Future<Output = Result<String, Report>>>,
    clients: &mut FuturesUnordered<impl Future<Output = Result<String, Report>>>,
) -> Result<Vec<String>> {
    let mut output = Vec::new();

    loop {
        tokio::select! {
            err = cluster.next() => {
                return match err.expect("should always be more then one node") {
                    Ok(output) => Err(eyre!("node exits early"))
                        .with_section(move || output.header("Output:")),
                    Err(err) => Err(err).wrap_err("node failed"),
                };
            }
            res = clients.next() => {
                match res {
                    Some(res) => {
                        let out = res.wrap_err("client failed")?;
                        output.push(out);
                    }
                    None => return Ok(output), // all clients are done
                }
            }
        }
    }
}

struct NodesList {
    list: Vec<SocketAddr>,
}

impl NodesList {
    fn from_hostnames(names: &[String], port: u16) -> Result<Self> {
        let mut list: Vec<SocketAddr> = Vec::new();
        for host_name in names {
            let addr: SocketAddr = format!("{host_name}:{port}")
                .to_socket_addrs()
                .wrap_err("Could not resolve host name")
                .with_section(|| host_name.to_owned().header("hostname:"))?
                .next()
                .ok_or_else(|| {
                    eyre!("Did not resolve to any adress")
                        .with_section(|| host_name.to_owned().header("hostname:"))
                })?;
            list.push(addr);
        }
        Ok(Self { list })
    }
}

#[async_trait::async_trait]
impl RandomNode for NodesList {
    async fn random_node(&self) -> SocketAddr {
        let mut rng = rand::thread_rng();
        self.list.iter().choose(&mut rng).unwrap().clone()
    }
}

async fn run_benchmark(
    run_numb: usize,
    bench: &Bench,
    ports: Ports,
    command: &bench::Command,
) -> Result<Vec<String>> {
    let nodes = deploy::reserve(bench.needed_nodes())?;

    let mut cluster = deploy::start_cluster(
        &bench,
        &nodes[0..bench.fs_nodes()],
        ports.president,
        ports.minister,
        ports.client,
    )?;
    // workaround for cluster
    // we start the benchmark
    sleep(Duration::from_millis(200)).await;

    let find_nodes = NodesList::from_hostnames(&nodes, ports.client)?;
    let mut client = client::Client::new(find_nodes);
    tokio::select! {
        err = cluster.next() => {
            return match err.expect("should always be more then one node") {
                Ok(output) => Err(eyre!("node exits early"))
                    .with_section(move || output.header("Output:")),
                Err(err) => Err(err).wrap_err("node failed"),
            };
        }
        _ = bench.prep(&mut client) => (),
    }
    // workaround to ensure create is done by all clerks before
    // we start the benchmark
    sleep(Duration::from_millis(200)).await;

    let server = sync::server(bench.client_nodes());
    let mut clients = deploy::start_clients(
        command,
        &nodes[bench.fs_nodes()..],
        run_numb,
        bench.clients_per_node,
    )?;

    tokio::select! {
        res = server.block_till_synced() => {
            info!("benchmark clients now synced");
            res?;
        }
        res = watch_nodes(&mut cluster, &mut clients) => {
            return Err(res.expect_err("benchmark can not be done before clients are synced"));
        }
    }

    let output = watch_nodes(&mut cluster, &mut clients).await?;
    cluster.clear(); /* TODO: see if we can remove this <dvdsk noreply@davidsk.dev> */
    clients.clear();
    Ok(output)
}

fn results_present(run_numb: usize, command: &bench::Command, bench: &Bench) -> bool {
    let file = command.results_file("split");
    let mut dir = PathBuf::from("..");
    dir.push(file.parent().unwrap());

    if !dir.exists() {
        return false;
    }

    let files_for_run_numb = std::fs::read_dir(dir)
        .unwrap()
        .map(Result::unwrap)
        .map(|entry| entry.path())
        .filter(|p| p.is_file())
        .map(|p| {
            let name = p.file_stem().unwrap().to_str().unwrap();
            let digit = name.split_once('_').unwrap().1;
            digit.parse::<usize>().unwrap()
        })
        .filter(|run| *run == run_numb)
        .count();

    files_for_run_numb == bench.client_nodes()
}

#[derive(Debug, Clone)]
struct Ports {
    pub president: u16,
    pub minister: u16,
    pub client: u16,
}

impl Default for Ports {
    fn default() -> Self {
        Self {
            president: 65000,
            minister: 65100,
            client: 65400,
        }
    }
}

impl AddAssign<u16> for Ports {
    fn add_assign(&mut self, rhs: u16) {
        self.president += rhs;
        self.minister += rhs;
        self.client += rhs;
    }
}

async fn bench_until_success(
    ports: &mut Ports,
    run_numb: usize,
    command: bench::Command,
    max_duration: Duration,
) {
    let bench = Bench::from(&command, 0);
    if results_present(run_numb, &command, &bench) {
        info!("skipping run: {run_numb}, found results");
        return;
    }

    let output = loop {
        *ports += 1; // socket might close inproperly, increment ports
                     // so we need not wait for the host to make the port availible again
        let benchmark = run_benchmark(run_numb, &bench, ports.clone(), &command);

        match timeout(max_duration, benchmark).await {
            Ok(Ok(output)) => {
                break output;
            }
            Ok(Err(err)) => {
                warn!("run failed retrying, err: {err:?}");
                panic!();
            }
            Err(_) => warn!("benchmark timed out"),
        }
    };
    info!("benchmark output: {output:?}");
}

async fn bench_ls(mut ports: Ports) {
    let max_duration = Duration::from_secs(120);
    for run_numb in 0..5 {
        for n_parts in 1..=5 {
            let command = bench::Command::LsBatch { n_parts };
            bench_until_success(&mut ports, run_numb, command, max_duration).await;
            let command = bench::Command::LsStride { n_parts };
            bench_until_success(&mut ports, run_numb, command, max_duration).await;
            println!("bench: nparts {n_parts}, run_numb: {run_numb} completed!");
        }
    }
}

async fn bench_touch(mut ports: Ports) {
    let max_duration = Duration::from_secs(120);
    for run_numb in 0..5 {
        for n_parts in 1..=5 {
            let command = bench::Command::Touch { n_parts };
            bench_until_success(&mut ports, run_numb, command, max_duration).await;
            println!("bench: nparts {n_parts}, run_numb: {run_numb} completed!");
        }
    }
}

async fn bench_range_vs_n_writers(mut ports: Ports) {
    const MAX_PER_NODE: f32 = 4.0;
    let max_duration = Duration::from_secs(120);
    let rows_len = 1_000;
    for run_numb in 0..5 {
        for clients in [1, 2, 4, 8, 16, 32] {
            let client_nodes = ((clients as f32) / MAX_PER_NODE).ceil() as usize;
            let clients_per_node = clients / client_nodes;
            let command = bench::Command::RangeByRow {
                rows_len,
                clients_per_node,
                client_nodes,
            };
            bench_until_success(&mut ports, run_numb, command, max_duration).await;
            let command = bench::Command::RangeWholeFile {
                rows_len,
                clients_per_node,
                client_nodes,
            };
            bench_until_success(&mut ports, run_numb, command, max_duration).await;
            dbg!(run_numb, clients, clients_per_node, client_nodes);
        }
    }
}

async fn bench_range_vs_rowlen(mut ports: Ports) {
    let max_duration = Duration::from_secs(120);
    for run_numb in 0..5 {
        for rows_len in [1_000, 10_000, 100_000, 1_000_000] {
            let command = bench::Command::RangeByRow {
                rows_len,
                clients_per_node: 3,
                client_nodes: 3,
            };
            bench_until_success(&mut ports, run_numb, command, max_duration).await;
            let command = bench::Command::RangeWholeFile {
                rows_len,
                clients_per_node: 3,
                client_nodes: 3,
            };
            bench_until_success(&mut ports, run_numb, command, max_duration).await;
            println!("run_numb: {run_numb} (rows len: {rows_len}) completed!");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_tracing();
    color_eyre::install().unwrap();
    // These port numbers are "randomly" picked to be usually free
    // on cluster nodes. Feel free to move them around
    let ports = Ports::default();
    let args = Args::parse();

    match args.command {
        Command::Ls => bench_ls(ports).await,
        Command::Touch => bench_touch(ports).await,
        Command::RangeVsRowlen => bench_range_vs_rowlen(ports).await,
        Command::RangeVsNWriters => bench_range_vs_n_writers(ports).await,
    }

    Ok(())
}

fn setup_tracing() {
    use tracing_error::ErrorLayer;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{filter, fmt};

    let filter = filter::EnvFilter::builder()
        .parse("warn,benchmark=info,benchmark::deploy=warn")
        .unwrap();

    let uptime = fmt::time::uptime();
    let fmt_layer = fmt::layer()
        .pretty()
        .with_line_number(true)
        .with_timer(uptime);

    let _ignore_err = tracing_subscriber::registry()
        .with(ErrorLayer::default())
        .with(filter)
        .with(fmt_layer)
        .try_init();
}
