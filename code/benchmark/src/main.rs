use std::{
    net::{SocketAddr, ToSocketAddrs},
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
use tokio::time::sleep;
use tracing::{info, warn};

#[derive(clap::Subcommand, Clone, Debug)]
pub enum Command {
    Ls,
    Range,
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
    pres_port: u16,
    min_port: u16,
    client_port: u16,
    command: &bench::Command,
) -> Result<Vec<String>> {
    let nodes = deploy::reserve(bench.needed_nodes())?;

    let mut cluster = deploy::start_cluster(
        &bench,
        &nodes[0..bench.fs_nodes()],
        pres_port,
        min_port,
        client_port,
    )?;
    // workaround for cluster
    // we start the benchmark
    sleep(Duration::from_millis(200)).await;

    let find_nodes = NodesList::from_hostnames(&nodes, client_port)?;
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
    sleep(Duration::from_millis(2000)).await;

    let server = sync::server(bench.client_nodes());
    let mut clients = deploy::start_clients(command, &nodes[bench.fs_nodes()..], run_numb)?;

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

async fn bench_until_success(
    pres_port: u16,
    min_port: u16,
    client_port: u16,
    i: &mut u16,
    run_numb: usize,
    command: bench::Command,
) {
    let bench = Bench::from(&command);
    let output = loop {
        *i += 1; // socket might close inproperly, increment ports
                 // so we need not wait for the host to make the port availible again
        match run_benchmark(
            run_numb,
            &bench,
            pres_port + *i,
            min_port + *i,
            client_port + *i,
            &command,
        )
        .await
        {
            Ok(output) => {
                break output;
            }
            Err(err) => {
                warn!("run failed retrying, err: {err:?}");
                panic!();
            }
        }
    };
    info!("benchmark output: {output:?}");
}

async fn bench_ls(pres_port: u16, min_port: u16, client_port: u16) {
    let mut i = 0;
    for run_numb in 0..5 {
        for n_parts in 1..=5 {
            let command = bench::Command::LsBatch { n_parts };
            bench_until_success(pres_port, min_port, client_port, &mut i, run_numb, command).await;
            let command = bench::Command::LsStride { n_parts };
            bench_until_success(pres_port, min_port, client_port, &mut i, run_numb, command).await;
            info!("bench: nparts {n_parts}, run_numb: {run_numb} completed!");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_tracing();
    color_eyre::install().unwrap();
    // These port numbers are "randomly" picked to be usually free
    // on cluster nodes. Feel free to move them around
    let (pres_port, min_port, client_port) = (65000, 65100, 65400);
    let args = Args::parse();

    // let command = bench::Command::LsBatch { n_parts: 3 };
    // bench_until_success(pres_port, min_port, client_port, &mut 1, 1, command).await;

    match args.command {
        Command::Ls => bench_ls(pres_port, min_port, client_port).await,
        _ => todo!(),
    }

    Ok(())
}

fn setup_tracing() {
    use tracing_error::ErrorLayer;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::{filter, fmt};

    let filter = filter::EnvFilter::builder()
        // .parse("warn,benchmark=info,benchmark::deploy=warn")
        .parse("info")
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
